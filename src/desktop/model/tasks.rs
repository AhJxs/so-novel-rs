//! `AppModel` 任务管理方法
//!
//! `delete_task` 走 3 步骤：
//! 1. `delete_task_inner` 决定能不能删 —— 纯函数返回 `DeleteTaskResult` enum；
//! 2. `&mut self.delete_task` 包装 inner + fire-and-forget 落盘（`self.runtime.spawn_blocking(... crate::db::save_with_trim ...)`）；
//! 3. 调用方（`TasksPage::prompt_delete` 的 `on_ok` 闭包）match enum 决定 push 哪条 toast。

use crate::i18n::ts_fmt;

use super::{AppModel, ops};
use crate::core::DownloadTask;

/// `AppModel::delete_task` 的返回值。区分三种互斥结果，供 UI 决定哪条 toast 文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteTaskResult {
    Deleted,
    StillRunning,
    Missing,
}

impl AppModel {
    /// 清掉所有已结束的任务。
    pub fn clear_finished_tasks(&mut self) {
        let before = self.tasks.len();
        ops::clear_finished_tasks(&mut self.tasks);
        let removed = before - self.tasks.len();
        if removed > 0 {
            let path = self.paths.tasks_file.clone();
            let tasks = self.tasks.clone();
            self.runtime.spawn_blocking(move || {
                if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
                    tracing::warn!("保存任务到文件失败: {e:#}");
                }
            });
            self.push_success(ts_fmt(
                "Toasts.clear_tasks_ok",
                &[("n", &removed.to_string())],
            ));
        }
    }

    /// 删除单条任务记录（仅已结束的，运行中跳过）。
    ///
    /// 返回 `DeleteTaskResult`：
    /// - `Deleted`：找到且已结束，内存 `tasks` retain 移除 + 异步落盘。
    /// - `StillRunning`：找到但还在跑，**不删** —— UI 入口已过滤运行中任务，这里兜底 race。
    /// - `Missing`：id 不存在 —— 同上兜底 concurrent delete。
    ///
    /// 落盘失败由 `tracing::warn!` 记日志，不弹 toast —— 与 `clear_finished_tasks` 行为一致。
    pub fn delete_task(&mut self, id: u64) -> DeleteTaskResult {
        let result = delete_task_inner(&mut self.tasks, id);
        if result == DeleteTaskResult::Deleted {
            let path = self.paths.tasks_file.clone();
            let tasks = self.tasks.clone();
            self.runtime.spawn_blocking(move || {
                if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
                    tracing::warn!("保存任务到文件失败: {e:#}");
                }
            });
        }
        result
    }
}

/// 纯函数：`delete_task` 的内存逻辑部分 —— 找 + retain。
///
/// 抽出来是为了让单元测试不必造 `AppModel`（后者要从磁盘读 yaml）。落盘逻辑保留
/// 在 `delete_task` 这层，因为持久化路径是 `&self` 的，不在内层函数可达范围。
fn delete_task_inner(tasks: &mut Vec<DownloadTask>, id: u64) -> DeleteTaskResult {
    let Some(task) = tasks.iter().find(|t| t.id == id) else {
        return DeleteTaskResult::Missing;
    };
    if task.is_running() {
        return DeleteTaskResult::StillRunning;
    }
    tasks.retain(|t| t.id != id);
    DeleteTaskResult::Deleted
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        non_snake_case
    )]

    use super::*;
    use crate::core::DownloadTask;
    use crate::models::SearchResult;
    use std::path::PathBuf;

    /// 构造一个最小 `DownloadTask`，16 个字段全填默认值；测试只关 `id` + `finished`。
    fn dummy_task(id: u64) -> DownloadTask {
        DownloadTask {
            id,
            origin: SearchResult::default(),
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
        }
    }

    #[test]
    fn delete_task_inner_returns_Deleted_for_finished_task() {
        let mut tasks = vec![{
            let mut t = dummy_task(7);
            t.finished = Some(Ok(PathBuf::from("/tmp/x.epub")));
            t
        }];
        let result = delete_task_inner(&mut tasks, 7);
        assert_eq!(result, DeleteTaskResult::Deleted);
        assert!(tasks.is_empty(), "已结束任务应从 Vec 里移除");
    }

    #[test]
    fn delete_task_inner_returns_StillRunning_for_running_task() {
        let mut tasks = vec![dummy_task(7)]; // finished: None → 运行中
        let result = delete_task_inner(&mut tasks, 7);
        assert_eq!(result, DeleteTaskResult::StillRunning);
        assert_eq!(tasks.len(), 1, "运行中任务不应被 retain 删掉");
        assert_eq!(tasks[0].id, 7);
    }

    #[test]
    fn delete_task_inner_returns_Missing_for_unknown_id() {
        let mut tasks = vec![dummy_task(7)];
        let result = delete_task_inner(&mut tasks, 999);
        assert_eq!(result, DeleteTaskResult::Missing);
        assert_eq!(tasks.len(), 1, "Missing 不应触碰 Vec");
    }
}
