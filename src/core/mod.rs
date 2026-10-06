//! 共享业务逻辑 —— 与 GUI 解耦、供 `desktop` 与 `crawler` 复用的核心类型与工具。
//!
//! 只放不依赖任何 GUI 框架的代码：当前主要居民 [`DownloadTask`]（下载任务的运行时表达，
//! 含后台进度接收端 + 取消令牌，调用方直接构造 / 消费同一实例）。
//!
//! 不放这里：GUI 专属的 `AppModel` / `UIEvent` / `list_cache`（留 `desktop/model/`）、
//! 持久化 record、通用引擎（留 `crawler` / `parser` / `db` 等）。
//! 原则：同一概念在多个调用点各写一份时，抽到这里作为共享契约层。

pub mod async_progress;
pub mod bootstrap;
pub mod config_helpers;
pub mod download_task;
pub mod library;
pub mod search;
pub mod sources;
pub mod update;

pub use async_progress::{DrainOutcome, try_drain_all};

pub use download_task::DownloadTask;
