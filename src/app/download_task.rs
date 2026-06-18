//! 一个正在跑的下载任务（由搜索页"下载"按钮触发，下载页/任务页消费）。

use std::time::Duration;
use tokio::sync::mpsc;

use crate::crawler::{CancelToken, Progress};
use crate::db::tasks::{DownloadTaskRecord, FailureRecord, FinishedReason};
use crate::models::{Book, SearchResult};

use super::now::now_unix_secs;

// `FinishedReason` 定义在 `db::tasks`（持久化层 —— JSON schema 跟着 db 走）。
// 直接 use 即可，业务层复用同一枚举类型。

/// 手动 Clone 跳过 `rx`/`cancel`（不可 Clone 的后台通道）。
impl Clone for DownloadTask {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            origin: self.origin.clone(),
            rx: None,
            cancel: None,
            cancelling: self.cancelling,
            started_at_unix: self.started_at_unix,
            finished_at_unix: self.finished_at_unix,
            book_meta: self.book_meta.clone(),
            total_chapters: self.total_chapters,
            completed: self.completed,
            failed: self.failed,
            last_chapter_title: self.last_chapter_title.clone(),
            finished: self.finished.clone(),
            failures: self.failures.clone(),
        }
    }
}

pub struct DownloadTask {
    /// 任务唯一 id（递增）。
    pub id: u64,
    /// 触发时拿到的搜索结果，包含 source / book_url / 书名作者等信息。
    pub origin: SearchResult,
    /// 后台推送进度的接收端；每帧 `try_recv` 排空。
    /// 加载自 SQLite 时为 None（已中断的旧任务不会有活通道）。
    pub rx: Option<mpsc::UnboundedReceiver<Progress>>,
    /// 后台任务的取消令牌。加载自 SQLite 时为 None。
    pub cancel: Option<CancelToken>,

    /// 用户点了"取消"但后台还没响应的中间态。true 时 UI 显示"正在取消..."，
    /// 按钮置灰避免重复点；drain 收到 `Progress::Cancelled` 时清零。
    /// 运行时字段，不持久化。
    pub cancelling: bool,

    // ---- 时间戳 ----
    /// 任务开始的 unix 时间戳（秒）。比 `Instant` 多两个能力：可序列化 / 跨重启。
    pub started_at_unix: i64,
    /// 任务结束的 unix 时间戳（秒）；None = 还没结束。给 UI 算"耗时"。
    pub finished_at_unix: Option<i64>,

    // ---- 累计状态（每帧 try_recv 时更新） ----
    pub book_meta: Option<Book>,
    pub total_chapters: usize,
    pub completed: u32,
    pub failed: u32,
    pub last_chapter_title: String,
    /// `Some(Ok(path))` 完成；`Some(Err(reason))` 失败 / 取消（语义分类见 `FinishedReason`）。
    /// `None` 还在跑。
    pub finished: Option<Result<std::path::PathBuf, FinishedReason>>,
    /// 失败章节明细（用于任务页详情显示）。持久化时通过 `FailureRecord` 转换。
    pub failures: Vec<(u32, String, String)>,
}

impl DownloadTask {
    /// 排空进度通道；返回是否产生过事件（用于触发 repaint）。
    pub fn drain(&mut self) -> bool {
        let mut any = false;
        let was_running = self.is_running();
        let Some(rx) = self.rx.as_mut() else {
            return false;
        };
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    any = true;
                    match ev {
                        Progress::BookResolved {
                            book,
                            total_chapters,
                        } => {
                            self.book_meta = Some(book);
                            self.total_chapters = total_chapters;
                        }
                        Progress::ChapterDone { index, title } => {
                            self.completed += 1;
                            self.last_chapter_title = title;
                            let _ = index;
                        }
                        Progress::ChapterFailed {
                            index,
                            title,
                            reason,
                        } => {
                            self.failed += 1;
                            self.failures.push((index, title, reason));
                        }
                        Progress::Finished { output_path } => {
                            self.finished = Some(Ok(output_path));
                        }
                        Progress::Cancelled => {
                            self.finished = Some(Err(FinishedReason::UserCancelled));
                            self.cancelling = false;
                        }
                        Progress::Failed { reason } => {
                            self.finished = Some(Err(FinishedReason::Failed { message: reason }));
                            self.cancelling = false;
                        }
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // 后台 task 异常退出（panic / 进程被 kill 等）—— 标记为 AppRestarted，
                    // 因为用户没主动取消，理论上应该自动恢复重跑。当前简化处理：直接标记结束。
                    if self.finished.is_none() {
                        self.finished = Some(Err(FinishedReason::AppRestarted));
                        self.cancelling = false;
                    }
                    break;
                }
            }
        }
        if was_running && self.finished.is_some() && self.finished_at_unix.is_none() {
            self.finished_at_unix = Some(now_unix_secs());
        }
        any
    }

    pub fn is_running(&self) -> bool {
        self.finished.is_none()
    }

    /// 用户主动取消 → cancelled。
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self.finished.as_ref(),
            Some(Err(FinishedReason::UserCancelled | FinishedReason::AppRestarted))
        )
    }

    /// 已结束且不是取消 → 失败。
    pub fn is_failed(&self) -> bool {
        matches!(self.finished.as_ref(), Some(Err(FinishedReason::Failed { .. })))
    }

    pub fn book_name(&self) -> &str {
        self.book_meta
            .as_ref()
            .map(|b| b.book_name.as_str())
            .unwrap_or(self.origin.book_name.as_str())
    }

    /// 距开始的实时耗时。
    pub fn elapsed(&self) -> Option<Duration> {
        let started = self.started_at_unix.max(0) as u64;
        if self.is_running() {
            let now = now_unix_secs().max(0) as u64;
            Some(Duration::from_secs(now.saturating_sub(started)))
        } else {
            self.finished_at_unix.map(|end| {
                let end_u = end.max(0) as u64;
                Duration::from_secs(end_u.saturating_sub(started))
            })
        }
    }

    /// 转成可持久化的 record（不含 rx/cancel）。
    pub fn to_record(&self) -> DownloadTaskRecord {
        DownloadTaskRecord {
            id: self.id,
            origin: self.origin.clone(),
            started_at_unix: self.started_at_unix,
            finished_at_unix: self.finished_at_unix,
            book_meta: self.book_meta.clone(),
            total_chapters: self.total_chapters,
            completed: self.completed,
            failed: self.failed,
            last_chapter_title: self.last_chapter_title.clone(),
            finished: self.finished.clone(),
            failures: self
                .failures
                .iter()
                .map(|(i, t, r)| FailureRecord {
                    index: *i,
                    title: t.clone(),
                    reason: r.clone(),
                })
                .collect(),
        }
    }

    /// 从 record 重建。`rx` 和 `cancel` 留 None。
    pub fn from_record(rec: DownloadTaskRecord) -> Self {
        Self {
            id: rec.id,
            origin: rec.origin,
            rx: None,
            cancel: None,
            cancelling: false,
            started_at_unix: rec.started_at_unix,
            finished_at_unix: rec.finished_at_unix,
            book_meta: rec.book_meta,
            total_chapters: rec.total_chapters,
            completed: rec.completed,
            failed: rec.failed,
            last_chapter_title: rec.last_chapter_title,
            finished: rec.finished,
            failures: rec.failures.into_iter().map(|f| (f.index, f.title, f.reason)).collect(),
        }
    }
}
