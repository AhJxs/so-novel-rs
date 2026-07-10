//! 从 JSON 文件加载所有任务记录 → 转成运行时 `DownloadTask` 列表。
//!
//! 副作用：上次退出时还在跑的任务（`finished.is_none()`）一律标成
//! "应用重启时中断"，并立即写回文件，避免下次再看到"未结束"状态。

use std::path::Path;

use crate::core::DownloadTask;
use crate::models::FinishedReason;
use crate::utils::time::now_unix_secs;

pub fn load_tasks_from_file(path: &Path) -> (Vec<DownloadTask>, u64) {
    let records = crate::db::load_tasks(path);
    let now = now_unix_secs();
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
    if need_rewrite {
        if let Err(e) = crate::db::save_tasks(path, &tasks) {
            tracing::warn!("rewrite interrupted tasks failed: {e}");
        }
    }

    (tasks, max_id + 1)
}
