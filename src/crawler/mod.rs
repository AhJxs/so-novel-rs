//! 下载调度层  对应 Java `core.Crawler` + `parse.ChapterParser` 的重试逻辑 + `handle.CrawlerPostHandler` 的导出+清理逻辑。
//!
//! 入口: `download_book(cfg, source, book_url, opts) -> Result<PathBuf, CrawlerError>`:
//! `resolve_book` (阶段一: 详情 + 目录) → `download_chapters` (阶段二: 并发抓取 + 导出)。
//! 子模块: [`progress`] / [`download_options`] / [`resolve`] / [`download`] / [`search`] / [`cover_updater`] / [`health`] / `retry`(私有)。
//!
//! 两处坑: `Progress` 经 `mpsc::UnboundedSender` 推给 UI, 由 `events::drain` 排空; `CancelToken`
//! 在每章入口检查, 正在跑的章节会跑完才退出 (任务级取消, 非连接级中断)。

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
