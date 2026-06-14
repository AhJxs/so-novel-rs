//! 下载任务相关业务方法。

use tokio::sync::mpsc;

use crate::crawler::{CancelToken, Progress};
use crate::models::SearchResult;

use super::super::download_task::DownloadTask;
use super::super::now::now_unix_secs;

/// 派一个新的下载任务到后台。返回新任务的 id。
pub fn spawn_download(
    rules: &[crate::models::Rule],
    config: &crate::config::AppConfig,
    runtime: &tokio::runtime::Runtime,
    next_task_id: &mut u64,
    target: SearchResult,
) -> (u64, DownloadTask) {
    let id = *next_task_id;
    *next_task_id += 1;
    let (tx, rx) = mpsc::unbounded_channel::<Progress>();
    let cancel = CancelToken::new();

    let rule = rules
        .iter()
        .find(|r| r.id == target.source_id)
        .cloned();
    let cfg = config.clone();
    let book_url = target.url.clone();
    let cancel_for_task = cancel.clone();
    let tx_for_task = tx.clone();

    runtime.spawn(async move {
        let Some(rule) = rule else {
            let _ = tx_for_task.send(Progress::Cancelled);
            return;
        };
        let source = crate::rules::Source::from(rule, &cfg);
        let opts = crate::crawler::DownloadOptions {
            progress: tx_for_task,
            cancel: cancel_for_task,
        };
        if let Err(e) = crate::crawler::download_book(&cfg, &source, &book_url, opts).await {
            tracing::warn!("download_book failed: {e}");
        }
    });

    drop(tx);

    let task = DownloadTask {
        id,
        origin: target,
        rx: Some(rx),
        cancel: Some(cancel),
        cancelling: false,
        started_at_unix: now_unix_secs(),
        finished_at_unix: None,
        book_meta: None,
        total_chapters: 0,
        completed: 0,
        failed: 0,
        last_chapter_title: String::new(),
        finished: None,
        failures: Vec::new(),
    };
    (id, task)
}

/// 清掉所有已结束的任务（完成 / 失败 / 取消）。运行中的任务保留。
pub fn clear_finished_tasks(
    tasks: &mut Vec<DownloadTask>,
    db: &crate::db::Db,
) {
    let before = tasks.len();
    tasks.retain(|t| t.is_running());
    let removed = before - tasks.len();
    if let Err(e) = crate::db::tasks::delete_finished(db.conn()) {
        tracing::warn!("clear_finished_tasks db delete failed: {e}");
    } else if removed > 0 {
        // toast 由调用方在能访问 self.toast 的地方处理
        tracing::info!("已清除 {removed} 条任务");
    }
}

/// 把单条任务 upsert 到 DB。
pub fn save_task_to_db(db: &crate::db::Db, task: &DownloadTask) {
    let rec = task.to_record();
    if let Err(e) = crate::db::tasks::upsert(db.conn(), &rec) {
        tracing::warn!("save_task_to_db failed for id={}: {e}", task.id);
    }
}
