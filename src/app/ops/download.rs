//! 下载任务相关业务方法。

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::app::search_state::TocState;
use crate::config::AppConfig;
use crate::crawler::{
    CancelToken, CrawlerError, DownloadOptions, Progress, download_book, download_chapters,
    resolve_book,
};
use crate::db::Db;
use crate::http::HttpClients;
use crate::models::{Book, Chapter, Rule, SearchResult};
use crate::rules::Source;

use super::super::download_task::DownloadTask;
use super::super::now::now_unix_secs;
use super::super::search_state::TocEvent;

/// 派一个 TOC 预取任务（获取元数据 + 章节列表，不开始下载）。
/// 返回接收端，调用方存入 `search.toc_rx`。
pub fn spawn_resolve_toc(
    rules: &[Rule],
    config: &AppConfig,
    http: Arc<HttpClients>,
    runtime: &tokio::runtime::Runtime,
    target: &SearchResult,
) -> mpsc::UnboundedReceiver<TocEvent> {
    let (tx, rx) = mpsc::unbounded_channel::<TocEvent>();

    let rule = rules.iter().find(|r| r.id == target.source_id).cloned();
    let cfg = config.clone();
    let book_url = target.url.clone();
    let source_id = target.source_id;
    let url_for_event = target.url.clone();

    tracing::info!(source_id = source_id, book_url = %book_url, "TOC 预取派发");

    runtime.spawn(async move {
        let started = std::time::Instant::now();
        let state = if let Some(rule) = rule {
            let source = Source::from(rule, &cfg);
            let cancel = CancelToken::new();
            let client = http.for_rule(&source.rule);
            match resolve_book(&cfg, client, &source, &book_url, &cancel).await {
                Ok((book, chapters)) => {
                    tracing::info!(source_id = source_id, book = %book.book_name, chapters = chapters.len(), elapsed_ms = started.elapsed().as_millis() as u64, "TOC 预取成功");
                    TocState::Loaded(Box::new(book), chapters)
                }
                Err(e) => {
                    tracing::warn!(source_id = source_id, book_url = %book_url, elapsed_ms = started.elapsed().as_millis() as u64, error = %format!("{e:#}"), "TOC 预取失败");
                    TocState::Failed(format!("{e:#}"))
                }
            }
        } else {
            tracing::warn!(source_id = source_id, "TOC 预取失败: 书源未找到（可能已被删除）");
            TocState::Failed("书源未找到".to_string())
        };
        let _ = tx.send(TocEvent {
            source_id,
            url: url_for_event,
            state,
        });
    });

    rx
}

/// 派一个指定章节范围的下载任务。跳过 resolve 阶段，直接进入下载。
/// `chapters` 已由调用方按用户选择过滤过范围。
// 参数刚好 8 个：rules/config/http/runtime/next_task_id 是共享的"spawn 上下文"，
// target/book/chapters 是任务数据。Phase 3.1 加了 `http: Arc<HttpClients>`
// 后触发了 too_many_arguments（阈值 7）。理论上的解法是把前 5 个字段塞进一个
// `OpsCtx` 结构（类似其他大型项目的 `AppContext`），但那是一个跨 3 个 spawn
// 函数 + 上层 AppModel::spawn_* 的小重构，超出 Phase 3.1 范围。本次保持参数
// 表面稳定，留待后续 Phase 集中重构时再做。
#[allow(clippy::too_many_arguments)]
pub fn spawn_download_range(
    rules: &[Rule],
    config: &AppConfig,
    http: Arc<HttpClients>,
    runtime: &tokio::runtime::Runtime,
    next_task_id: &mut u64,
    target: SearchResult,
    book: Book,
    chapters: Vec<Chapter>,
) -> (u64, DownloadTask) {
    let id = *next_task_id;
    *next_task_id += 1;
    let (tx, rx) = mpsc::unbounded_channel::<Progress>();
    let cancel = CancelToken::new();

    let rule = rules.iter().find(|r| r.id == target.source_id).cloned();
    let cfg = config.clone();
    let book_url = target.url.clone();
    let cancel_for_task = cancel.clone();
    let tx_for_task = tx.clone();

    let total = chapters.len();
    let book_for_meta = book.clone();
    let _ = tx_for_task.send(Progress::BookResolved {
        book: book.clone(),
        total_chapters: total,
    });

    tracing::info!(task_id = id, source_id = target.source_id, book = %book.book_name, total_chapters = total, book_url = %book_url, "下载任务派发（指定范围）");

    runtime.spawn(async move {
        let Some(rule) = rule else {
            let _ = tx_for_task.send(Progress::Cancelled);
            return;
        };
        let source = Source::from(rule, &cfg);
        let client = http.for_rule(&source.rule);
        // 留一个 sender 副本用于失败时发 Progress::Failed（tx_for_task 会 move 进 opts）。
        let tx_for_failure = tx_for_task.clone();
        let opts = DownloadOptions {
            progress: tx_for_task,
            cancel: cancel_for_task,
        };
        if let Err(e) =
            download_chapters(&cfg, client, &source, &book_url, &book, chapters, opts).await
        {
            // 用户取消已由 crawler 内部发 Progress::Cancelled；真正的失败发
            // Progress::Failed，让 UI 区分"取消"与"失败"并保留原因。
            if !matches!(e, CrawlerError::Cancelled) {
                tracing::warn!("download_chapters failed: {e:#}");
                let _ = tx_for_failure.send(Progress::Failed {
                    reason: format!("{e:#}"),
                });
            }
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
        book_meta: Some(book_for_meta),
        total_chapters: total,
        completed: 0,
        failed: 0,
        last_chapter_title: String::new(),
        finished: None,
        failures: Vec::new(),
    };
    (id, task)
}

/// 派一个新的下载任务到后台。返回新任务的 id。
pub fn spawn_download(
    rules: &[Rule],
    config: &AppConfig,
    http: Arc<HttpClients>,
    runtime: &tokio::runtime::Runtime,
    next_task_id: &mut u64,
    target: SearchResult,
) -> (u64, DownloadTask) {
    let id = *next_task_id;
    *next_task_id += 1;
    let (tx, rx) = mpsc::unbounded_channel::<Progress>();
    let cancel = CancelToken::new();

    let rule = rules.iter().find(|r| r.id == target.source_id).cloned();
    let cfg = config.clone();
    let book_url = target.url.clone();
    let cancel_for_task = cancel.clone();
    let tx_for_task = tx.clone();

    tracing::info!(task_id = id, source_id = target.source_id, book_name = %target.book_name, book_url = %book_url, "下载任务派发");

    runtime.spawn(async move {
        let Some(rule) = rule else {
            tracing::warn!(task_id = id, "下载任务取消: 书源未找到（可能已被删除）");
            let _ = tx_for_task.send(Progress::Cancelled);
            return;
        };
        let source = Source::from(rule, &cfg);
        let client = http.for_rule(&source.rule);
        // 留一个 sender 副本用于失败时发 Progress::Failed（tx_for_task 会 move 进 opts）。
        let tx_for_failure = tx_for_task.clone();
        let opts = DownloadOptions {
            progress: tx_for_task,
            cancel: cancel_for_task,
        };
        if let Err(e) = download_book(&cfg, client, &source, &book_url, opts).await {
            // 用户取消已由 crawler 内部发 Progress::Cancelled；真正的失败发
            // Progress::Failed，让 UI 区分"取消"与"失败"并保留原因。
            if !matches!(e, CrawlerError::Cancelled) {
                tracing::warn!(task_id = id, "download_book failed: {e:#}");
                let _ = tx_for_failure.send(Progress::Failed {
                    reason: format!("{e:#}"),
                });
            }
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
pub fn clear_finished_tasks(tasks: &mut Vec<DownloadTask>, db: &Db) {
    let before = tasks.len();
    tasks.retain(|t| t.is_running());
    let removed = before - tasks.len();
    if let Err(e) = crate::db::tasks::delete_finished(db.conn()) {
        tracing::warn!("clear_finished_tasks db delete failed: {e}");
    } else if removed > 0 {
        tracing::info!("已清除 {removed} 条任务");
    }
}

/// 把单条任务 upsert 到 DB。
pub fn save_task_to_db(db: &Db, task: &DownloadTask) {
    let rec = task.to_record();
    if let Err(e) = crate::db::tasks::upsert(db.conn(), &rec) {
        tracing::warn!("save_task_to_db failed for id={}: {e}", task.id);
    }
}
