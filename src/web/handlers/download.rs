//! 下载 API（JSON `task_id`，进度走任务轮询）。任务管理 / per-task drain 在 [`super::tasks`]。
//!
//! ## 数据流
//!
//! ```text
//! crawler  ──mpsc::UnboundedSender──▶  per-task drain  ──lock state.tasks, 更新 task 字段
//! ```
//!
//! - crawler 看到的还是 mpsc (crawler API 不变; 跟 GPUI 路径完全一致)
//! - 每个下载一个 per-task drain tokio task (不依赖中心循环), spawn 后自生自灭
//! - drain 是单一 mpsc consumer + 状态更新者, 二者合一 → 不再有"状态更新到了 /
//!   事件没发"的漂移窗口
//! - 前端轮询 `GET /api/tasks` 读进度 (`useTasks` 已有 refetchInterval 机制)

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::core::DownloadTask;
use crate::crawler::{self, CancelToken, CrawlerError, DownloadOptions, Progress};
use crate::i18n::ts_for_locale;
use crate::models::Source;
use crate::models::{Chapter, SearchResult};
use crate::utils::time::now_unix_secs;
use crate::web::error::WebError;
use crate::web::error::read_state_or_json;
use crate::web::error_code::ErrorCode;

use super::super::SharedState;
use super::tasks::spawn_task_drain;
use crate::utils::lock::{mutex_or, rw_read_or};
use crate::web::locale::Locale;

/// 把 [`CrawlerError`] 映射到稳定的 [`ErrorCode`] —— 与
/// [`crate::web::error::WebError::code`] 的 `Self::Crawler(_)` 分支同语义。
const fn crawler_err_code(e: &CrawlerError) -> ErrorCode {
    use CrawlerError as CE;
    use ErrorCode as C;
    match e {
        CE::EmptyToc => C::EmptyToc,
        CE::Client(_) => C::CrawlerClient,
        CE::Io(_) => C::CrawlerIo,
        CE::Export(_) => C::CrawlerExport,
        CE::Cancelled => C::Cancelled,
        CE::InvalidRange(_) => C::InvalidRange,
        CE::Book(_) => C::CrawlerBookAggregate,
        CE::Toc(_) => C::CrawlerTocAggregate,
    }
}

/// `POST /api/download` 请求体。
#[derive(Deserialize)]
pub struct DownloadRequest {
    pub url: String,
    pub source_id: i32,
    /// 搜索结果展示的书名 —— 在 `BookResolved` 事件抵达 drain 之前填充
    /// `origin.book_name`, 避免任务列表在最初的几个 frame 看到空书名。
    pub book_name: Option<String>,
    pub format: Option<String>,
    pub chapter_start: Option<u32>,
    pub chapter_end: Option<u32>,
}

/// `POST /api/download` 响应体（JSON，替代旧 SSE 进度流）。
#[derive(Serialize)]
pub struct DownloadResponse {
    pub task_id: u64,
}

/// `POST /api/download` — 创建下载任务并立即返回 `task_id`。
///
/// ## 时序保证
///
/// 1. 任务先 push 进 `state.tasks` (可见性), 再 spawn drain, 再 spawn crawler
///    —— 返回 `task_id` 时 `tasks_list` 已经能列到这条记录
/// 2. drain 收到 mpsc 断开 (crawler 退出) 时, 若 `finished.is_none()` 则标
///    `AppRestarted` 并 save (与 GPUI `DownloadTask::drain` 同语义)
/// 3. 用户 cancel: `task.cancelling = true` 立即反映到 `tasks_list`;
///    `Progress::Cancelled` 经 mpsc → drain → `apply_progress` 最终落 `UserCancelled`
#[tracing::instrument(
    name = "web::download",
    skip_all,
    fields(
        source_id = req.source_id,
        %req.url,
        chapter_start = ?req.chapter_start,
        chapter_end = ?req.chapter_end,
    )
)]
pub async fn download(
    Locale(locale): Locale,
    State(state): State<SharedState>,
    Json(req): Json<DownloadRequest>,
) -> Result<Json<DownloadResponse>, WebError> {
    let config = read_state_or_json("download:cfg", || {
        Ok(rw_read_or("download:cfg", &state.config)?.clone())
    })?;
    let rule = read_state_or_json("download:rules", || {
        Ok(rw_read_or("download:rules", &state.rules)?
            .iter()
            .find(|r| r.id == req.source_id)
            .cloned())
    })?;

    let Some(rule) = rule else {
        return Err(WebError::NotFound("source"));
    };

    // 1. mint id —— push 到 state.tasks 之前必须先有 id (其它请求靠它找任务)
    let task_id: u64 = read_state_or_json("download:next_task_id", || -> Result<u64, String> {
        let mut id = mutex_or("download:next_task_id", &state.next_task_id)?;
        let current = *id;
        *id += 1;
        // 显式 drop MutexGuard, 让锁尽早释放 (clippy::significant_drop_tightening)
        drop(id);
        Ok(current)
    })?;

    let mut config = config;
    if let Some(fmt) = &req.format {
        config.download.ext_name = crate::config::ExportFormat::parse(fmt);
    }

    let source = Source::from(rule, &config);
    let client = state.http.for_rule(&source.rule);
    let cancel = CancelToken::new();
    let (crawler_tx, crawler_rx) = mpsc::unbounded_channel::<Progress>();

    // 2. 任务先入 state.tasks —— 返回 task_id 时 `tasks_list` 已经能列到这条记录
    read_state_or_json("download:push_task", || -> Result<(), String> {
        // 用块作用域把 MutexGuard 提前 drop, 避免 clippy
        // `significant_drop_tightening` (guard 持有到闭包结尾).
        {
            let mut tasks = mutex_or("download:push_task", &state.tasks)?;
            tasks.push(DownloadTask {
                id: task_id,
                origin: SearchResult {
                    source_id: source.rule.id,
                    source_name: source.rule.name.clone(),
                    url: req.url.clone(),
                    book_name: req.book_name.clone().unwrap_or_default(),
                    ..Default::default()
                },
                rx: None,
                cancel: Some(cancel.clone()),
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
            });
        }
        Ok(())
    })?;
    if let Ok(tasks) = mutex_or("download:save_after_push", &state.tasks) {
        let _ = crate::db::save_with_trim(&state.tasks_file, &tasks);
    }

    // 3. spawn per-task drain
    spawn_task_drain(Arc::clone(&state), task_id, crawler_rx);

    // 4. spawn crawler (吃掉 crawler_tx + cancel 的所有权)
    let book_url = req.url.clone();
    let state_for_crawler = Arc::clone(&state);
    let cancel_for_crawler = cancel;
    let chapter_start = req.chapter_start;
    let chapter_end = req.chapter_end;
    tokio::spawn(async move {
        let opts = DownloadOptions {
            progress: crawler_tx,
            cancel: cancel_for_crawler,
            notify: None,
        };

        let resolve_result =
            crawler::resolve_book(&config, &client, &source, &book_url, &opts.cancel).await;

        let (book, chapters) = match resolve_result {
            Ok((book, chapters)) => (book, chapters),
            Err(e) => {
                // resolve 失败：经 mpsc 走 drain（替代旧 broadcast）—— drain 的
                // `apply_progress` 会把任务标 Failed，随后 mpsc 关闭时因
                // `finished.is_some()` 不会被 AppRestarted 覆盖。
                let code = crawler_err_code(&e);
                tracing::warn!(
                    task_id,
                    cause = %format!("{e:#}"),
                    key = code.key(),
                    "download resolve_book failed"
                );
                let reason = ts_for_locale(locale, code.key());
                let _ = opts.progress.send(Progress::Failed { reason });
                if let Ok(tasks) = state_for_crawler.tasks.lock() {
                    let _ = crate::db::save_with_trim(&state_for_crawler.tasks_file, &tasks);
                }
                return;
            }
        };

        let chapters: Vec<Chapter> = if let (Some(start), Some(end)) = (chapter_start, chapter_end)
        {
            chapters
                .into_iter()
                .filter(|c| c.order >= start && c.order <= end)
                .collect()
        } else {
            chapters
        };

        // 对齐 GPUI `spawn_download_range`: 在 `download_chapters` 前手动发
        // `BookResolved`. `download_chapters` 内部只发 Cancelled/ChapterDone/
        // ChapterFailed/Finished —— 不补这一发 drain 拿不到 book_meta /
        // total_chapters, 任务列表 book_name=null、total_chapters=0。
        let _ = opts.progress.send(Progress::BookResolved {
            book: Box::new(book.clone()),
            total_chapters: chapters.len(),
        });

        let result =
            crawler::download_chapters(&config, &client, &source, &book, chapters, opts).await;

        // crawler 退出 → drop(progress: crawler_tx) → drain 端 mpsc recv 返 None
        // → drain 退出循环并 save. 这里再 save 一次兜底: crawler 路径上某些 early
        // return (如 resolve 失败) drain 看不到任何终结事件, drain 还是会标
        // AppRestarted + save, 但显式 save 不亏。
        let _ = result;
        if let Ok(tasks) = state_for_crawler.tasks.lock() {
            let _ = crate::db::save_with_trim(&state_for_crawler.tasks_file, &tasks);
        }
    });

    // 5. 立即返回 task_id —— 前端轮询 GET /api/tasks 看进度
    Ok(Json(DownloadResponse { task_id }))
}
