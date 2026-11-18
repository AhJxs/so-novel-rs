//! 书架持久化：`library.json` 的加载 / 保存 / 损坏兜底。
//!
//! 本文件只负责「记录集合」本身；扫描合并、导入、进度等业务变更在后续任务里追加。
//! store 不启动线程、不持有 channel —— 扫描与元数据由 `desktop/model` 调度。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rust_i18n::t;
use serde::{Deserialize, Serialize};

use super::book::{Book, BookFormat, BookOrigin, id_for_key, normalize_path_key, now_unix};
use super::scan::ScannedFile;
use crate::error::{AppError, AppResult};

/// 当前书架文件版本。
pub const LIBRARY_VERSION: u32 = 1;

/// `library.json` 的根结构。
#[derive(Debug, Serialize, Deserialize)]
struct LibraryFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    books: Vec<Book>,
}

/// 后台工作线程把封面写入 `covers_dir` 之后的结果；drain 只做内存回写，不碰磁盘。
#[derive(Debug, Clone, Default)]
pub struct AppliedMeta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub cover_file: Option<String>,
    pub encoding: Option<String>,
}

/// 一次导入（可能多选）的结果统计。
#[derive(Debug, Default)]
pub struct ImportOutcome {
    pub added: usize,
    pub existing: usize,
    /// 失败项及错误文案（文件不存在 / 不支持的后缀）。
    pub failed: Vec<(PathBuf, String)>,
    /// 需要后台提取元数据的书籍 id。
    pub meta_ids: Vec<String>,
}

/// 书架内存状态 + 脏标记。
#[derive(Debug, Default)]
pub struct LibraryStore {
    books: Vec<Book>,
    dirty: bool,
    /// 文件版本高于本程序支持的版本：只读，拒绝覆盖，避免把新版数据写回旧格式。
    future_version: bool,
}

impl LibraryStore {
    /// 读取书架文件。
    ///
    /// 返回值第二项是给用户看的提示（损坏备份 / 版本过高），`None` 表示一切正常。
    /// 损坏时把原文件改名为 `library.json.corrupt-<unix>` 再以空库启动，
    /// 保证用户数据不会被下一次保存直接抹掉。
    #[tracing::instrument(name = "library::LibraryStore::load", skip_all, fields(path = %path.display()))]
    pub fn load(path: &Path) -> (Self, Option<String>) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), None);
            }
            Err(error) => {
                return (
                    Self::default(),
                    Some(t!("Library.error.load_failed", detail = error.to_string()).to_string()),
                );
            }
        };
        match serde_json::from_slice::<LibraryFile>(&bytes) {
            Ok(file) => {
                let mut store = Self {
                    books: file.books,
                    dirty: false,
                    future_version: file.version > LIBRARY_VERSION,
                };
                store.normalize_ids();
                let warning = store.future_version.then(|| {
                    t!(
                        "Library.error.future_version",
                        version = file.version,
                        supported = LIBRARY_VERSION
                    )
                    .to_string()
                });
                (store, warning)
            }
            Err(error) => {
                let backup = path.with_extension(format!("json.corrupt-{}", now_unix()));
                let rename_error = std::fs::rename(path, &backup).err();
                let mut message = t!(
                    "Library.error.corrupt_backup",
                    path = backup.display().to_string(),
                    detail = error.to_string()
                )
                .to_string();
                if let Some(rename_error) = rename_error {
                    message.push_str(&t!(
                        "Library.error.backup_failed",
                        detail = rename_error.to_string()
                    ));
                }
                (Self::default(), Some(message))
            }
        }
    }

    /// 把书架写回磁盘（原子写）。
    ///
    /// # Errors
    ///
    /// 版本过高拒写、序列化失败、目录创建或写盘失败时返回 [`AppError`]。
    #[tracing::instrument(name = "library::LibraryStore::save", skip_all, fields(path = %path.display()))]
    pub fn save(&self, path: &Path) -> AppResult<()> {
        if self.future_version {
            return Err(AppError::business(
                t!("Library.error.save_refused").to_string(),
            ));
        }
        let file = LibraryFile {
            version: LIBRARY_VERSION,
            books: self.books.clone(),
        };
        let json = serde_json::to_vec_pretty(&file)
            .map_err(|error| AppError::internal(error.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| AppError::io_msg(&error, t!("Library.error.create_dir_failed")))?;
        }
        crate::db::write_atomically(path, &json)
            .map_err(|error| AppError::io_msg(&error, t!("Library.error.write_failed")))
    }

    /// 全部书籍（含隐藏项，UI 自行过滤）。
    #[must_use]
    pub fn books(&self) -> &[Book] {
        &self.books
    }

    /// 按 ID 取书。
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Book> {
        self.books.iter().find(|book| book.id == id)
    }

    /// 是否有未落盘的变更。
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 保存成功后清除脏标记。
    pub const fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// 标记有变更。后续任务的写方法统一调它。
    // 当前生产代码尚未接入写方法，只有测试调用；保留以免后续任务重复造轮子。
    #[allow(dead_code)]
    pub(super) const fn touch(&mut self) {
        self.dirty = true;
    }

    /// 重新按 `path_key` 计算 ID，并消解重复 ID（极低概率的哈希碰撞 / 手改文件）。
    fn normalize_ids(&mut self) {
        let mut seen: HashSet<String> = HashSet::new();
        for book in &mut self.books {
            let base = if book.path_key.is_empty() {
                book.id.clone()
            } else {
                id_for_key(&book.path_key)
            };
            let base = if base.is_empty() {
                id_for_key(&format!("index:{}", seen.len()))
            } else {
                base
            };
            let mut candidate = base.clone();
            let mut suffix = 1;
            while !seen.insert(candidate.clone()) {
                candidate = format!("{base}-{suffix}");
                suffix += 1;
            }
            book.id = candidate;
        }
    }

    /// 合并一次目录扫描结果，返回需要提取元数据的书籍 id。
    ///
    /// 规则见设计文档 3.1：新文件入队、变化文件按需重新提取、隐藏项跳过、
    /// 扫描结果里缺席的下载书标失效（记录保留，文件放回后自动恢复）。
    // `ScannedFile` 字段是 `mtime`，`Book` 字段是 `file_mtime`；clippy 误以为是手误。
    #[allow(clippy::suspicious_operation_groupings)]
    #[must_use]
    pub fn merge_scan(&mut self, files: &[ScannedFile]) -> Vec<String> {
        let mut meta_ids = Vec::new();
        let present: HashSet<String> = files
            .iter()
            .map(|file| normalize_path_key(&file.path))
            .collect();

        for file in files {
            let key = normalize_path_key(&file.path);
            if let Some(book) = self.books.iter_mut().find(|book| book.path_key == key) {
                if book.hidden {
                    continue;
                }
                let changed = book.size_bytes != file.size_bytes
                    || book.file_mtime != file.mtime
                    || book.missing;
                if changed {
                    book.size_bytes = file.size_bytes;
                    book.file_mtime = file.mtime;
                    book.missing = false;
                    self.dirty = true;
                    if !book.metadata_edited {
                        meta_ids.push(book.id.clone());
                    }
                }
                continue;
            }
            match Book::from_file(&file.path, BookOrigin::Downloaded) {
                Ok(book)
                    if !self
                        .books
                        .iter()
                        .any(|other| other.path_key == book.path_key) =>
                {
                    meta_ids.push(book.id.clone());
                    self.books.push(book);
                    self.dirty = true;
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!("扫描到的文件无法建书 {}: {error}", file.path.display());
                }
            }
        }

        for book in &mut self.books {
            let missing = match book.origin {
                BookOrigin::Downloaded => !present.contains(&book.path_key),
                BookOrigin::Imported => !Path::new(&book.path).exists(),
            };
            if book.missing != missing {
                book.missing = missing;
                self.dirty = true;
            }
        }
        meta_ids
    }

    /// 导入一批文件（只记录路径，不复制）。已存在则取消隐藏并刷新文件信息，保留进度。
    #[must_use]
    pub fn import_files(&mut self, paths: &[PathBuf]) -> ImportOutcome {
        let mut outcome = ImportOutcome::default();
        for path in paths {
            if BookFormat::from_path(path).is_none() {
                outcome.failed.push((
                    path.clone(),
                    t!("Library.error.unsupported_format").to_string(),
                ));
                continue;
            }
            let book = match Book::from_file(path, BookOrigin::Imported) {
                Ok(book) => book,
                Err(error) => {
                    outcome.failed.push((path.clone(), error));
                    continue;
                }
            };
            if let Some(existing) = self
                .books
                .iter_mut()
                .find(|other| other.path_key == book.path_key)
            {
                existing.hidden = false;
                existing.missing = false;
                existing.size_bytes = book.size_bytes;
                existing.file_mtime = book.file_mtime;
                outcome.existing += 1;
                self.dirty = true;
                continue;
            }
            outcome.meta_ids.push(book.id.clone());
            self.books.push(book);
            outcome.added += 1;
            self.dirty = true;
        }
        outcome
    }

    /// 回写后台提取的元数据；手动编辑过的书不接受自动覆盖。
    // 逐字段用 `changed` 累计是否真的写入了新值，比拆成五个 if 表达式更易读。
    #[allow(clippy::useless_let_if_seq)]
    pub fn apply_meta(&mut self, id: &str, meta: AppliedMeta) {
        let Some(book) = self.books.iter_mut().find(|book| book.id == id) else {
            return;
        };
        if book.metadata_edited {
            return;
        }
        let mut changed = false;
        if let Some(title) = meta.title.filter(|title| !title.trim().is_empty())
            && book.title != title
        {
            book.title = title;
            changed = true;
        }
        if let Some(author) = meta.author
            && book.author != author
        {
            book.author = author;
            changed = true;
        }
        if let Some(intro) = meta.intro
            && book.intro.as_deref() != Some(intro.as_str())
        {
            book.intro = Some(intro);
            changed = true;
        }
        if let Some(cover_file) = meta.cover_file
            && book.cover_file.as_deref() != Some(cover_file.as_str())
        {
            book.cover_file = Some(cover_file);
            changed = true;
        }
        if let Some(encoding) = meta.encoding
            && book.encoding.is_none()
        {
            book.encoding = Some(encoding);
            changed = true;
        }
        if changed {
            self.dirty = true;
        }
    }

    /// 手动编辑元数据；`cover_file = None` 表示移除封面。
    pub fn edit_metadata(
        &mut self,
        id: &str,
        title: &str,
        author: &str,
        cover_file: Option<String>,
    ) {
        let Some(book) = self.books.iter_mut().find(|book| book.id == id) else {
            return;
        };
        book.title = title.trim().to_string();
        book.author = author.trim().to_string();
        book.cover_file = cover_file;
        book.metadata_edited = true;
        self.dirty = true;
    }

    /// 更新失效标记。
    pub fn mark_missing(&mut self, id: &str, missing: bool) {
        if let Some(book) = self.books.iter_mut().find(|book| book.id == id)
            && book.missing != missing
        {
            book.missing = missing;
            self.dirty = true;
        }
    }

    /// 移出 / 放回书架（仅改变可见性，不碰磁盘）。
    pub fn set_hidden(&mut self, id: &str, hidden: bool) {
        if let Some(book) = self.books.iter_mut().find(|book| book.id == id)
            && book.hidden != hidden
        {
            book.hidden = hidden;
            self.dirty = true;
        }
    }

    /// 彻底删除记录（删除磁盘文件后调用）。
    #[must_use]
    pub fn remove(&mut self, id: &str) -> Option<Book> {
        let index = self.books.iter().position(|book| book.id == id)?;
        self.dirty = true;
        Some(self.books.remove(index))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::library::book::{BookFormat, BookOrigin};

    fn sample_book(dir: &std::path::Path, name: &str) -> Book {
        let path = dir.join(name);
        std::fs::write(&path, b"hello").unwrap();
        Book::from_file(&path, BookOrigin::Downloaded).unwrap()
    }

    #[test]
    fn missing_file_loads_empty_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let (store, warning) = LibraryStore::load(&dir.path().join("library.json"));
        assert!(store.books().is_empty());
        assert!(warning.is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        let mut store = LibraryStore::default();
        let mut book = sample_book(dir.path(), "书.txt");
        book.favorite = true;
        book.metadata_edited = true;
        book.title = "改过的书名".to_string();
        store.books.push(book.clone());

        store.save(&file).unwrap();
        let (loaded, warning) = LibraryStore::load(&file);
        assert!(warning.is_none());
        assert_eq!(loaded.books().len(), 1);
        let restored = loaded.get(&book.id).unwrap();
        assert_eq!(restored.title, "改过的书名");
        assert_eq!(restored.format, BookFormat::Txt);
        assert!(restored.favorite);
        assert!(restored.metadata_edited);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        std::fs::write(&file, b"{ not json").unwrap();

        let (store, warning) = LibraryStore::load(&file);
        assert!(store.books().is_empty());
        assert!(warning.is_some());
        assert!(!file.exists(), "损坏文件应被改名备份");
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("library.json.corrupt-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn duplicate_ids_are_reassigned() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        let json = r#"{
            "version": 1,
            "books": [
                {"id": "same", "path_key": "a", "title": "A"},
                {"id": "same", "path_key": "b", "title": "B"}
            ]
        }"#;
        std::fs::write(&file, json).unwrap();

        let (store, _) = LibraryStore::load(&file);
        let ids: Vec<_> = store.books().iter().map(|book| book.id.as_str()).collect();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(ids[0], id_for_key("a"));
    }

    #[test]
    fn future_version_refuses_to_save() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        std::fs::write(&file, r#"{"version": 99, "books": []}"#).unwrap();

        let (store, warning) = LibraryStore::load(&file);
        assert!(warning.is_some());
        assert!(store.save(&file).is_err());
    }

    #[test]
    fn dirty_flag_starts_clean() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = LibraryStore::load(&dir.path().join("library.json"));
        assert!(!store.is_dirty());
        store.touch();
        assert!(store.is_dirty());
        store.clear_dirty();
        assert!(!store.is_dirty());
    }

    fn scanned(path: &std::path::Path) -> crate::library::scan::ScannedFile {
        let metadata = std::fs::metadata(path).unwrap();
        crate::library::scan::ScannedFile {
            path: path.to_path_buf(),
            size_bytes: metadata.len(),
            mtime: crate::library::book::mtime_secs(&metadata),
        }
    }

    #[test]
    fn merge_adds_new_and_keeps_progress() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();

        let meta_ids = store.merge_scan(&[scanned(&path)]);
        assert_eq!(store.books().len(), 1);
        assert_eq!(meta_ids.len(), 1);
        assert_eq!(store.books()[0].origin, BookOrigin::Downloaded);
        assert!(store.is_dirty());

        let id = store.books()[0].id.clone();
        store.books[0].progress.chapter = 3;
        store.clear_dirty();

        let again = store.merge_scan(&[scanned(&path)]);
        assert!(again.is_empty(), "文件未变不应重复提取元数据");
        assert_eq!(store.get(&id).unwrap().progress.chapter, 3);
        assert!(!store.is_dirty());
    }

    #[test]
    fn merge_marks_missing_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let _ = store.merge_scan(&[scanned(&path)]);
        let id = store.books()[0].id.clone();

        let _ = store.merge_scan(&[]);
        assert!(store.get(&id).unwrap().missing);

        let _ = store.merge_scan(&[scanned(&path)]);
        assert!(!store.get(&id).unwrap().missing);
    }

    #[test]
    fn merge_skips_hidden_books() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let _ = store.merge_scan(&[scanned(&path)]);
        let id = store.books()[0].id.clone();
        store.set_hidden(&id, true);

        let _ = store.merge_scan(&[scanned(&path)]);
        assert!(store.get(&id).unwrap().hidden);
    }

    #[test]
    fn imported_missing_checks_disk_not_scan_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("imported.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let outcome = store.import_files(std::slice::from_ref(&path));
        assert_eq!(outcome.added, 1);
        let id = outcome.meta_ids[0].clone();

        let _ = store.merge_scan(&[]);
        assert!(!store.get(&id).unwrap().missing, "不在下载目录不等于失效");

        std::fs::remove_file(&path).unwrap();
        let _ = store.merge_scan(&[]);
        assert!(store.get(&id).unwrap().missing);
    }

    #[test]
    fn import_counts_existing_and_failures() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.txt");
        std::fs::write(&ok, b"one").unwrap();
        let bad = dir.path().join("bad.docx");
        std::fs::write(&bad, b"x").unwrap();
        let mut store = LibraryStore::default();

        let first = store.import_files(std::slice::from_ref(&ok));
        assert_eq!((first.added, first.existing), (1, 0));

        let second = store.import_files(&[ok, bad]);
        assert_eq!((second.added, second.existing), (0, 1));
        assert_eq!(second.failed.len(), 1);
        assert_eq!(store.books().len(), 1);
    }

    #[test]
    fn apply_meta_respects_manual_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let id = store.merge_scan(&[scanned(&path)]).remove(0);

        store.apply_meta(
            &id,
            AppliedMeta {
                title: Some("自动标题".to_string()),
                author: Some("自动作者".to_string()),
                ..AppliedMeta::default()
            },
        );
        assert_eq!(store.get(&id).unwrap().title, "自动标题");

        store.edit_metadata(&id, "手改标题", "手改作者", None);
        store.apply_meta(
            &id,
            AppliedMeta {
                title: Some("再次自动".to_string()),
                ..AppliedMeta::default()
            },
        );
        let book = store.get(&id).unwrap();
        assert_eq!(book.title, "手改标题");
        assert_eq!(book.author, "手改作者");
        assert!(book.metadata_edited);
    }

    #[test]
    fn remove_drops_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let id = store.merge_scan(&[scanned(&path)]).remove(0);
        assert!(store.remove(&id).is_some());
        assert!(store.books().is_empty());
        assert!(store.remove(&id).is_none());
    }
}
