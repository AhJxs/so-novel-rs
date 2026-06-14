//! 从 DB 加载所有任务记录 → 转成运行时 `DownloadTask` 列表。
//!
//! 副作用：上次退出时还在跑的任务（`finished.is_none()`）一律标成
//! "应用重启时中断"，并立即写回 DB，避免下次再看到"未结束"状态。

use crate::db::{Db, DownloadTaskRecord};

use super::download_task::DownloadTask;
use super::now::now_unix_secs;

pub fn load_tasks_from_db(db: &Db) -> (Vec<DownloadTask>, u64) {
    let records = match crate::db::tasks::list(db.conn()) {
        Ok(rs) => rs,
        Err(e) => {
            tracing::warn!("load tasks from db failed: {e}");
            return (Vec::new(), 1);
        }
    };
    let now = now_unix_secs();
    let mut max_id: u64 = 0;
    let mut tasks = Vec::with_capacity(records.len());
    let mut need_rewrite: Vec<DownloadTaskRecord> = Vec::new();
    for rec in records {
        max_id = max_id.max(rec.id);
        let mut task = DownloadTask::from_record(rec.clone());
        if task.finished.is_none() {
            task.finished = Some(Err("应用重启时中断".to_string()));
            task.finished_at_unix = Some(now);
            need_rewrite.push(task.to_record());
        }
        tasks.push(task);
    }
    for r in &need_rewrite {
        if let Err(e) = crate::db::tasks::upsert(db.conn(), r) {
            tracing::warn!("rewrite interrupted task {} failed: {e}", r.id);
        }
    }
    (tasks, max_id + 1)
}
