//! 从持久化数据加载所有任务记录 → 转成运行时 `DownloadTask` 列表。
//!
//! 副作用：上次退出时还在跑的任务（`finished.is_none()`）一律标成
//! "应用重启时中断"，并由调用方把改写后的 JSON 写回，避免下次再看到"未结束"状态。

use crate::core::DownloadTask;
use crate::models::FinishedReason;
use crate::utils::time;

/// 从持久化数据加载出的历史任务快照。
pub struct LoadedTasks {
    /// 还原后的运行时任务列表。
    pub tasks: Vec<DownloadTask>,
    /// 已用过的最大任务 ID（没有任务时为 0）。
    pub max_id: u64,
}

/// 从 JSON 字节还原任务；第二个返回值是「有任务被标成 AppRestarted，需回写的 JSON」。
///
/// `None` 表示没有数据或没有任务被中断，不需要回写。
pub fn load_tasks_from_bytes(bytes: Option<&[u8]>) -> (LoadedTasks, Option<Vec<u8>>) {
    let records = bytes.map(crate::db::parse_tasks).unwrap_or_default();
    let now = time::now_unix_secs();
    let mut max_id: u64 = 0;
    let mut tasks = Vec::with_capacity(records.len());
    let mut need_rewrite = false;

    for rec in records {
        max_id = max_id.max(rec.id);
        let mut task = DownloadTask::from_record(rec);
        if task.finished.is_none() {
            // 加载自持久化数据时 rx/cancel 都是 None —— 没有活动后台任务。
            // 之前跑着的任务实际是应用退出时被中断的，标 AppRestarted 让 UI 正确归类。
            task.finished = Some(Err(FinishedReason::AppRestarted));
            task.finished_at_unix = Some(now);
            need_rewrite = true;
        }
        tasks.push(task);
    }

    let rewrite = if need_rewrite {
        match crate::db::tasks_json_with_trim(&tasks) {
            Ok(json) => Some(json.into_bytes()),
            Err(error) => {
                tracing::warn!("rewrite interrupted tasks failed: {error}");
                None
            }
        }
    } else {
        None
    };

    (LoadedTasks { tasks, max_id }, rewrite)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::models::{DownloadTaskRecord, SearchResult};

    fn record(id: u64, finished: bool) -> DownloadTaskRecord {
        DownloadTaskRecord {
            id,
            origin: SearchResult::default(),
            started_at_unix: 1,
            finished_at_unix: finished.then_some(2),
            book_meta: None,
            total_chapters: 10,
            completed: if finished { 10 } else { 0 },
            failed: 0,
            last_chapter_title: String::new(),
            finished: finished.then_some(Ok(std::path::PathBuf::from("/tmp/a.epub"))),
            failures: Vec::new(),
        }
    }

    #[test]
    fn restarted_task_is_marked_and_rewritten() {
        let bytes = serde_json::to_vec(&vec![record(1, false), record(2, true)]).unwrap();

        let (loaded, rewrite) = load_tasks_from_bytes(Some(&bytes));

        assert_eq!(loaded.tasks.len(), 2);
        assert_eq!(loaded.max_id, 2);
        assert!(
            matches!(
                loaded.tasks[0].finished,
                Some(Err(FinishedReason::AppRestarted))
            ),
            "未完成任务必须标成应用重启中断"
        );
        assert!(loaded.tasks[1].finished.is_some());

        let rewrite = rewrite.expect("有中断任务时必须给出回写数据");
        let parsed: Vec<DownloadTaskRecord> = serde_json::from_slice(&rewrite).unwrap();
        assert!(parsed[0].finished.is_some(), "回写里不能还是未完成");
    }

    #[test]
    fn no_data_is_empty_and_no_rewrite() {
        let (loaded, rewrite) = load_tasks_from_bytes(None);
        assert!(loaded.tasks.is_empty());
        assert_eq!(loaded.max_id, 0);
        assert!(rewrite.is_none());
    }
}
