//! 下载任务相关业务方法。

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::Instrument;

use crate::app::search_state::TocState;
use crate::config::AppConfig;
use crate::crawler::{
    CancelToken, CrawlerError, DownloadOptions, Progress, download_book, download_chapters,
    resolve_book,
};
use crate::http::HttpClients;
use crate::models::Source;
use crate::models::{Book, Chapter, Rule, SearchResult};

use super::super::download_task::DownloadTask;
use super::super::events::WakeupHandle;
use super::super::search_state::TocEvent;
use super::super::trace::{TraceId, sub};
use crate::utils::time::now_unix_secs;

/// spawn 共享上下文：提取 `rules` / `config` / `http` / `runtime` 四个参数，
/// 消除 `spawn_download` / `spawn_download_range` / `spawn_resolve_toc` 的重复参数列表。
pub struct OpsCtx<'a> {
    pub rules: &'a [Rule],
    pub config: &'a AppConfig,
    pub http: Arc<HttpClients>,
    pub runtime: &'a tokio::runtime::Runtime,
    /// 唤醒信号 sender：producer 写入 mpsc 后调 `notify()` 让 `drain_loop`
    /// 立即排空，不必等 100ms 兜底。详见 `crate::app::events::WakeupHandle`。
    pub wakeup: &'a WakeupHandle,
}

/// 派一个 TOC 预取任务（获取元数据 + 章节列表，不开始下载）。
/// 返回接收端，调用方存入 `search.toc_rx`。
pub fn spawn_resolve_toc(
    ctx: &OpsCtx<'_>,
    target: &SearchResult,
) -> mpsc::UnboundedReceiver<TocEvent> {
    let (tx, rx) = mpsc::unbounded_channel::<TocEvent>();

    let rule = ctx.rules.iter().find(|r| r.id == target.source_id).cloned();
    let cfg = ctx.config.clone();
    let http = Arc::clone(&ctx.http);
    let book_url = target.url.clone();
    let source_id = target.source_id;

    // 顶层 trace_id —— TOC 预取是一次独立的"动作"，独立 mint。
    let trace_id = TraceId::mint();
    let span = tracing::info_span!(
        sub::TOC,
        trace_id = %trace_id,
        source_id = source_id,
        %book_url,
    );
    let span_for_instrument = span;
    let wakeup = ctx.wakeup.clone();

    ctx.runtime.spawn(
        async move {
            let started = std::time::Instant::now();
            let state = if let Some(rule) = rule {
                let source = Source::from(rule, &cfg);
                let cancel = CancelToken::new();
                let client = http.for_rule(&source.rule);
                match resolve_book(&cfg, &client, &source, &book_url, &cancel).await {
                    Ok((book, chapters)) => {
                        tracing::info!(
                            book = %book.book_name,
                            chapters = chapters.len(),
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "TOC 预取成功"
                        );
                        TocState::Loaded(Box::new(book), chapters)
                    }
                    Err(e) => {
                        tracing::warn!(
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            error = %format!("{e:#}"),
                            "TOC 预取失败"
                        );
                        TocState::Failed(format!("{e:#}"))
                    }
                }
            } else {
                tracing::warn!("TOC 预取失败: 书源未找到（可能已被删除）");
                TocState::Failed("书源未找到".to_string())
            };
            let _ = tx.send(TocEvent {
                source_id,
                url: book_url,
                state,
            });
            wakeup.notify();
        }
        .instrument(span_for_instrument),
    );

    rx
}

/// 记录下载任务的终态日志（ok / cancelled / failed）。
/// 消除 `spawn_download` / `spawn_download_range` 的 match 块复制。
fn log_download_outcome(
    result: Result<PathBuf, CrawlerError>,
    book_name: &str,
    started: std::time::Instant,
    label: &str,
    tx_for_failure: &mpsc::UnboundedSender<Progress>,
) {
    match result {
        Ok(_path) => {
            tracing::info!(
                book = %book_name,
                elapsed_ms = started.elapsed().as_millis() as u64,
                outcome = "ok",
                "下载任务终止{label}",
            );
        }
        Err(CrawlerError::Cancelled) => {
            // 用户取消 — Progress::Cancelled 已由 crawler 内部发；这里只补一条尾日志。
            tracing::info!(
                book = %book_name,
                elapsed_ms = started.elapsed().as_millis() as u64,
                outcome = "cancelled",
                "下载任务终止{label}",
            );
        }
        Err(e) => {
            let reason = format!("{e:#}");
            tracing::warn!(
                book = %book_name,
                elapsed_ms = started.elapsed().as_millis() as u64,
                outcome = "failed",
                error = %reason,
                "下载任务终止{label}",
            );
            let _ = tx_for_failure.send(Progress::Failed { reason });
        }
    }
}

/// 派一个指定章节范围的下载任务。跳过 resolve 阶段，直接进入下载。
/// `chapters` 已由调用方按用户选择过滤过范围。
pub fn spawn_download_range(
    ctx: &OpsCtx<'_>,
    next_task_id: u64,
    target: SearchResult,
    book: Book,
    chapters: Vec<Chapter>,
) -> (u64, DownloadTask) {
    let id = next_task_id;
    let (tx, rx) = mpsc::unbounded_channel::<Progress>();
    let cancel = CancelToken::new();

    let rule = ctx.rules.iter().find(|r| r.id == target.source_id).cloned();
    let cfg = ctx.config.clone();
    let http = Arc::clone(&ctx.http);
    let book_url = target.url.clone();
    let cancel_for_task = cancel.clone();
    let tx_for_task = tx.clone();

    let total = chapters.len();
    let book_for_meta = book.clone();
    let _ = tx_for_task.send(Progress::BookResolved {
        book: Box::new(book.clone()),
        total_chapters: total,
    });
    let wakeup_guard = ctx.wakeup.clone();
    wakeup_guard.notify();

    // 顶层 trace_id：一次下载 = 一个 trace_id；后续所有阶段共享。
    let trace_id = TraceId::mint();
    let span = tracing::info_span!(
        sub::DOWNLOAD,
        trace_id = %trace_id,
        task_id = id,
        source_id = target.source_id,
        book = %target.book_name,
        %book_url,
        total_chapters = total,
    );
    let span_for_instrument = span;
    let started = std::time::Instant::now();
    let book_name = target.book_name.clone();

    drop(tx);

    let wakeup_inner = ctx.wakeup.clone();

    ctx.runtime.spawn(
        async move {
            let Some(rule) = rule else {
                let _ = tx_for_task.send(Progress::Cancelled);
                tracing::warn!(
                    book = %book_name,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    outcome = "no_rule",
                    "下载任务终止（指定范围）"
                );
                return;
            };
            let source = Source::from(rule, &cfg);
            let client = http.for_rule(&source.rule);
            let tx_for_failure = tx_for_task.clone();
            let notify: std::option::Option<
                std::sync::Arc<dyn Fn() + std::marker::Send + std::marker::Sync>,
            > = Some(std::sync::Arc::new(move || wakeup_inner.notify()));
            let opts = DownloadOptions {
                progress: tx_for_task,
                cancel: cancel_for_task,
                notify,
            };
            let result = download_chapters(&cfg, &client, &source, &book, chapters, opts).await;
            log_download_outcome(result, &book_name, started, "（指定范围）", &tx_for_failure);
        }
        .instrument(span_for_instrument),
    );

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
        version: 0,
    };
    (id, task)
}

/// 派一个新的下载任务到后台。返回 `(task_id, task)`。
pub fn spawn_download(
    ctx: &OpsCtx<'_>,
    next_task_id: u64,
    target: SearchResult,
) -> (u64, DownloadTask) {
    let id = next_task_id;
    let (tx, rx) = mpsc::unbounded_channel::<Progress>();
    let cancel = CancelToken::new();

    let rule = ctx.rules.iter().find(|r| r.id == target.source_id).cloned();
    let cfg = ctx.config.clone();
    let http = Arc::clone(&ctx.http);
    let book_url = target.url.clone();
    let cancel_for_task = cancel.clone();
    let tx_for_task = tx.clone();
    // 顶层 trace_id：一次下载 = 一个 trace_id；后续所有阶段共享。
    let trace_id = TraceId::mint();
    let span = tracing::info_span!(
        sub::DOWNLOAD,
        trace_id = %trace_id,
        task_id = id,
        source_id = target.source_id,
        book = %target.book_name,
        %book_url,
    );
    let span_for_instrument = span;
    let started = std::time::Instant::now();
    let book_name = target.book_name.clone();
    let wakeup_inner = ctx.wakeup.clone();

    ctx.runtime.spawn(
        async move {
            let Some(rule) = rule else {
                let _ = tx_for_task.send(Progress::Cancelled);
                tracing::warn!(
                    book = %book_name,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    outcome = "no_rule",
                    "下载任务终止"
                );
                return;
            };
            let source = Source::from(rule, &cfg);
            let client = http.for_rule(&source.rule);
            // 留一个 sender 副本用于失败时发 Progress::Failed（tx_for_task 会 move 进 opts）。
            let tx_for_failure = tx_for_task.clone();
            let notify: std::option::Option<
                std::sync::Arc<dyn Fn() + std::marker::Send + std::marker::Sync>,
            > = Some(std::sync::Arc::new(move || wakeup_inner.notify()));
            let opts = DownloadOptions {
                progress: tx_for_task,
                cancel: cancel_for_task,
                notify,
            };
            let result = download_book(&cfg, &client, &source, &book_url, opts).await;
            log_download_outcome(result, &book_name, started, "", &tx_for_failure);
        }
        .instrument(span_for_instrument),
    );

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
        version: 0,
    };
    (id, task)
}

/// 清掉所有已结束的任务（完成 / 失败 / 取消）。运行中的任务保留。
pub fn clear_finished_tasks(tasks: &mut Vec<DownloadTask>) {
    let before = tasks.len();
    tasks.retain(super::super::download_task::DownloadTask::is_running);
    let removed = before - tasks.len();
    if removed > 0 {
        tracing::info!("已清除 {removed} 条任务");
    }
}
