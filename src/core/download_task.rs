//! 一个正在跑的下载任务（由搜索页"下载"按钮触发，下载页/任务页消费）。

use std::time::Duration;
use tokio::sync::mpsc;

use crate::crawler::{CancelToken, Progress};
use crate::models::{Book, DownloadTaskRecord, FailureRecord, FinishedReason, SearchResult};
use crate::utils::time::now_unix_secs;

// `FinishedReason` 定义在 `db::tasks`（JSON schema 跟着持久化层走），业务层直接 use。

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
            // version 不进 Clone —— 调用方拿到的本就是同一逻辑任务，无需 +1。
            version: 0,
        }
    }
}

pub struct DownloadTask {
    /// 任务唯一 id（递增）。
    pub id: u64,
    /// 触发时拿到的搜索结果（source / `book_url` / 书名作者等）。
    pub origin: SearchResult,
    /// 后台推送进度的接收端；每帧 `try_recv` 排空。
    /// 加载自 `SQLite` 时为 None（已中断的旧任务没有活通道）。
    pub rx: Option<mpsc::UnboundedReceiver<Progress>>,
    /// 后台任务的取消令牌。加载自 `SQLite` 时为 None。
    pub cancel: Option<CancelToken>,

    /// 用户点了"取消"但后台还没响应的中间态（true 时 UI 显示"正在取消..."、按钮置灰）。
    /// drain 收到 `Progress::Cancelled` 时清零；运行时字段，不持久化。
    pub cancelling: bool,

    /// 任务开始的 unix 时间戳（秒）。比 `Instant` 多两个能力：可序列化 / 跨重启。
    pub started_at_unix: i64,
    /// 任务结束的 unix 时间戳（秒）；None = 还没结束。给 UI 算"耗时"。
    pub finished_at_unix: Option<i64>,

    pub book_meta: Option<Book>,
    pub total_chapters: usize,
    pub completed: u32,
    pub failed: u32,
    pub last_chapter_title: String,
    /// `Some(Ok(path))` 完成；`Some(Err(reason))` 失败 / 取消；`None` 还在跑。
    pub finished: Option<Result<std::path::PathBuf, FinishedReason>>,
    /// 失败章节明细（用于任务页详情显示）。持久化时通过 `FailureRecord` 转换。
    pub failures: Vec<(u32, String, String)>,

    /// 任务自身状态变更版本号：每帧 `drain` 收到事件 +1，UI 配合 `AppModel::tasks_version`
    /// 作为列表 cache 的 key 一部分。**不**进 Clone（Clone 出来的是逻辑上同一任务）。
    pub version: u64,
}

impl DownloadTask {
    /// 把单条进度事件应用到任务字段。
    pub fn apply_progress(&mut self, ev: Progress) {
        match ev {
            Progress::BookResolved {
                book,
                total_chapters,
            } => {
                self.book_meta = Some(*book);
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

    /// 排空进度通道；返回是否产生过事件（用于触发 repaint）。
    ///
    /// 用 `mem::take` 把 `rx` 从 `self` 移出, 循环里只 `&mut self` 写字段, 因此可以直接调 `self.apply_progress(ev)`。
    /// 契约: `rx = None` → false 且不动字段; `Empty` → 放回 rx（关键不变量）; `Disconnected` → 标 `AppRestarted`
    /// （若 `finished` 未设）; 一旦结束补填 `finished_at_unix`, 收到任何事件都 `version.wrapping_add(1)` 并返回 true。
    pub fn drain(&mut self) -> bool {
        let mut any = false;
        let was_running = self.is_running();
        let Some(mut rx) = self.rx.take() else {
            return false;
        };
        loop {
            match rx.try_recv() {
                Ok(ev) => {
                    any = true;
                    self.apply_progress(ev);
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    // 关键：把 rx 放回去 —— 下次 drain 还能读到新事件。
                    self.rx = Some(rx);
                    break;
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // 后台 task 异常退出（panic / 进程被 kill）：用户没主动取消，理论上应自动恢复重跑。
                    // 当前简化处理：直接标记结束。
                    if self.finished.is_none() {
                        self.finished = Some(Err(FinishedReason::AppRestarted));
                        self.cancelling = false;
                    }
                    // Disconnected 路径不放回 rx —— sender 已 drop，留 None 即可。
                    break;
                }
            }
        }
        if was_running && self.finished.is_some() && self.finished_at_unix.is_none() {
            self.finished_at_unix = Some(now_unix_secs());
        }
        if any {
            self.version = self.version.wrapping_add(1);
        }
        any
    }

    pub const fn is_running(&self) -> bool {
        self.finished.is_none()
    }

    /// 用户主动取消 → cancelled。
    pub const fn is_cancelled(&self) -> bool {
        matches!(
            self.finished.as_ref(),
            Some(Err(
                FinishedReason::UserCancelled | FinishedReason::AppRestarted
            ))
        )
    }

    /// 已结束且不是取消 → 失败。
    pub const fn is_failed(&self) -> bool {
        matches!(
            self.finished.as_ref(),
            Some(Err(FinishedReason::Failed { .. }))
        )
    }

    pub fn book_name(&self) -> &str {
        self.book_meta
            .as_ref()
            .map_or(self.origin.book_name.as_str(), |b| b.book_name.as_str())
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
            failures: rec
                .failures
                .into_iter()
                .map(|f| (f.index, f.title, f.reason))
                .collect(),
            version: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn task_with_rx() -> (DownloadTask, mpsc::UnboundedSender<Progress>) {
        let (tx, rx) = mpsc::unbounded_channel::<Progress>();
        let task = DownloadTask {
            id: 1,
            origin: search_result_dummy(),
            rx: Some(rx),
            cancel: None,
            cancelling: false,
            started_at_unix: 0,
            finished_at_unix: None,
            book_meta: None,
            total_chapters: 0,
            completed: 0,
            failed: 0,
            last_chapter_title: String::new(),
            finished: None,
            failures: Vec::new(),
            version: 0,
        };
        (task, tx)
    }

    fn search_result_dummy() -> crate::models::SearchResult {
        crate::models::SearchResult::default()
    }

    #[test]
    fn drain_returns_false_and_leaves_state_untouched_when_rx_is_none() {
        let mut task = DownloadTask {
            id: 1,
            origin: search_result_dummy(),
            rx: None,
            cancel: None,
            cancelling: false,
            started_at_unix: 0,
            finished_at_unix: None,
            book_meta: None,
            total_chapters: 0,
            completed: 0,
            failed: 0,
            last_chapter_title: String::new(),
            finished: None,
            failures: Vec::new(),
            version: 0,
        };
        assert!(!task.drain(), "rx=None 应返回 false");
        assert_eq!(task.version, 0, "rx=None 不应 bump version");
        assert!(task.finished.is_none(), "rx=None 不应改 finished");
    }

    #[test]
    fn drain_empty_branch_puts_rx_back_so_next_drain_works() {
        // 关键不变量：排空后必须把 rx 放回，否则下一次 drain 再也读不到事件
        //（`mem::take` 重写最容易漏的一步）。
        let (mut task, tx) = task_with_rx();

        let any = task.drain();
        assert!(!any, "空通道应返回 false");
        assert!(task.rx.is_some(), "Empty 分支必须把 rx 放回去");

        tx.send(Progress::Cancelled).expect("send");
        let any = task.drain();
        assert!(any, "第二次 drain 必须能读到事件");
        assert!(matches!(
            task.finished,
            Some(Err(FinishedReason::UserCancelled))
        ));
    }

    #[test]
    fn drain_disconnected_does_not_put_rx_back() {
        // sender drop 后放回 rx 没有意义（永远读不出事件），所以 drain 后 rx 应为 None。
        let (mut task, tx) = task_with_rx();
        drop(tx);
        let any = task.drain();
        assert!(!any, "disconnected 应返回 false（没读到事件）");
        assert!(task.rx.is_none(), "Disconnected 后 rx 应为 None");
        assert!(matches!(
            task.finished,
            Some(Err(FinishedReason::AppRestarted))
        ));
    }

    #[test]
    fn drain_disconnected_does_not_overwrite_existing_finished() {
        // 守卫：finished 已设（用户取消 / Finished 已收到）时 Disconnected 不应覆盖。
        let (mut task, tx) = task_with_rx();
        task.finished = Some(Err(FinishedReason::UserCancelled));
        drop(tx);
        task.drain();
        assert!(
            matches!(task.finished, Some(Err(FinishedReason::UserCancelled))),
            "Disconnected 不应覆盖已存在的 finished"
        );
    }

    #[test]
    fn drain_uses_apply_progress_for_chapter_done() {
        let (mut task, tx) = task_with_rx();
        tx.send(Progress::ChapterDone {
            index: 5,
            title: "第五章".into(),
        })
        .expect("send");
        let any = task.drain();
        assert!(any);
        assert_eq!(task.completed, 1, "ChapterDone → completed += 1");
        assert_eq!(task.last_chapter_title, "第五章");
        assert_eq!(task.version, 1, "收到事件 → version += 1");
    }

    #[test]
    fn drain_records_finished_at_unix_when_finished_via_event() {
        let (mut task, tx) = task_with_rx();
        let out = std::path::PathBuf::from("/tmp/out.epub");
        tx.send(Progress::Finished {
            output_path: out.clone(),
        })
        .expect("send");
        task.drain();
        assert!(matches!(task.finished, Some(Ok(ref p)) if p == &out));
        assert!(
            task.finished_at_unix.is_some(),
            "Finished 事件应触发 finished_at_unix 兜底"
        );
    }
}
