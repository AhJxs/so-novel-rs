//! 下载 API (SSE 进度)。任务管理 / per-task drain 在 [`super::tasks`]。
//!
//! ## 事件流
//!
//! ```text
//! crawler  ──mpsc::UnboundedSender──▶  per-task drain  ──broadcast::Sender──▶  SSE
//!                                        │
//!                                        └─lock state.tasks, 更新 task 字段
//! ```
//!
//! - crawler 看到的还是 mpsc (crawler API 不变; 跟 GPUI 路径完全一致)
//! - 每个下载一个 per-task drain tokio task (不依赖中心循环), spawn 后自生自灭
//! - drain 既是单一 mpsc consumer (forward 引用全部权), 也是 broadcast producer
//!   + 状态更新者, 三者合一 → 不再有"状态更新到了 / broadcast 没发" / 反过来的
//!     漂移窗口
//! - SSE handler subscribe broadcast; 多个并发 SSE 客户端互不干扰

use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::response::Sse;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use crate::core::DownloadTask;
use crate::crawler::{self, CancelToken, DownloadOptions, Progress};
use crate::models::Source;
use crate::models::{Chapter, SearchResult};
use crate::utils::time::now_unix_secs;

use super::super::SharedState;
use super::super::error::read_state_or_sse;
use super::tasks::spawn_task_drain;
use crate::utils::lock::{mutex_or, rw_read_or};

/// SSE 流的内部类型别名, 集中在这里以便一处修改。
pub(super) type BoxedSseStream =
    std::pin::Pin<Box<dyn Stream<Item = Result<axum::response::sse::Event, Infallible>> + Send>>;

/// 在 SSE handler 入口拿到 poisoned lock 时, 把错误以 SSE `failed` event 形式
/// 给前端 (稳定字面量 + status code), 避免连接哑断。
///
/// Phase 3.8：本函数仍保留原签名，作为 [`read_state_or_sse`] 的 `make_stream` 回调；
/// 真正消除的是入口处的 match-IIFE 模板。
fn lock_failure_stream(status: u16, msg: &str) -> Sse<BoxedSseStream> {
    let reason = format!("[{status}] {msg}");
    let stream = async_stream::stream! {
        let ev = ProgressEvent {
            kind: "failed",
            reason: Some(reason),
            ..Default::default()
        };
        yield Ok(axum::response::sse::Event::default()
            .event("progress")
            .data(serde_json::to_string(&ev).unwrap_or_default()));
    };
    Sse::new(Box::pin(stream))
}

/// `POST /api/download` 请求体。
#[derive(Deserialize)]
pub struct DownloadRequest {
    pub url: String,
    pub source_id: i32,
    /// 搜索结果展示的书名 —— 在 `BookResolved` 事件抵达 drain 之前填充
    /// `origin.book_name`, 避免任务列表在最初的几个 frame 看到空书名。
    /// 旧 web 流程这里写 `String::new()` 是导致 `book_name: null` 的根因之一。
    pub book_name: Option<String>,
    pub format: Option<String>,
    pub chapter_start: Option<u32>,
    pub chapter_end: Option<u32>,
}

/// SSE `progress` 事件的 JSON 形态。
///
/// 前端按 `kind` 字段分发 (`book_resolved` / `chapter_done` / `chapter_failed` /
/// finished / cancelled / failed), 其它字段按需取, 全部 Option 序列化。
#[derive(Serialize, Default)]
pub(super) struct ProgressEvent {
    #[serde(rename = "type")]
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub book_name: Option<String>,
}

/// `POST /api/download` — 创建下载任务 + SSE 进度流。
///
/// ## 时序保证
///
/// 1. 任务先 push 进 `state.tasks` (可见性), 再 spawn drain, 再 spawn crawler
///    —— [`super::tasks::tasks_list`] 在返回 SSE 之前已经能看到这个 id
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
    State(state): State<SharedState>,
    Json(req): Json<DownloadRequest>,
) -> Sse<BoxedSseStream> {
    // Lock 失败也走 SSE 错误事件, 让前端看到稳定原因, 而不是连接哑断。
    // SSE handler 直接返 `Sse<...>` 而非 `Result<_, _>`, 不能用 `?` 早返;
    // 这里用 `match ... { Err(sse) => return sse }` 模式 (跟原 match-IIFE 等价).
    let config = match read_state_or_sse("download:cfg", lock_failure_stream, || {
        Ok(rw_read_or("download:cfg", &state.config)?.clone())
    }) {
        Ok(v) => v,
        Err(sse) => return sse,
    };
    let rule = match read_state_or_sse("download:rules", lock_failure_stream, || {
        Ok(rw_read_or("download:rules", &state.rules)?
            .iter()
            .find(|r| r.id == req.source_id)
            .cloned())
    }) {
        Ok(v) => v,
        Err(sse) => return sse,
    };

    let Some(rule) = rule else {
        let stream = async_stream::stream! {
            let ev = ProgressEvent {
                kind: "failed",
                reason: Some("书源未找到".into()),
                ..Default::default()
            };
            yield Ok(axum::response::sse::Event::default()
                .event("progress")
                .data(serde_json::to_string(&ev).unwrap_or_default()));
        };
        return Sse::new(Box::pin(stream));
    };

    // 1. mint id —— push 到 state.tasks 之前必须先有 id (其它请求靠它找任务)
    let task_id: u64 = match read_state_or_sse(
        "download:next_task_id",
        lock_failure_stream,
        || -> Result<u64, String> {
            let mut id = mutex_or("download:next_task_id", &state.next_task_id)?;
            let current = *id;
            *id += 1;
            // 显式 drop MutexGuard, 让锁尽早释放 (clippy::significant_drop_tightening)
            drop(id);
            Ok(current)
        },
    ) {
        Ok(v) => v,
        Err(sse) => return sse,
    };

    let mut config = config;
    if let Some(fmt) = &req.format {
        config.download.ext_name = crate::config::ExportFormat::parse(fmt);
    }

    let source = Source::from(rule, &config);
    let client = state.http.for_rule(&source.rule);
    let cancel = CancelToken::new();
    let (crawler_tx, crawler_rx) = mpsc::unbounded_channel::<Progress>();
    let (sse_tx, _) = broadcast::channel::<Progress>(256);

    // 2. 任务先入 state.tasks —— `tasks_list` 在 SSE 返回前已经能列到这条记录
    if let Err(sse) = read_state_or_sse(
        "download:push_task",
        lock_failure_stream,
        || -> Result<(), String> {
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
                        // 旧 web 这里写空串 → /tasks 返回 `book_name: null`。
                        // 改用请求里带的搜索书名 (前端可保证非空; 缺省时回退到空串,
                        // 后续 BookResolved 会被 drain 写入 book_meta, book_name() 仍可显示)。
                        book_name: req.book_name.clone().unwrap_or_default(),
                        ..Default::default()
                    },
                    // web 不在 struct 上挂 rx —— drain task 拥有它, drain 退出时丢。
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
        },
    ) {
        return sse;
    }
    if let Ok(tasks) = mutex_or("download:save_after_push", &state.tasks) {
        let _ = crate::db::save_with_trim(&state.tasks_file, &tasks);
    }

    // 3. spawn per-task drain
    spawn_task_drain(Arc::clone(&state), task_id, crawler_rx, sse_tx.clone());

    // 4. spawn crawler (吃掉 crawler_tx + cancel 的所有权)
    let book_url = req.url.clone();
    let state_for_crawler = Arc::clone(&state);
    let cancel_for_crawler = cancel;
    let sse_tx_for_crawler = sse_tx.clone();
    let chapter_start = req.chapter_start;
    let chapter_end = req.chapter_end;
    tokio::spawn(async move {
        let opts = DownloadOptions {
            progress: crawler_tx,
            cancel: cancel_for_crawler,
            // Web handler 暂未接 wakeup 通知 (SSE 走独立 channel, 见下面 sse_tx)
            notify: None,
        };

        let resolve_result =
            crawler::resolve_book(&config, &client, &source, &book_url, &opts.cancel).await;

        let (book, chapters) = match resolve_result {
            Ok((book, chapters)) => (book, chapters),
            Err(e) => {
                // resolve 失败也走 broadcast → SSE; 同时 drain 拿不到事件, drain
                // 会在 mpsc 断开时补 AppRestarted —— 这里显式发 Failed 优先
                // 让前端看到具体错误。
                let _ = sse_tx_for_crawler.send(Progress::Failed {
                    reason: format!("{e:#}"),
                });
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

    // 5. SSE 流 —— subscribe broadcast
    let mut sse_rx = sse_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match sse_rx.recv().await {
                Ok(progress) => {
                    let ev = match progress {
                        Progress::BookResolved { book, total_chapters } => ProgressEvent {
                            kind: "book_resolved",
                            book_name: Some(book.book_name),
                            total: Some(total_chapters),
                            ..Default::default()
                        },
                        Progress::ChapterDone { index, title } => ProgressEvent {
                            kind: "chapter_done",
                            index: Some(index),
                            title: Some(title),
                            ..Default::default()
                        },
                        Progress::ChapterFailed { index, title, reason } => ProgressEvent {
                            kind: "chapter_failed",
                            index: Some(index),
                            title: Some(title),
                            reason: Some(reason),
                            ..Default::default()
                        },
                        Progress::Finished { output_path } => {
                            let filename = output_path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            ProgressEvent {
                                kind: "finished",
                                task_id: Some(task_id),
                                filename: Some(filename),
                                ..Default::default()
                            }
                        }
                        Progress::Cancelled => ProgressEvent {
                            kind: "cancelled",
                            ..Default::default()
                        },
                        Progress::Failed { reason } => ProgressEvent {
                            kind: "failed",
                            reason: Some(reason),
                            ..Default::default()
                        },
                    };
                    let data = serde_json::to_string(&ev).unwrap_or_default();
                    let is_done = ev.kind == "finished" || ev.kind == "failed" || ev.kind == "cancelled";
                    yield Ok(axum::response::sse::Event::default()
                        .event("progress")
                        .data(data));
                    if is_done { break; }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(Box::pin(stream))
}
