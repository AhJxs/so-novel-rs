//! Tasks 页数据摘要：`TaskSummary`（避开 `DownloadTask` 不可 Clone）+ `TaskFilter` 分类。
//!
//! `DownloadTask` 含 `mpsc::Receiver` / `CancelToken` 不可 Clone，UI 渲染时复制必要字段
//! 为 `TaskSummary`。所有"过滤 / 排序 / 复制字段"的 helper 都集中到这里，让 `mod.rs`
//! 只剩组装逻辑。

use std::path::PathBuf;

use crate::desktop::model::AppModel;
use crate::core::DownloadTask;
use crate::models::{Book, FinishedReason, SearchResult};

/// `DownloadTask` 的轻量 Clone 视图 —— 给 List delegate 用。
#[derive(Clone)]
pub struct TaskSummary {
    pub id: u64,
    /// 全局序号（跨分页连续，0-based，显示时 +1）。render 切片时填入。
    pub index: usize,
    pub origin: SearchResult,
    pub started_at_unix: i64,
    pub book_meta: Option<Book>,
    pub total_chapters: usize,
    pub completed: u32,
    pub failed: u32,
    /// 跟 `DownloadTask::finished` 同型 —— 成功 = Ok(path)；结束原因见 `FinishedReason`。
    pub finished: Option<Result<PathBuf, FinishedReason>>,
    pub failures: Vec<(u32, String, String)>,
    pub cancelling: bool,
}

impl TaskSummary {
    pub const fn is_running(&self) -> bool {
        self.finished.is_none()
    }
    /// 已结束且 reason 是 `Failed`（不是 cancelled 也不是运行中）。
    pub const fn is_failed(&self) -> bool {
        !self.is_cancelled() && !self.is_running() && self.is_finished_with_err_failed()
    }
    pub const fn is_cancelled(&self) -> bool {
        matches!(
            self.finished.as_ref(),
            Some(Err(reason)) if reason.is_cancelled()
        )
    }
    const fn is_finished_with_err_failed(&self) -> bool {
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
}

/// 过滤种类 —— 按下载状态分组，`All` 不限。顺序固定 = 按钮组顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskFilter {
    #[default]
    All,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl TaskFilter {
    /// 全部过滤，顺序与按钮组 index 对齐。
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Running,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
    ];

    /// label 的 i18n key（不含数量后缀）。
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::All => "Tasks.tab.all",
            Self::Running => "Tasks.tab.running",
            Self::Completed => "Tasks.tab.completed",
            Self::Failed => "Tasks.tab.failed",
            Self::Cancelled => "Tasks.tab.cancelled",
        }
    }

    /// 空态 title 的 i18n key。
    pub const fn empty_title_key(self) -> &'static str {
        match self {
            Self::All => "Tasks.empty.all.title",
            Self::Running => "Tasks.empty.running.title",
            Self::Completed => "Tasks.empty.completed.title",
            Self::Failed => "Tasks.empty.failed.title",
            Self::Cancelled => "Tasks.empty.cancelled.title",
        }
    }

    /// 空态 subtitle 的 i18n key。
    pub const fn empty_subtitle_key(self) -> &'static str {
        match self {
            Self::All => "Tasks.empty.all.subtitle",
            Self::Running => "Tasks.empty.running.subtitle",
            Self::Completed => "Tasks.empty.completed.subtitle",
            Self::Failed => "Tasks.empty.failed.subtitle",
            Self::Cancelled => "Tasks.empty.cancelled.subtitle",
        }
    }
}

/// 任务是否属于该过滤。
pub const fn task_matches_filter(t: &DownloadTask, f: TaskFilter) -> bool {
    match f {
        TaskFilter::All => true,
        TaskFilter::Running => t.is_running(),
        TaskFilter::Completed => matches!(t.finished, Some(Ok(_))),
        TaskFilter::Failed => matches!(t.finished.as_ref(), Some(Err(_))) && !task_is_cancelled(t),
        TaskFilter::Cancelled => task_is_cancelled(t),
    }
}

pub const fn task_is_cancelled(t: &DownloadTask) -> bool {
    matches!(
        t.finished.as_ref(),
        Some(Err(reason)) if reason.is_cancelled()
    )
}

/// 统计各状态数量（按钮 label 后缀用，顺序对齐 `TaskFilter::ALL`）。
pub fn count_by_status(model: &AppModel) -> [usize; 5] {
    let mut counts = [0usize; 5];
    for t in &model.tasks {
        counts[0] += 1;
        if t.is_running() {
            counts[1] += 1;
        } else if matches!(t.finished, Some(Ok(_))) {
            counts[2] += 1;
        } else if t.is_failed() {
            counts[3] += 1;
        } else if t.is_cancelled() {
            counts[4] += 1;
        }
    }
    counts
}

/// 按过滤筛 + 排序（运行中在前；同组按时间倒序）。
pub fn filter_and_sort_indices(model: &AppModel, filter: TaskFilter) -> Vec<usize> {
    let mut indices: Vec<usize> = (0..model.tasks.len())
        .filter(|&i| task_matches_filter(&model.tasks[i], filter))
        .collect();
    indices.sort_by(|&a, &b| {
        let ta = &model.tasks[a];
        let tb = &model.tasks[b];
        let key = |t: &DownloadTask| t.finished_at_unix.unwrap_or(t.started_at_unix);
        tb.is_running()
            .cmp(&ta.is_running())
            .then_with(|| key(tb).cmp(&key(ta)))
    });
    indices
}

/// 给定过滤+排序后的 indices，复制成 `TaskSummary` 列表（每条带全局 index）。
pub fn build_summaries(model: &AppModel, indices: &[usize]) -> Vec<TaskSummary> {
    indices
        .iter()
        .enumerate()
        .map(|(index, &i)| {
            let t = &model.tasks[i];
            TaskSummary {
                id: t.id,
                index,
                origin: t.origin.clone(),
                started_at_unix: t.started_at_unix,
                book_meta: t.book_meta.clone(),
                total_chapters: t.total_chapters,
                completed: t.completed,
                failed: t.failed,
                finished: t.finished.clone(),
                failures: t.failures.clone(),
                cancelling: t.cancelling,
            }
        })
        .collect()
}
