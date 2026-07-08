//! 下载 API（SSE 进度）+ 任务管理。
//!
//! ## 任务存储：单源 `Vec<DownloadTask>`（跟 GPUI 一致）
//!
//! 旧实现是双 store（`HashMap<u64, ActiveDownload>` + `Vec<DownloadTaskRecord>` + 一个
//! bridge task 来回同步），反复出现 active 拿到 `(None, 0)` 把 history 修正过的元数据
//! 盖回去、或者 `Finished` 状态被 `Downloading` 覆盖之类的 bug。根因是同一份数据
//! 两边各持一份，时序窗口不一致。
//!
//! 现在 web 跟 GPUI 一样只持一个 `Vec<DownloadTask>`，所有字段（持久化 + 运行期 +
//! rx/cancel）都在这个 struct 上。
//!
//! ## 事件流
//!
//! ```text
//! crawler  ──mpsc::UnboundedSender──▶  per-task drain  ──broadcast::Sender──▶  SSE
//!                                        │
//!                                        └─lock state.tasks, 更新 task 字段
//! ```
//!
//!  - crawler 看到的还是 mpsc（crawler API 不变；跟 GPUI 路径完全一致）
//!  - 每个下载一个 per-task drain tokio task（不依赖中心循环），spawn 后自生自灭
//!  - drain 既是单一 mpsc consumer（forward 引用全部权），也是 broadcast producer
//!    + 状态更新者，三者合一 → 不再有"状态更新到了 / broadcast 没发" / 反过来的
//!      漂移窗口
//!  - SSE handler subscribe broadcast；多个并发 SSE 客户端互不干扰
//!
//! ## 时序保证
//!
//! 1. 任务先 push 进 `state.tasks`（可见性），再 spawn drain，再 spawn crawler
//!    —— `tasks_list` 在返回 SSE 之前已经能看到这个 id
//! 2. drain 收到 mpsc 断开（crawler 退出）时，若 `finished.is_none()` 则标
//!    `AppRestarted` 并 save（与 GPUI `DownloadTask::drain` 同语义）
//! 3. 用户 cancel：`task.cancelling = true` 立即反映到 `tasks_list`；
//    `Progress::Cancelled` 经 mpsc → drain → apply_progress 最终落 `UserCancelled`

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Json, Sse};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};

use crate::app::DownloadTask;
use crate::crawler::{self, CancelToken, DownloadOptions, Progress};
use crate::models::Source;
use crate::models::{Chapter, FinishedReason, SearchResult};
use crate::utils::time::now_unix_secs;

use super::super::{SharedState, TaskStatus, WebState};
use super::lock::{mutex, rw_read};

/// 在 SSE handler 入口拿到 poisoned lock 时，把错误以 SSE `failed` event 形式
/// 给前端（稳定字面量 + status code），避免连接哑断。
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

#[derive(Deserialize)]
pub struct DownloadRequest {
    url: String,
    source_id: i32,
    /// 搜索结果展示的书名 —— 在 `BookResolved` 事件抵达 drain 之前填充
    /// `origin.book_name`，避免任务列表在最初的几个 frame 看到空书名。
    /// 旧 web 流程这里写 `String::new()` 是导致 `book_name: null` 的根因之一。
    book_name: Option<String>,
    format: Option<String>,
    chapter_start: Option<u32>,
    chapter_end: Option<u32>,
}

#[derive(Serialize, Default)]
struct ProgressEvent {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    book_name: Option<String>,
}

type BoxedSseStream =
    std::pin::Pin<Box<dyn Stream<Item = Result<axum::response::sse::Event, Infallible>> + Send>>;

/// 单个下载任务的 per-task drain。
///
/// 三件事合一：
/// 1. 单一 mpsc consumer（`crawler_rx`）—— 没人能 race
/// 2. 状态更新者：lock `state.tasks` → 找对应 id → `task.apply_progress(ev)`
/// 3. broadcast producer：把同一事件转发给 SSE subscribers
///
/// 退出条件：`crawler_rx.recv()` 返回 `None`（crawler 退出发送端被 drop）。
/// 退出前若 `finished.is_none()` 标 `AppRestarted`（对齐 GPUI `DownloadTask::drain`
/// 的 `TryRecvError::Disconnected` 分支），并触发一次 `save_tasks_to_file` 兜底
/// 落盘 —— 不依赖中心 tick，drain 退出后所有变更都已经被保存。
fn spawn_task_drain(
    state: Arc<WebState>,
    task_id: u64,
    mut crawler_rx: mpsc::UnboundedReceiver<Progress>,
    sse_tx: broadcast::Sender<Progress>,
) {
    tokio::spawn(async move {
        while let Some(progress) = crawler_rx.recv().await {
            // 1. 锁 + 更新 in-memory task；poison 状态跳过（drain 里没法返 500，
            //    也不应让 worker panic —— 标记 lost-progress 后继续 SSE 转发）
            match state.tasks.lock() {
                Ok(mut tasks) => {
                    if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                        task.apply_progress(progress.clone());
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "spawn_task_drain {task_id}: tasks Mutex poisoned, drop progress: {e}"
                    );
                }
            }
            // 2. 转发给 SSE subscribers（多 client 并发各自 lagging 互不干扰）
            let _ = sse_tx.send(progress);
        }

        // mpsc 断开：crawler 已退出。若任务还没走到 finished 态，补 AppRestarted
        // + finished_at_unix，然后 save。
        let needs_save = match state.tasks.lock() {
            Ok(mut tasks) => {
                let mut changed = false;
                if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                    if task.finished.is_none() {
                        task.finished = Some(Err(FinishedReason::AppRestarted));
                        changed = true;
                    }
                    if task.finished_at_unix.is_none() {
                        task.finished_at_unix = Some(now_unix_secs());
                        changed = true;
                    }
                }
                changed
            }
            Err(e) => {
                tracing::error!(
                    "spawn_task_drain {task_id}: tasks Mutex poisoned on exit, skip AppRestarted: {e}"
                );
                false
            }
        };
        if needs_save {
            state.save_tasks_to_file();
        }
    });
}

pub async fn download(
    State(state): State<SharedState>,
    Json(req): Json<DownloadRequest>,
) -> Sse<BoxedSseStream> {
    // Lock 失败也走 SSE 错误事件，让前端看到稳定原因，而不是连接哑断。
    let (config, rule) = match (|| -> Result<_, (StatusCode, String)> {
        let cfg = rw_read("download:cfg", &state.config)?;
        let rules = rw_read("download:rules", &state.rules)?;
        let rule = rules.iter().find(|r| r.id == req.source_id).cloned();
        Ok((cfg.clone(), rule))
    })() {
        Ok(v) => v,
        Err((code, msg)) => return lock_failure_stream(code.as_u16(), &msg),
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

    // 1. mint id —— push 到 state.tasks 之前必须先有 id（其它请求靠它找任务）
    let task_id = match (|| -> Result<_, (StatusCode, String)> {
        let mut id = mutex("download:next_task_id", &state.next_task_id)?;
        let current = *id;
        *id += 1;
        Ok(current)
    })() {
        Ok(v) => v,
        Err((code, msg)) => return lock_failure_stream(code.as_u16(), &msg),
    };

    let mut config = config;
    if let Some(fmt) = &req.format {
        config.ext_name = crate::config::ExportFormat::parse(fmt);
    }

    let source = Source::from(rule, &config);
    let client = state.http.for_rule(&source.rule);
    let cancel = CancelToken::new();
    let (crawler_tx, crawler_rx) = mpsc::unbounded_channel::<Progress>();
    let (sse_tx, _) = broadcast::channel::<Progress>(256);

    // 2. 任务先入 state.tasks —— `tasks_list` 在 SSE 返回前已经能列到这条记录
    if let Err((code, msg)) = (|| -> Result<(), (StatusCode, String)> {
        let mut tasks = mutex("download:push_task", &state.tasks)?;
        tasks.push(DownloadTask {
            id: task_id,
            origin: SearchResult {
                source_id: source.rule.id,
                source_name: source.rule.name.clone(),
                url: req.url.clone(),
                // 旧 web 这里写空串 → /tasks 返回 `book_name: null`。
                // 改用请求里带的搜索结果书名（前端可保证非空；缺省时回退到空串，
                // 后续 BookResolved 会被 drain 写入 book_meta，book_name() 仍可显示）。
                book_name: req.book_name.clone().unwrap_or_default(),
                ..Default::default()
            },
            // web 不在 struct 上挂 rx —— drain task 拥有它，drain 退出时丢。
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
        Ok(())
    })() {
        return lock_failure_stream(code.as_u16(), &msg);
    }
    state.save_tasks_to_file();

    // 3. spawn per-task drain
    spawn_task_drain(Arc::clone(&state), task_id, crawler_rx, sse_tx.clone());

    // 4. spawn crawler（吃掉 crawler_tx + cancel 的所有权）
    let book_url = req.url.clone();
    let state_for_crawler = Arc::clone(&state);
    let cancel_for_crawler = cancel.clone();
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
                // resolve 失败也走 broadcast → SSE；同时 drain 拿不到事件，drain
                // 会在 mpsc 断开时补 AppRestarted —— 这里显式发 Failed 优先
                // 让前端看到具体错误。
                let _ = sse_tx_for_crawler.send(Progress::Failed {
                    reason: format!("{e:#}"),
                });
                state_for_crawler.save_tasks_to_file();
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

        // 对齐 GPUI `spawn_download_range`：在 `download_chapters` 前手动发
        // `BookResolved`。`download_chapters` 内部只发 Cancelled/ChapterDone/
        // ChapterFailed/Finished —— 不补这一发 drain 拿不到 book_meta /
        // total_chapters，任务列表 book_name=null、total_chapters=0。
        let _ = opts.progress.send(Progress::BookResolved {
            book: Box::new(book.clone()),
            total_chapters: chapters.len(),
        });

        let result =
            crawler::download_chapters(&config, &client, &source, &book, chapters, opts).await;

        // crawler 退出 → drop(progress: crawler_tx) → drain 端 mpsc recv 返 None
        // → drain 退出循环并 save。这里再 save 一次兜底：crawler 路径上某些 early
        // return（如 resolve 失败）drain 看不到任何终结事件，drain 还是会标
        // AppRestarted + save，但显式 save 不亏。
        let _ = result;
        state_for_crawler.save_tasks_to_file();
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
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(Box::pin(stream))
}

// ─── 任务管理 ────────────────────────────────────────

#[derive(Serialize)]
pub(crate) struct TaskInfo {
    id: u64,
    filename: Option<String>,
    book_name: Option<String>,
    total_chapters: usize,
    current_chapter: u32,
    /// 已失败章节数（与 GPUI `DownloadTask::failed` 同语义，前端 UI 用作红色 chip）。
    failed: u32,
    status: super::super::TaskStatus,
    started_at_unix: i64,
    finished_at_unix: Option<i64>,
}

/// 从 `book.epub` / `book(作者).txt` 等文件名里粗略抽书名 —— 只作 fallback：
/// 历史里 `book_meta` 缺失（旧任务漏发过 `BookResolved`）时，至少让 UI 显示个名字。
/// 规则：去掉扩展名，再把尾部 `(...作者)` / `（...作者）` 整段砍掉。
fn derive_book_name_from_filename(filename: &str) -> Option<String> {
    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())?;
    let cut = stem.rfind(['(', '（']).unwrap_or(stem.len());
    let without_author = stem[..cut].trim_end();
    let result = if without_author.is_empty() {
        stem.trim()
    } else {
        without_author
    };
    if result.is_empty() {
        None
    } else {
        Some(result.to_string())
    }
}

/// `DownloadTask` → `TaskInfo`。
///
/// 关键点：不再 merge 两个 store —— `state.tasks` 是唯一来源。
fn task_to_info(task: &DownloadTask) -> TaskInfo {
    let status = match &task.finished {
        Some(Ok(_)) => TaskStatus::Finished,
        Some(Err(FinishedReason::UserCancelled)) => TaskStatus::Cancelled,
        Some(Err(FinishedReason::AppRestarted)) => TaskStatus::Cancelled,
        Some(Err(FinishedReason::Failed { .. })) => TaskStatus::Failed,
        None => TaskStatus::Downloading,
    };
    let filename = task
        .finished
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .and_then(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        });
    // book_name 优先：book_meta（BookResolved 之后）> origin.book_name（请求里带的搜索书名）。
    // 两者都为空时回退到 finished output filename 派生（仅 Finished 任务有值）。
    let book_name = {
        let direct = task.book_name();
        if !direct.is_empty() {
            Some(direct.to_string())
        } else {
            filename.as_deref().and_then(derive_book_name_from_filename)
        }
    };
    TaskInfo {
        id: task.id,
        filename,
        book_name,
        total_chapters: task.total_chapters,
        current_chapter: task.completed,
        failed: task.failed,
        status,
        started_at_unix: task.started_at_unix,
        finished_at_unix: task.finished_at_unix,
    }
}

pub async fn tasks_list(
    State(state): State<SharedState>,
) -> Result<Json<Vec<TaskInfo>>, (StatusCode, String)> {
    let tasks = mutex("tasks_list", &state.tasks)?;
    let mut result: Vec<TaskInfo> = tasks.iter().map(task_to_info).collect();
    drop(tasks);
    // 按 id 降序（最新任务在前）。
    result.sort_by_key(|b| std::cmp::Reverse(b.id));
    Ok(Json(result))
}

pub async fn task_cancel(
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<&'static str, (StatusCode, String)> {
    let mut tasks = mutex("task_cancel", &state.tasks)?;
    let Some(task) = tasks.iter_mut().find(|t| t.id == id) else {
        return Err((StatusCode::NOT_FOUND, "任务未找到".to_string()));
    };
    // 任务已结束：cancel 不会触发任何 crawler 状态变化（crawler 已退出）。
    // 返回 409 提示前端"无法取消已结束任务"，避免前端 cancel 按钮无响应却显示 ok。
    if task.finished.is_some() {
        return Err((StatusCode::CONFLICT, "任务已结束,无法取消".to_string()));
    }
    let Some(cancel) = task.cancel.as_ref() else {
        return Err((StatusCode::NOT_FOUND, "任务未找到".to_string()));
    };
    // 立即翻 cancelling 标记（前端可显示"正在取消..."），cancel.cancel() 同步触发
    // crawler 内部的 CancelToken；crawler 下一次 progress tick 看到 cancel 时会发
    // Progress::Cancelled → drain → apply_progress 落 UserCancelled。
    task.cancelling = true;
    cancel.cancel();
    Ok("已取消")
}

/// 从 `state.tasks` 移除一条任务记录，**不动磁盘**。
///
/// 跟 `task_cancel` 的语义区别：
/// - `cancel`：仅对 `Downloading` 任务有意义（触发 crawler stop），已结束任务是 409
/// - `delete`：纯 metadata 清理。任何 `finished.is_some()` 的任务都能删，活跃任务也
///   允许 —— 删后该任务的 in-flight crawler / drain 还在跑，自然写入
///   `state.tasks` 的旧下标位置已经不存在，但 `apply_progress` 是按 `id` 查找的，
///   find 会 no-op，drain 退出时 save 兜底空 vec 也写。**、简单粗暴地对
///   tasks.json 做一次 trim —— 这是用户主动清理历史，不算脏数据。
///
/// 跟 library delete 的语义区别：library delete 删的是磁盘文件；这里删的是
/// tasks.json 里的任务记录。两个端点分开是因为用途不同：
///   - 在 /tasks 页面删 → 删记录（保留文件）
///   - 在 /library 页面删 → 删文件（记录留着也无害：下一次 /api/tasks 重启时
///     会从 .tasks.json 加载，但既然文件没了，UI 上 `filename` 是孤儿）
pub async fn task_delete(
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<&'static str, (StatusCode, String)> {
    let mut tasks = mutex("task_delete", &state.tasks)?;
    let initial_len = tasks.len();
    tasks.retain(|t| t.id != id);
    if tasks.len() == initial_len {
        return Err((StatusCode::NOT_FOUND, "任务未找到".to_string()));
    }
    drop(tasks);
    state.save_tasks_to_file();
    Ok("已删除任务")
}
