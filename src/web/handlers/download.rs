//! 下载 API（SSE 进度）+ 任务管理。

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Json, Sse};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::crawler::{self, CancelToken, DownloadOptions, Progress};
use crate::models::{Chapter, DownloadTaskRecord, FailureRecord, FinishedReason};
use crate::rules::Source;

use super::super::{ActiveDownload, SharedState, TaskStatus};

/// 将任务历史持久化到 `tasks.json`（失败仅 warn，不阻塞）。
fn persist_tasks(state: &SharedState) {
    let mut history = state.task_history.lock().unwrap();
    if let Err(e) = crate::persistent::save_with_trim(&state.tasks_file, &mut history) {
        tracing::warn!("保存 tasks.json 失败: {e}");
    }
}

#[derive(Deserialize)]
pub struct DownloadRequest {
    url: String,
    source_id: i32,
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

pub async fn download(
    State(state): State<SharedState>,
    Json(req): Json<DownloadRequest>,
) -> Sse<BoxedSseStream> {
    let (config, rule) = {
        let cfg = state.config.read().unwrap();
        let rules = state.rules.read().unwrap();
        let rule = rules.iter().find(|r| r.id == req.source_id).cloned();
        (cfg.clone(), rule)
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

    let task_id = {
        let mut id = state.next_task_id.lock().unwrap();
        let current = *id;
        *id += 1;
        current
    };

    let mut config = config;
    if let Some(fmt) = &req.format {
        config.ext_name = crate::config::ExportFormat::parse(fmt);
    }

    let source = Source::from(rule, &config);
    let client = state.http.for_rule(&source.rule);
    let cancel = CancelToken::new();
    let (broadcast_tx, _) = broadcast::channel::<Progress>(256);

    {
        let mut downloads = state.active_downloads.lock().unwrap();
        downloads.insert(
            task_id,
            ActiveDownload {
                cancel: cancel.clone(),
                progress_tx: broadcast_tx.clone(),
                filename: None,
                book_name: None,
                total_chapters: 0,
                current_chapter: 0,
                status: TaskStatus::Downloading,
            },
        );
    }

    // 插入历史任务记录
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let mut history = state.task_history.lock().unwrap();
        history.push(DownloadTaskRecord {
            id: task_id,
            origin: crate::models::SearchResult {
                source_id: source.rule.id,
                source_name: source.rule.name.clone(),
                url: req.url.clone(),
                book_name: String::new(),
                ..Default::default()
            },
            started_at_unix: now,
            finished_at_unix: None,
            book_meta: None,
            total_chapters: 0,
            completed: 0,
            failed: 0,
            last_chapter_title: String::new(),
            finished: None,
            failures: Vec::new(),
        });
    }

    let book_url = req.url.clone();
    let cancel_clone = cancel.clone();
    let broadcast_tx_clone = broadcast_tx.clone();
    let state_clone = Arc::clone(&state);

    let (crawler_tx, mut crawler_rx) = tokio::sync::mpsc::unbounded_channel::<Progress>();

    let broadcast_tx_bridge = broadcast_tx.clone();
    let state_bridge = Arc::clone(&state);
    tokio::spawn(async move {
        while let Some(progress) = crawler_rx.recv().await {
            // 更新 ActiveDownload 状态 + 历史记录
            {
                let mut downloads = state_bridge.active_downloads.lock().unwrap();
                if let Some(dl) = downloads.get_mut(&task_id) {
                    match &progress {
                        Progress::BookResolved {
                            book,
                            total_chapters,
                        } => {
                            dl.book_name = Some(book.book_name.clone());
                            dl.total_chapters = *total_chapters;
                        }
                        Progress::ChapterDone { index, .. } => {
                            dl.current_chapter = *index;
                        }
                        Progress::Finished { .. } => {
                            dl.status = TaskStatus::Finished;
                        }
                        Progress::Failed { .. } => {
                            dl.status = TaskStatus::Failed;
                        }
                        Progress::Cancelled => {
                            dl.status = TaskStatus::Cancelled;
                        }
                        _ => {}
                    }
                }
            }
            // 同步更新历史记录
            {
                let mut history = state_bridge.task_history.lock().unwrap();
                if let Some(rec) = history.iter_mut().find(|r| r.id == task_id) {
                    match &progress {
                        Progress::BookResolved {
                            book,
                            total_chapters,
                        } => {
                            rec.book_meta = Some(*book.clone());
                            rec.total_chapters = *total_chapters;
                            rec.origin.book_name = book.book_name.clone();
                        }
                        Progress::ChapterDone { index, title } => {
                            rec.completed = *index;
                            rec.last_chapter_title = title.clone();
                        }
                        Progress::ChapterFailed {
                            index,
                            title,
                            reason,
                        } => {
                            rec.failed += 1;
                            rec.failures.push(FailureRecord {
                                index: *index,
                                title: title.clone(),
                                reason: reason.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            }
            let _ = broadcast_tx_bridge.send(progress);
        }
    });

    tokio::spawn(async move {
        let opts = DownloadOptions {
            progress: crawler_tx,
            cancel: cancel_clone,
        };

        let resolve_result =
            crawler::resolve_book(&config, &client, &source, &book_url, &opts.cancel).await;

        let (book, chapters) = match resolve_result {
            Ok((book, chapters)) => (book, chapters),
            Err(e) => {
                let _ = broadcast_tx_clone.send(Progress::Failed {
                    reason: format!("{e:#}"),
                });
                return;
            }
        };

        let chapters: Vec<Chapter> =
            if let (Some(start), Some(end)) = (req.chapter_start, req.chapter_end) {
                chapters
                    .into_iter()
                    .filter(|c| c.order >= start && c.order <= end)
                    .collect()
            } else {
                chapters
            };

        let result =
            crawler::download_chapters(&config, &client, &source, &book, chapters, opts).await;

        // 结束时更新历史记录
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        match result {
            Ok(path) => {
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();
                {
                    let mut downloads = state_clone.active_downloads.lock().unwrap();
                    if let Some(dl) = downloads.get_mut(&task_id) {
                        dl.filename = Some(filename.clone());
                    }
                }
                // 更新历史记录：完成
                {
                    let mut history = state_clone.task_history.lock().unwrap();
                    if let Some(rec) = history.iter_mut().find(|r| r.id == task_id) {
                        rec.finished_at_unix = Some(now);
                        rec.finished = Some(Ok(path.clone()));
                    }
                }
                persist_tasks(&state_clone);
                let _ = broadcast_tx_clone.send(Progress::Finished { output_path: path });
            }
            Err(e) => {
                if matches!(e, crawler::CrawlerError::Cancelled) {
                    // 更新历史记录：用户取消
                    {
                        let mut history = state_clone.task_history.lock().unwrap();
                        if let Some(rec) = history.iter_mut().find(|r| r.id == task_id) {
                            rec.finished_at_unix = Some(now);
                            rec.finished = Some(Err(FinishedReason::UserCancelled));
                        }
                    }
                    persist_tasks(&state_clone);
                } else {
                    // 更新历史记录：失败
                    {
                        let mut history = state_clone.task_history.lock().unwrap();
                        if let Some(rec) = history.iter_mut().find(|r| r.id == task_id) {
                            rec.finished_at_unix = Some(now);
                            rec.finished = Some(Err(FinishedReason::Failed {
                                message: format!("{e:#}"),
                            }));
                        }
                    }
                    persist_tasks(&state_clone);
                    let _ = broadcast_tx_clone.send(Progress::Failed {
                        reason: format!("{e:#}"),
                    });
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        state_clone
            .active_downloads
            .lock()
            .unwrap()
            .remove(&task_id);
    });

    let mut rx = broadcast_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
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
    status: super::super::TaskStatus,
    started_at_unix: i64,
    finished_at_unix: Option<i64>,
}

/// 将 `DownloadTaskRecord` 转换为 `TaskInfo` 用于 API 返回。
fn record_to_info(rec: &DownloadTaskRecord) -> TaskInfo {
    let status = match &rec.finished {
        Some(Ok(_)) => TaskStatus::Finished,
        Some(Err(FinishedReason::UserCancelled)) => TaskStatus::Cancelled,
        Some(Err(FinishedReason::AppRestarted)) => TaskStatus::Cancelled,
        Some(Err(FinishedReason::Failed { .. })) => TaskStatus::Failed,
        None => TaskStatus::Downloading,
    };
    TaskInfo {
        id: rec.id,
        filename: rec
            .finished
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
            }),
        book_name: rec.book_meta.as_ref().map(|b| b.book_name.clone()),
        total_chapters: rec.total_chapters,
        current_chapter: rec.completed,
        status,
        started_at_unix: rec.started_at_unix,
        finished_at_unix: rec.finished_at_unix,
    }
}

pub async fn tasks_list(State(state): State<SharedState>) -> Json<Vec<TaskInfo>> {
    // 合并活跃任务 + 历史记录（活跃任务优先，覆盖历史中同 ID 的条目）。
    let mut result: Vec<TaskInfo> = {
        let history = state.task_history.lock().unwrap();
        history.iter().map(record_to_info).collect()
    };

    {
        let downloads = state.active_downloads.lock().unwrap();
        for (id, dl) in downloads.iter() {
            let info = TaskInfo {
                id: *id,
                filename: dl.filename.clone(),
                book_name: dl.book_name.clone(),
                total_chapters: dl.total_chapters,
                current_chapter: dl.current_chapter,
                status: dl.status,
                started_at_unix: 0, // 活跃任务的 started_at_unix 从历史取
                finished_at_unix: None,
            };
            // 用活跃状态覆盖历史记录
            if let Some(existing) = result.iter_mut().find(|t| t.id == *id) {
                existing.filename = info.filename;
                existing.book_name = info.book_name;
                existing.total_chapters = info.total_chapters;
                existing.current_chapter = info.current_chapter;
                existing.status = info.status;
            } else {
                result.push(info);
            }
        }
    }

    // 按 id 降序（最新任务在前）。
    result.sort_by_key(|b| std::cmp::Reverse(b.id));
    Json(result)
}

pub async fn task_cancel(
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<&'static str, (StatusCode, String)> {
    let downloads = state.active_downloads.lock().unwrap();
    if let Some(dl) = downloads.get(&id) {
        dl.cancel.cancel();
        Ok("已取消")
    } else {
        Err((StatusCode::NOT_FOUND, "任务未找到".to_string()))
    }
}
