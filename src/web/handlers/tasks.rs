//! 下载任务管理端点 + per-task drain 基础设施。
//!
//! 任务存储是单源 `Vec<DownloadTask>`（跟 GPUI 一致）；双 store + bridge task 会因
//! 时序窗口不一致互相覆盖（`Finished` 被 `Downloading` 盖回去），已废弃。
//! 顺序不能变：先 push 进 `state.tasks`（可见性）→ 再 spawn drain → 再 spawn crawler。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use tokio::sync::mpsc;

use crate::core::DownloadTask;
use crate::crawler::Progress;
use crate::i18n::ts_for_locale;
use crate::models::FinishedReason;
use crate::utils::time::now_unix_secs;

use super::super::{TaskStatus, WebState};
use crate::utils::lock::mutex_or;
use crate::web::SharedState;
use crate::web::error::{WebError, read_state_or_json};
use crate::web::locale::Locale;

/// `GET /api/tasks` 响应体。
#[derive(Serialize)]
pub struct TaskInfo {
    pub id: u64,
    pub filename: Option<String>,
    pub book_name: Option<String>,
    pub total_chapters: usize,
    pub current_chapter: u32,
    /// 已失败章节数（与 GPUI `DownloadTask::failed` 同语义, 前端 UI 用作红色 chip）。
    pub failed: u32,
    pub status: TaskStatus,
    pub started_at_unix: i64,
    pub finished_at_unix: Option<i64>,
}

/// 从 `book(作者).txt` 等文件名粗略抽书名 —— 只作 fallback：历史任务可能漏发
/// `BookResolved` 导致 `book_meta` 缺失，至少让 UI 有名字显示。
/// 规则: 去掉扩展名, 再把尾部 `(...作者)` / `（...作者）` 整段砍掉。
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

/// `DownloadTask` → `TaskInfo`（`state.tasks` 是唯一数据来源，不 merge 别的 store）。
fn task_to_info(task: &DownloadTask) -> TaskInfo {
    let status = match &task.finished {
        Some(Ok(_)) => TaskStatus::Finished,
        Some(Err(FinishedReason::UserCancelled | FinishedReason::AppRestarted)) => {
            TaskStatus::Cancelled
        }
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
                .map(std::string::ToString::to_string)
        });
    // book_name 优先 book_meta（BookResolved 之后）> origin.book_name（请求里的搜索书名），
    // 两者都空才回退到 finished 文件名派生（仅 Finished 任务有值）。
    let book_name = {
        let direct = task.book_name();
        if direct.is_empty() {
            filename.as_deref().and_then(derive_book_name_from_filename)
        } else {
            Some(direct.to_string())
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

/// `GET /api/tasks` — 列出所有任务, 按 id 降序 (最新在前)。
///
/// # Errors
///
/// - `WebError::Internal("internal_error")` (500) — `state.tasks` 锁被毒化
#[tracing::instrument(name = "web::tasks_list", skip_all)]
pub async fn tasks_list(State(state): State<SharedState>) -> Result<Json<Vec<TaskInfo>>, WebError> {
    let tasks = read_state_or_json("tasks_list", || mutex_or("tasks_list", &state.tasks))?;
    let mut result: Vec<TaskInfo> = tasks.iter().map(task_to_info).collect();
    drop(tasks);
    result.sort_by_key(|b| std::cmp::Reverse(b.id));
    Ok(Json(result))
}

/// `POST /api/tasks/{id}/cancel` — 翻 `cancelling` 标记 + 触发 `CancelToken`。
///
/// 任务已结束（`finished.is_some()`）→ 409，避免前端 cancel 按钮无响应却显示 ok。
///
/// # Errors
///
/// - `WebError::NotFound` (404) — 任务 id 不存在或 `task.cancel` 不存在
/// - `WebError::TaskAlreadyFinished` (409) — 任务已终结（`code: 3006`）
/// - `WebError::Internal` (500) — 锁被毒化
#[tracing::instrument(name = "web::task_cancel", skip_all, fields(task_id = id))]
pub async fn task_cancel(
    Locale(locale): Locale,
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<String, WebError> {
    let cancel;
    {
        let mut tasks =
            read_state_or_json("task_cancel", || mutex_or("task_cancel", &state.tasks))?;
        let Some(task) = tasks.iter_mut().find(|t| t.id == id) else {
            return Err(WebError::NotFound(""));
        };
        // crawler 已退出，cancel 不会再产生状态变化 → 409（见函数文档）。
        if task.finished.is_some() {
            return Err(WebError::TaskAlreadyFinished);
        }
        let Some(c) = task.cancel.as_ref() else {
            return Err(WebError::NotFound(""));
        };
        // 先翻 cancelling（前端显示"正在取消..."），cancel() 同步触发 crawler 的
        // CancelToken；下一个 tick 发 Progress::Cancelled → drain 落 UserCancelled。
        task.cancelling = true;
        cancel = c.clone();
        drop(tasks);
    }
    cancel.cancel();
    // plain text body：前端不读，但保留 localized 文案便于 `curl -X POST` 调试。
    Ok(ts_for_locale(locale, "WebErrors.task_cancelled"))
}

/// `DELETE /api/tasks/{id}` — 从 `state.tasks` 移除一条任务记录, **不动磁盘**。
///
/// 与 `task_cancel` 不同：delete 是纯 metadata 清理，活跃任务也允许删 ——
/// 在飞的 crawler / drain 仍会 `apply_progress`（按 `id` 查找，找不到就 no-op），
/// 所以删完对 tasks.json 做一次 trim 是安全的。
/// 与 library delete 也不同：那边删磁盘文件，这边只删记录（在 /tasks 页删记录，
/// 在 /library 页删文件）。
///
/// # Errors
///
/// - `WebError::NotFound` (404) — 任务 id 不存在
/// - `WebError::Internal` (500) — 锁被毒化
#[tracing::instrument(name = "web::task_delete", skip_all, fields(task_id = id))]
pub async fn task_delete(
    Locale(locale): Locale,
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<String, WebError> {
    let mut tasks = read_state_or_json("task_delete", || mutex_or("task_delete", &state.tasks))?;
    let initial_len = tasks.len();
    tasks.retain(|t| t.id != id);
    if tasks.len() == initial_len {
        return Err(WebError::NotFound(""));
    }
    drop(tasks);
    if let Ok(tasks) = mutex_or("task_delete:save", &state.tasks) {
        let _ = crate::db::save_with_trim(&state.tasks_file, &tasks);
    }
    Ok(ts_for_locale(locale, "WebErrors.task_deleted"))
}

/// 单个下载任务的 per-task drain：既是唯一 mpsc consumer（没人能 race），
/// 也是状态更新者（lock `state.tasks` → 按 id `apply_progress`）。
///
/// 退出条件：`crawler_rx.recv()` 返回 `None`（发送端被 drop）。退出前若
/// `finished.is_none()` 标 `AppRestarted` 并 save 兜底 —— 不依赖中心 tick；
/// 与 GPUI `DownloadTask::drain` 的 `Disconnected` 分支同语义。
pub(super) fn spawn_task_drain(
    state: Arc<WebState>,
    task_id: u64,
    mut crawler_rx: mpsc::UnboundedReceiver<Progress>,
) {
    tokio::spawn(async move {
        while let Some(progress) = crawler_rx.recv().await {
            // poison 时丢弃该条 progress（drain 里没法返 500，也不该 panic）
            match state.tasks.lock() {
                Ok(mut tasks) => {
                    if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                        task.apply_progress(progress);
                    }
                }
                Err(e) => {
                    tracing::error!(
                        "spawn_task_drain {task_id}: tasks Mutex poisoned, drop progress: {e}"
                    );
                }
            }
        }

        // mpsc 断开（crawler 已退出）：若还没到 finished 态，补 AppRestarted + 落盘。
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
        if needs_save && let Ok(tasks) = mutex_or("spawn_task_drain:save", &state.tasks) {
            let _ = crate::db::save_with_trim(&state.tasks_file, &tasks);
        }
    });
}
