//! 下载调度层  对应 Java `core.Crawler` +
//! `parse.ChapterParser` 的重试逻辑 + `handle.CrawlerPostHandler` 的导出+清理逻辑。
//!
//! 入口: `download_book(cfg, source, book_url, opts) -> Result<PathBuf, CrawlerError>`.
//!
//! # 子模块 (按职责拆分)
//!
//! - [`progress`] — `Progress` 枚举, 调度层 → UI 的消息协议
//! - [`download_options`] — `DownloadOptions` (progress/cancel/notify) + `CancelToken`
//! - [`resolve`] — `CrawlerError` + `resolve_book` (阶段一: 详情 + 目录)
//! - [`download`] — `download_book` + `download_chapters` (阶段二: 并发抓取 + 导出)
//! - [`search`] / [`cover_updater`] / [`health`] — 搜索 / 封面替换 / 连通性
//! - `retry` — 指数退避 (私有)
//!
//! # 整体流程 (ASCII)
//!
//! ```text
//!  download_book(cfg, source, book_url, opts)
//!      │
//!      ├─► resolve_book     ── parse_book_detail ──►  BookError
//!      │      │                                       (CFG/HTTP/Parse/CF)
//!      │      ▼
//!      │   Book { url, name, author, ... }
//!      │
//!      ├─► resolve_toc      ── parse_toc ──────────►  TocError
//!      │      │
//!      │      ▼
//!      │   Vec<ChapterMeta>  (URL + title, no body)
//!      │
//!      ├─► download_chapters  (in `download` module)
//!      │      ├─ spawn parallel (Semaphore: cfg.crawl.concurrency)
//!      │      ├─ each chapter: parse_chapter + retry + per-task write
//!      │      ├─ if EPUB: download cover bytes (soft-skip)
//!      │      ├─ Exporter::merge_with_cover
//!      │      └─ if !preserve_chapter_cache: rm chapters_dir
//! ```
//!
//! # 进度 / 取消
//!
//! - **进度**: 通过 `mpsc::UnboundedSender<Progress>` 推送事件给 UI; `events::drain` 排空.
//! - **取消**: `CancelToken` (Arc<AtomicBool>); 在每章入口检查; 正在跑的章节会跑完才退出
//!   (非连接级中断; 用 `CancelToken` 轮询实现任务级取消).

pub mod cover_updater;
pub mod download;
pub mod download_options;
pub mod health;
mod progress;
pub mod resolve;
mod retry;
pub mod search;

// Re-exports (业务层只跟 mod.rs 的 pub 交互, 不直接 import 子模块)
pub use download::{download_book, download_chapters};
pub use download_options::{CancelToken, DownloadOptions};
pub use progress::Progress;
pub use resolve::{CrawlerError, resolve_book};
