//! 下载任务持久化数据结构。
//!
//! 从 `db/tasks.rs` 迁移而来，供 `tasks_store` 和 `app` 模块共用。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{Book, SearchResult};

/// 任务结束原因 —— 替代原先用字符串字面量做语义 tag 的反模式。
///
/// `is_cancelled` / `is_failed` 直接 match enum，不再依赖字符串等值检测。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FinishedReason {
    /// 用户在 UI 点了"取消"，后台响应后通知。
    UserCancelled,
    /// 应用重启 / 进程被杀时正在跑的任务，重新打开 app 时识别为此状态。
    AppRestarted,
    /// 真正失败（网络 / parser / 后台异常）。消息展示给用户，UI 走 i18n。
    Failed { message: String },
}

impl FinishedReason {
    /// 给 UI 用的错误消息文案（仅 `Failed` 有内容）。
    pub const fn user_message(&self) -> Option<&str> {
        match self {
            Self::Failed { message } => Some(message.as_str()),
            Self::UserCancelled | Self::AppRestarted => None,
        }
    }

    /// 用户取消 或 应用重启中断（"非真正的失败"）。
    pub const fn is_cancelled(&self) -> bool {
        matches!(self, Self::UserCancelled | Self::AppRestarted)
    }
}

/// 任务表持久化形态。所有字段对应 `DownloadTask` 里需要落盘的部分；
/// 运行时字段（rx / cancel / `started_at` Instant）不存。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTaskRecord {
    pub id: u64,
    pub origin: SearchResult,
    /// 任务开始 unix 时间戳（秒）。比 `Instant` 多一个语义：可序列化 / 跨重启。
    pub started_at_unix: i64,
    /// 任务结束 unix 时间戳（秒）。None = 还在跑。
    pub finished_at_unix: Option<i64>,
    pub book_meta: Option<Book>,
    pub total_chapters: usize,
    pub completed: u32,
    pub failed: u32,
    pub last_chapter_title: String,
    /// `Some(Ok(path))` = 完成；`Some(Err(reason))` = 失败 / 取消（语义分类见 `FinishedReason`）；
    /// `None` = 还在跑。
    pub finished: Option<Result<PathBuf, FinishedReason>>,
    pub failures: Vec<FailureRecord>,
}

/// 失败章节明细。`DownloadTask` 里原本是 `Vec<(u32, String, String)>` 元组，
/// 这里改成 struct 让 serde 序列化为 `{"index":..., "title":..., "reason":...}`，
/// 人类可读且向后兼容性好（加字段不破坏老数据）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRecord {
    pub index: u32,
    pub title: String,
    pub reason: String,
}

impl From<(u32, String, String)> for FailureRecord {
    fn from((index, title, reason): (u32, String, String)) -> Self {
        Self {
            index,
            title,
            reason,
        }
    }
}

impl From<FailureRecord> for (u32, String, String) {
    fn from(f: FailureRecord) -> Self {
        (f.index, f.title, f.reason)
    }
}
