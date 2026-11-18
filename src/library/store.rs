//! 书架持久化：`library.json` 的加载 / 保存 / 损坏兜底。
//!
//! 本文件只负责「记录集合」本身；扫描合并、导入、进度等业务变更在后续任务里追加。
//! store 不启动线程、不持有 channel —— 扫描与元数据由 `desktop/model` 调度。

use std::collections::HashSet;
use std::path::Path;

use rust_i18n::t;
use serde::{Deserialize, Serialize};

use super::book::{Book, id_for_key, now_unix};
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
}
