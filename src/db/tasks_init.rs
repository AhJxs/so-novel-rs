//! 从 JSON 文件加载所有任务记录 → 转成运行时 `DownloadTask` 列表。
//!
//! 副作用：上次退出时还在跑的任务（`finished.is_none()`）一律标成
//! "应用重启时中断"，并立即写回文件，避免下次再看到"未结束"状态。

use std::path::Path;

use crate::core::DownloadTask;
use crate::db::DaoError;
use crate::models::FinishedReason;
use crate::utils::time;

/// 从 `tasks.json` 加载出的历史任务快照。
pub struct LoadedTasks {
    /// 还原后的运行时任务列表。
    pub tasks: Vec<DownloadTask>,
    /// 已用过的最大任务 ID（没有任务时为 0）。
    pub max_id: u64,
}

/// 读取历史任务并还原成运行时任务；同时返回已用过的最大 ID，供新任务续号。
pub fn load_tasks_from_file(path: &Path) -> Result<LoadedTasks, DaoError> {
    let records = crate::db::load_tasks(path);
    let now = time::now_unix_secs();
    let mut max_id: u64 = 0;
    let mut tasks = Vec::with_capacity(records.len());
    let mut need_rewrite = false;

    for rec in records {
        max_id = max_id.max(rec.id);
        let mut task = DownloadTask::from_record(rec);
        if task.finished.is_none() {
            // 加载自文件时 rx/cancel 都是 None —— 没有活动后台任务。
            // 之前跑着的任务实际是应用退出时被中断的，标 AppRestarted 让 UI 正确归类。
            task.finished = Some(Err(FinishedReason::AppRestarted));
            task.finished_at_unix = Some(now);
            need_rewrite = true;
        }
        tasks.push(task);
    }

    // 如果有中断的任务，重新保存到文件
    if need_rewrite && let Err(e) = crate::db::save_tasks(path, &tasks) {
        tracing::warn!("rewrite interrupted tasks failed: {e}");
    }

    Ok(LoadedTasks { tasks, max_id })
}
