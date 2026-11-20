//! `AppModel` 书架与阅读进度方法：扫描调度、元数据工作线程、导入 / 编辑 / 删除、进度落盘。

use std::path::{Path, PathBuf};

use rust_i18n::t;

use super::{AppModel, MetaEvent, Scan, ScanEvent};
use crate::error::AppError;
use crate::library::{AppliedMeta, Book, ReadingMode, ReadingProgress};

/// 删除文件；文件本就不存在视为成功（missing 书记录也要能删掉）。
fn remove_file_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

impl AppModel {
    /// 扫描下载目录（异步）。重复触发由 `scan_in_flight` 拦截。
    pub fn refresh_library_async(&mut self) {
        if self.library.scan_in_flight {
            return;
        }
        // 与书架数据里的 `path_key` 共用 `absolute_dir`，否则
        // 「相对路径 vs 绝对路径」永远对不上 → 同一目录被反复当成新目录扫描。
        let dir = crate::library::absolute_dir(Path::new(&self.config.download.download_path));
        // 目录还不存在时不记「已扫描」：下载目录可能稍后才被创建，
        // 记下了书架页的 `maybe_auto_scan` 就再也不会自动重扫。
        if !dir.exists() {
            return;
        }
        self.library.scanned_dir = Some(dir.clone());
        self.library.last_error = None;
        let (tx, rx) = smol::channel::unbounded::<ScanEvent>();
        self.library.scan_rx = Some(rx);
        self.library.scan_in_flight = true;
        let runtime = self.runtime;
        runtime.spawn(async move {
            let root = dir.clone();
            let result =
                tokio::task::spawn_blocking(move || crate::library::scan_download_dir(&dir)).await;
            let event = match result {
                Ok(Ok(files)) => Ok(Scan { root, files }),
                Ok(Err(error)) => Err(AppError::io_msg(&error, t!("Library.scan_failed"))),
                Err(error) => Err(AppError::internal(error.to_string())),
            };
            let _ = tx.send(event).await;
        });
    }

    /// 启动一个元数据工作线程；同一时刻只有一个，drain 结束后再启动下一个。
    pub fn spawn_meta_worker(&mut self) {
        if self.library.meta_in_flight {
            return;
        }
        loop {
            let Some(id) = self.library.meta_queue.pop_front() else {
                return;
            };
            let Some(book) = self.library.store.get(&id).cloned() else {
                continue;
            };
            let (tx, rx) = smol::channel::unbounded::<MetaEvent>();
            self.library.meta_rx = Some(rx);
            self.library.meta_in_flight = true;
            let covers_dir = self.covers_dir();
            let path = PathBuf::from(&book.path);
            let format = book.format;
            let cover_id = id.clone();
            let runtime = self.runtime;
            runtime.spawn(async move {
                let outcome = tokio::task::spawn_blocking(move || {
                    let meta = crate::library::meta::extract(&path, format);
                    let cover_file = meta.cover.as_deref().and_then(|bytes| {
                        covers_dir
                            .as_deref()
                            .and_then(|dir| crate::library::write_cover(dir, &cover_id, bytes))
                    });
                    AppliedMeta {
                        title: meta.title,
                        author: meta.author,
                        intro: meta.intro,
                        cover_file,
                        encoding: meta.encoding,
                    }
                })
                .await
                .map_err(|error| error.to_string());
                let _ = tx.send(MetaEvent { id, outcome }).await;
            });
            return;
        }
    }

    /// 书架落盘；失败只推一次错误，且失败同样计入 5 秒节流。
    ///
    /// 落盘错误记在 `last_save_error` 而不是 `last_error`：后者是扫描错误，
    /// 下一次扫描成功会把它清掉，混用会让两种失败互相掩盖。
    pub fn persist_library(&mut self) {
        self.library.last_save_attempt_unix = crate::library::now_unix();
        let outcome = self.library.store.to_json().and_then(|bytes| {
            self.state
                .put(crate::db::KEY_LIBRARY, &bytes)
                .map_err(|error| AppError::internal(error.to_string()))
        });
        match outcome {
            Ok(()) => {
                self.library.store.clear_dirty();
                self.library.last_save_unix = crate::library::now_unix();
                self.library.last_save_error = None;
            }
            Err(error) => {
                let message = error.message();
                // 同一个错误只推一次 toast：drain 是 100ms 循环，重试失败不能刷屏。
                if self.library.last_save_error.as_deref() != Some(message.as_str()) {
                    self.library.last_save_error = Some(message.clone());
                    self.push_error(message);
                }
            }
        }
    }

    /// 关闭阅读器 / 退出应用时强制落盘。
    pub fn flush_library(&mut self) {
        self.persist_library();
        self.persist_settings();
    }

    /// 阅读缓存的根目录（`<下载目录>/.sonovel`）；下载目录为空时 `None`。
    #[must_use]
    pub fn reader_cache_dir(&self) -> Option<PathBuf> {
        crate::library::cache_root(&self.config.download.download_path)
    }

    /// 封面缓存目录（`<下载目录>/.sonovel/covers`）；下载目录为空时 `None`。
    ///
    /// 现算而不是存字段：设置页改下载目录后，下一次读写自然落到新目录。
    #[must_use]
    pub fn covers_dir(&self) -> Option<PathBuf> {
        let root = crate::library::cache_root(&self.config.download.download_path)?;
        crate::library::hide_dir(&root);
        Some(root.join(crate::library::COVERS_DIR_NAME))
    }

    /// 删掉一本书的阅读缓存。书没了缓存留着也没人会用，只会白占小说目录空间。
    fn forget_reader_cache(&self, id: &str) {
        if let Some(dir) = self.reader_cache_dir() {
            crate::reader::cache::remove(&dir, id);
        }
    }

    /// 取一本书的快照；书籍已不存在时返回 `None`。
    #[must_use]
    pub fn book_snapshot(&self, id: &str) -> Option<Book> {
        self.library.store.get(id).cloned()
    }

    /// 只取书签列表：`book_snapshot` 会整本 clone，书签抽屉每帧都要用，别让它带整本书。
    #[must_use]
    pub fn bookmarks_of(&self, id: &str) -> Option<Vec<crate::library::Bookmark>> {
        self.library
            .store
            .get(id)
            .map(|book| book.bookmarks.clone())
    }

    /// 阅读器打开成功后记录章节数与编码。
    pub fn mark_book_opened(&mut self, id: &str, chapter_count: u32, encoding: Option<String>) {
        self.library.store.mark_opened(id, chapter_count, encoding);
        self.library.bump();
    }

    /// 切换收藏并立即落盘（书架卡片的星标）。
    pub fn toggle_book_favorite(&mut self, id: &str) {
        let Some(book) = self.library.store.get(id) else {
            return;
        };
        let next = !book.favorite;
        self.library.store.set_favorite(id, next);
        self.library.bump();
        self.persist_library();
    }

    /// 记录阅读位置（只改内存 + 标脏，由 drain 每 5s 落盘）。
    pub fn record_reading_progress(&mut self, id: &str, progress: &ReadingProgress) {
        self.library.store.record_progress(
            id,
            progress.chapter,
            progress.chapter_ratio,
            progress.mode,
            progress.finished,
        );
        self.library.bump();
    }

    /// 只更新阅读方式（保留当前章节位置）。
    pub fn set_reading_mode(&mut self, id: &str, mode: ReadingMode) {
        let Some(book) = self.library.store.get(id) else {
            return;
        };
        let (chapter, ratio, finished) = (
            book.progress.chapter,
            book.progress.chapter_ratio,
            book.progress.finished,
        );
        self.library
            .store
            .record_progress(id, chapter, ratio, mode, finished);
        self.library.bump();
    }

    /// 添加书签；重复位置返回 `false`。立即落盘。
    #[must_use]
    pub fn add_reading_bookmark(&mut self, id: &str, bookmark: crate::library::Bookmark) -> bool {
        let added = self.library.store.add_bookmark(id, bookmark);
        if added {
            self.library.bump();
            self.persist_library();
        }
        added
    }

    /// 删除书签并立即落盘。
    pub fn remove_reading_bookmark(&mut self, id: &str, bookmark_id: &str) {
        self.library.store.remove_bookmark(id, bookmark_id);
        self.library.bump();
        self.persist_library();
    }

    /// 导入一批文件（引用式，不复制）。完成后启动元数据提取。
    pub fn import_library_files(&mut self, paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }
        let outcome = self.library.store.import_files(paths);
        self.library.enqueue_meta(outcome.meta_ids);
        self.library.bump();
        self.persist_library();
        for (path, error) in &outcome.failed {
            self.push_warning(
                t!(
                    "Library.import.failed_item",
                    file = path.display().to_string(),
                    err = error
                )
                .to_string(),
            );
        }
        if outcome.added + outcome.existing > 0 {
            self.push_success(
                t!(
                    "Library.import.done",
                    added = &outcome.added.to_string(),
                    existing = &outcome.existing.to_string()
                )
                .to_string(),
            );
        }
        self.spawn_meta_worker();
    }

    /// 记录用户在阅读器里手动指定的编码，下次打开不再重新探测。
    pub fn set_book_encoding(&mut self, id: &str, encoding: Option<String>) {
        if self.library.store.set_encoding(id, encoding) {
            self.library.bump();
            self.persist_library();
        }
    }

    /// 这本书是否位于当前下载目录内（扫描会再次发现它）。
    ///
    /// 按路径判定而不是 `origin`：导入书后来被挪进下载目录后，扫描同样会把它收回来，
    /// 这类书必须隐藏而不是删记录 —— 否则下次扫描又冒出来，「移出」等于没生效。
    fn book_is_scanned(&self, book: &Book) -> bool {
        let download_path = &self.config.download.download_path;
        if download_path.trim().is_empty() {
            return false;
        }
        // 与 `refresh_library_async` 用同一个根：先 `absolute_dir`，再用与 `path_key`
        // 完全相同的规则规范化（扫描合并里也是先 `absolute_dir` 再 `normalize_path_key`）。
        // 这里不 trim：扫描器不 trim，比较要回答的是「下一次扫描会不会找到这个文件」。
        let root = crate::library::absolute_dir(Path::new(download_path));
        let root_key = crate::library::normalize_path_key(&root);
        crate::library::path_key_is_within(&book.path_key, &root_key)
    }

    /// 移出书架：扫描范围内的书只隐藏（重扫不会再加回），范围外直接删记录。
    pub fn hide_book(&mut self, id: &str) {
        let Some(book) = self.library.store.get(id).cloned() else {
            return;
        };
        if self.book_is_scanned(&book) {
            self.library.store.set_hidden(id, true);
        } else {
            if let Some(dir) = self.covers_dir() {
                crate::library::remove_cover_file(&dir, book.cover_file.as_deref());
            }
            let _ = self.library.store.remove(id);
        }
        // 移出书架的两种结果都不再能打开，缓存跟着删。
        self.forget_reader_cache(&book.id);
        self.library.bump();
        self.persist_library();
    }

    /// 删除磁盘文件并移除记录。
    pub fn delete_book_file(&mut self, id: &str) {
        let Some(book) = self.library.store.get(id).cloned() else {
            return;
        };
        if let Err(error) = remove_file_if_exists(Path::new(&book.path)) {
            self.push_error(t!("Library.delete.failed", err = error.to_string()).to_string());
            return;
        }
        if let Some(dir) = self.covers_dir() {
            crate::library::remove_cover_file(&dir, book.cover_file.as_deref());
        }
        let _ = self.library.store.remove(id);
        self.forget_reader_cache(&book.id);
        self.library.bump();
        self.persist_library();
    }

    /// 重新定位失效文件；封面缓存文件名跟随新 ID 改名。
    ///
    /// # Errors
    ///
    /// 书籍不存在、格式不一致、目标文件已在书架中时返回 [`AppError`]。
    pub fn relink_book(&mut self, id: &str, new_path: &Path) -> Result<String, AppError> {
        let old_cover = self
            .library
            .store
            .get(id)
            .and_then(|book| book.cover_file.clone());
        let new_id = {
            // 与扫描共用同一个根：新路径在下载目录内才算「下载书」，
            // 否则 relink 会把它转为导入来源，避免下次扫描误标失效。
            let trimmed = self.config.download.download_path.trim();
            let root =
                (!trimmed.is_empty()).then(|| crate::library::absolute_dir(Path::new(trimmed)));
            self.library.store.relink(id, new_path, root.as_deref())?
        };
        // id 变了就是另一条路径的缓存键：旧缓存不会再有命中者。
        if new_id != id {
            self.forget_reader_cache(id);
        }
        let renamed = self.covers_dir().as_deref().and_then(|dir| {
            crate::library::cover::rename_cover_for_relink(dir, id, &new_id, old_cover.as_deref())
        });
        if old_cover.is_some() {
            self.library
                .store
                .set_cover_file(&new_id, renamed.or(old_cover));
        }
        self.library.bump();
        self.persist_library();
        Ok(new_id)
    }

    /// 从头开始：清空阅读进度、保留阅读方式并立即落盘。
    pub fn restart_book(&mut self, id: &str) {
        let Some(book) = self.library.store.get(id) else {
            return;
        };
        let mode = book.progress.mode;
        self.library.store.record_progress(id, 0, 0.0, mode, false);
        self.library.bump();
        self.persist_library();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn remove_file_treats_missing_as_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone.txt");
        assert!(super::remove_file_if_exists(&path).is_ok());
        std::fs::write(&path, b"x").unwrap();
        assert!(super::remove_file_if_exists(&path).is_ok());
        assert!(!path.exists());
    }
}
