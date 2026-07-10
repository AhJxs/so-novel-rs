//! 三端共享逻辑（cli / web / desktop 都可能用到的核心类型与工具）。
//!
//! 这里只放 **三端都用到的** 代码。当前唯一一个居民：
//! - [`DownloadTask`] — 下载任务的运行时表达（含后台进度接收端 + 取消令牌），
//!   桌面与 Web 都直接构造 / 消费同一实例。
//!
//! ## 什么不该放这里
//!
//! - GUI 专属的 `AppModel` / `UIEvent` / `list_cache` —— 留 `desktop/model/`。
//! - 仅某端使用的持久化 record（如 `DownloadTaskRecord`）—— 留 `models/`。
//! - 爬取 / 解析 / 导出 / DB 等通用引擎 —— 留 `crawler` / `parser` / `db` 等。
//!
//! ## 添加原则
//!
//! 当你发现同一段概念在 cli / web / desktop 三处各写了一份（或者 web / desktop
//! 都用到而 cli 的实现又值得复用），就抽到这里作为三端共享的契约层。

pub mod config_helpers;
pub mod download_task;
pub mod search;

pub use download_task::DownloadTask;
