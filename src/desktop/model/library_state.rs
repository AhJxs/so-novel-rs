//! 书架页状态：`LibraryStore` + 后台扫描 / 元数据通道 + 查询条件。
//!
//! 这里只排空 channel、更新 store 内存；落盘由 `AppModel::persist_library` 负责。

use std::collections::VecDeque;
use std::path::PathBuf;

use smol::channel::Receiver;

use crate::error::AppResult;
use crate::library::{AppliedMeta, LibraryStore, ScannedFile, ShelfQuery, now_unix};

/// 后台扫描结果。
pub type ScanEvent = AppResult<Vec<ScannedFile>>;

/// 单本书的元数据提取结果（封面已由工作线程落盘）。
#[derive(Debug)]
pub struct MetaEvent {
    pub id: String,
    pub outcome: Result<AppliedMeta, String>,
}

/// 书架页状态。
pub struct LibraryState {
    pub store: LibraryStore,
    /// store 内容每次变化 +1；UI 用它判定是否需要重绘。
    pub books_version: u64,
    pub query: ShelfQuery,
    /// 待提取元数据的书籍 id，按入队顺序出队。
    pub meta_queue: VecDeque<String>,
    pub scan_rx: Option<Receiver<ScanEvent>>,
    pub meta_rx: Option<Receiver<MetaEvent>>,
    pub scan_in_flight: bool,
    pub meta_in_flight: bool,
    pub last_error: Option<String>,
    /// 上次进度落盘时间（unix 秒）。
    pub last_save_unix: i64,
    /// 上次扫描的目录，用于「下载目录变了才重扫」。
    pub scanned_dir: Option<PathBuf>,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self::new(LibraryStore::default())
    }
}

impl LibraryState {
    /// 用已加载的 store 构造。
    #[must_use]
    pub fn new(store: LibraryStore) -> Self {
        Self {
            store,
            books_version: 0,
            query: ShelfQuery::default(),
            meta_queue: VecDeque::new(),
            scan_rx: None,
            meta_rx: None,
            scan_in_flight: false,
            meta_in_flight: false,
            last_error: None,
            last_save_unix: now_unix(),
            scanned_dir: None,
        }
    }

    /// 书籍集合变化：让 UI 缓存与书架页失效。
    pub const fn bump(&mut self) {
        self.books_version = self.books_version.wrapping_add(1);
    }

    /// 排空扫描通道；返回是否有进展。
    #[must_use]
    pub fn drain_scan(&mut self) -> bool {
        let Some(rx) = self.scan_rx.take() else {
            return false;
        };
        let mut any = false;
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    any = true;
                    self.scan_in_flight = false;
                    match event {
                        Ok(files) => {
                            let meta_ids = self.store.merge_scan(&files);
                            self.meta_queue.extend(meta_ids);
                            self.bump();
                            self.last_error = None;
                        }
                        Err(error) => self.last_error = Some(error.message()),
                    }
                }
                Err(smol::channel::TryRecvError::Empty) => {
                    // 一次扫描只回送一个事件；空通道说明还没回送，接收端放回继续等。
                    if !any {
                        self.scan_rx = Some(rx);
                    }
                    break;
                }
                Err(smol::channel::TryRecvError::Closed) => {
                    self.scan_in_flight = false;
                    break;
                }
            }
        }
        any
    }

    /// 排空元数据通道；返回是否有进展。
    #[must_use]
    pub fn drain_meta(&mut self) -> bool {
        let Some(rx) = self.meta_rx.take() else {
            return false;
        };
        let mut any = false;
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    any = true;
                    self.meta_in_flight = false;
                    match event.outcome {
                        Ok(meta) => self.store.apply_meta(&event.id, meta),
                        Err(error) => {
                            tracing::warn!("提取元数据失败 {}: {error}", event.id);
                        }
                    }
                    self.bump();
                }
                Err(smol::channel::TryRecvError::Empty) => {
                    // 一次元数据任务只回送一个事件；空通道时放回接收端。
                    if !any {
                        self.meta_rx = Some(rx);
                    }
                    break;
                }
                Err(smol::channel::TryRecvError::Closed) => {
                    self.meta_in_flight = false;
                    break;
                }
            }
        }
        any
    }

    /// 入队待提取元数据的书籍。
    pub fn enqueue_meta(&mut self, ids: Vec<String>) {
        self.meta_queue.extend(ids);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn drain_scan_merges_files_and_bumps_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"x").unwrap();
        let (tx, rx) = smol::channel::unbounded::<ScanEvent>();
        let mut state = LibraryState::new(LibraryStore::default());
        state.scan_rx = Some(rx);
        state.scan_in_flight = true;
        smol::block_on(tx.send(Ok(vec![ScannedFile {
            path,
            size_bytes: 1,
            mtime: 1,
        }])))
        .unwrap();
        drop(tx);

        assert!(state.drain_scan());
        assert_eq!(state.store.books().len(), 1);
        assert_eq!(state.books_version, 1);
        assert!(!state.scan_in_flight);
        assert!(state.scan_rx.is_none());
        assert_eq!(state.meta_queue.len(), 1);
    }

    #[test]
    fn drain_scan_records_error() {
        let (tx, rx) = smol::channel::unbounded::<ScanEvent>();
        let mut state = LibraryState::new(LibraryStore::default());
        state.scan_rx = Some(rx);
        smol::block_on(tx.send(Err(crate::error::AppError::internal("boom".into())))).unwrap();
        drop(tx);

        assert!(state.drain_scan());
        assert!(state.last_error.is_some());
    }
}
