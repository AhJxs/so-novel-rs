//! 下载任务管理端点 + per-task drain 基础设施
//!
//! ## 任务存储: 单源 `Vec<DownloadTask>` (跟 GPUI 一致)
//!
//! 旧实现是双 store (`HashMap<u64, ActiveDownload>` + `Vec<DownloadTaskRecord>` + 一个
//! bridge task 来回同步), 反复出现 active 拿到 `(None, 0)` 把 history 修正过的元数据
//! 盖回去、或者 `Finished` 状态被 `Downloading` 覆盖之类的 bug。根因是同一份数据
//! 两边各持一份, 时序窗口不一致。
//!
//! 现在 web 跟 GPUI 一样只持一个 `Vec<DownloadTask>`, 所有字段 (持久化 + 运行期 +
//! rx/cancel) 都在这个 struct 上。
//!
//! ## Drain 时序保证
//!
//! 1. 任务先 push 进 `state.tasks` (可见性), 再 spawn drain, 再 spawn crawler
//!    —— `tasks_list` 在返回 SSE 之前已经能看到这个 id
//! 2. drain 收到 mpsc 断开 (crawler 退出) 时, 若 `finished.is_none()` 则标
//!    `AppRestarted` 并 save (与 GPUI `DownloadTask::drain` 同语义)
//! 3. 用户 cancel: `task.cancelling = true` 立即反映到 `tasks_list`;
//!    `Progress::Cancelled` 经 mpsc → drain → `apply_progress` 最终落 `UserCancelled`

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};

use crate::core::DownloadTask;
use crate::crawler::Progress;
use crate::models::FinishedReason;
use crate::utils::time::now_unix_secs;

use super::super::{TaskStatus, WebState};
use crate::utils::lock::mutex_or;
use crate::web::SharedState;
use crate::web::error::read_state_or_json;

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

/// 从 `book.epub` / `book(作者).txt` 等文件名里粗略抽书名 —— 只作 fallback:
/// 历史里 `book_meta` 缺失 (旧任务漏发过 `BookResolved`) 时, 至少让 UI 显示个名字。
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

/// `DownloadTask` → `TaskInfo`。
///
/// 关键点: 不再 merge 两个 store —— `state.tasks` 是唯一来源。
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
    // book_name 优先: book_meta (BookResolved 之后) > origin.book_name (请求里带的搜索书名)。
    // 两者都为空时回退到 finished output filename 派生 (仅 Finished 任务有值)。
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
/// - `(INTERNAL_SERVER_ERROR, ...)` — `state.tasks` 锁被毒化
#[tracing::instrument(name = "web::tasks_list", skip_all)]
pub async fn tasks_list(
    State(state): State<SharedState>,
) -> Result<Json<Vec<TaskInfo>>, (StatusCode, String)> {
    let tasks = read_state_or_json("tasks_list", || mutex_or("tasks_list", &state.tasks))?;
    let mut result: Vec<TaskInfo> = tasks.iter().map(task_to_info).collect();
    drop(tasks);
    // 按 id 降序 (最新任务在前)。
    result.sort_by_key(|b| std::cmp::Reverse(b.id));
    Ok(Json(result))
}

/// `POST /api/tasks/{id}/cancel` — 翻 `cancelling` 标记 + 触发 `CancelToken`。
///
/// 任务已结束 (任何 `finished.is_some()`) → 409 提示前端"无法取消已结束任务",
/// 避免 cancel 按钮无响应却显示 ok。
///
/// # Errors
///
/// - `(NOT_FOUND, ...)` — 任务 id 不存在或 `task.cancel` 不存在
/// - `(CONFLICT, "任务已结束,无法取消")` — 任务已终结
/// - `(INTERNAL_SERVER_ERROR, ...)` — 锁被毒化
#[tracing::instrument(name = "web::task_cancel", skip_all, fields(task_id = id))]
pub async fn task_cancel(
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<&'static str, (StatusCode, String)> {
    let cancel;
    {
        let mut tasks =
            read_state_or_json("task_cancel", || mutex_or("task_cancel", &state.tasks))?;
        let Some(task) = tasks.iter_mut().find(|t| t.id == id) else {
            return Err((StatusCode::NOT_FOUND, "任务未找到".to_string()));
        };
        // 任务已结束: cancel 不会触发任何 crawler 状态变化 (crawler 已退出)。
        // 返回 409 提示前端"无法取消已结束任务", 避免前端 cancel 按钮无响应却显示 ok。
        if task.finished.is_some() {
            return Err((StatusCode::CONFLICT, "任务已结束,无法取消".to_string()));
        }
        let Some(c) = task.cancel.as_ref() else {
            return Err((StatusCode::NOT_FOUND, "任务未找到".to_string()));
        };
        // 立即翻 cancelling 标记 (前端可显示"正在取消..."), cancel.cancel() 同步触发
        // crawler 内部的 CancelToken; crawler 下一次 progress tick 看到 cancel 时会发
        // Progress::Cancelled → drain → apply_progress 落 UserCancelled。
        task.cancelling = true;
        cancel = c.clone();
        drop(tasks);
    }
    cancel.cancel();
    Ok("已取消")
}

/// `DELETE /api/tasks/{id}` — 从 `state.tasks` 移除一条任务记录, **不动磁盘**。
///
/// 跟 `task_cancel` 的语义区别:
/// - `cancel`: 仅对 `Downloading` 任务有意义 (触发 crawler stop), 已结束任务是 409
/// - `delete`: 纯 metadata 清理。任何 `finished.is_some()` 的任务都能删, 活跃任务也
///   允许 —— 删后该任务的 in-flight crawler / drain 还在跑, 自然写入
///   `state.tasks` 的旧下标位置已经不存在, 但 `apply_progress` 是按 `id` 查找的,
///   find 会 no-op, drain 退出时 save 兜底空 vec 也写。简单粗暴地对
///   tasks.json 做一次 trim —— 这是用户主动清理历史, 不算脏数据。
///
/// 跟 library delete 的语义区别: library delete 删的是磁盘文件; 这里删的是
/// tasks.json 里的任务记录。两个端点分开是因为用途不同:
///   - 在 /tasks 页面删 → 删记录 (保留文件)
///   - 在 /library 页面删 → 删文件 (记录留着也无害)
///
/// # Errors
///
/// - `(NOT_FOUND, ...)` — 任务 id 不存在
/// - `(INTERNAL_SERVER_ERROR, ...)` — 锁被毒化
#[tracing::instrument(name = "web::task_delete", skip_all, fields(task_id = id))]
pub async fn task_delete(
    State(state): State<SharedState>,
    Path(id): Path<u64>,
) -> Result<&'static str, (StatusCode, String)> {
    let mut tasks = read_state_or_json("task_delete", || mutex_or("task_delete", &state.tasks))?;
    let initial_len = tasks.len();
    tasks.retain(|t| t.id != id);
    if tasks.len() == initial_len {
        return Err((StatusCode::NOT_FOUND, "任务未找到".to_string()));
    }
    drop(tasks);
    if let Ok(tasks) = mutex_or("task_delete:save", &state.tasks) {
        let _ = crate::db::save_with_trim(&state.tasks_file, &tasks);
    }
    Ok("已删除任务")
}

/// 单个下载任务的 per-task drain。
///
/// 三件事合一:
/// 1. 单一 mpsc consumer (`crawler_rx`) —— 没人能 race
/// 2. 状态更新者: lock `state.tasks` → 找对应 id → `task.apply_progress(ev)`
/// 3. broadcast producer: 把同一事件转发给 SSE subscribers
///
/// 退出条件: `crawler_rx.recv()` 返回 `None` (crawler 退出发送端被 drop)。
/// 退出前若 `finished.is_none()` 标 `AppRestarted` (对齐 GPUI `DownloadTask::drain`
/// 的 `TryRecvError::Disconnected` 分支), 并触发一次 `db::save_with_trim` 兜底
/// 落盘 —— 不依赖中心 tick, drain 退出后所有变更都已经被保存。
pub(super) fn spawn_task_drain(
    state: Arc<WebState>,
    task_id: u64,
    mut crawler_rx: mpsc::UnboundedReceiver<Progress>,
    sse_tx: broadcast::Sender<Progress>,
) {
    tokio::spawn(async move {
        while let Some(progress) = crawler_rx.recv().await {
            // 1. 锁 + 更新 in-memory task; poison 状态跳过 (drain 里没法返 500,
            //    也不应让 worker panic —— 标记 lost-progress 后继续 SSE 转发)
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
            // 2. 转发给 SSE subscribers (多 client 并发各自 lagging 互不干扰)
            let _ = sse_tx.send(progress);
        }

        // mpsc 断开: crawler 已退出。若任务还没走到 finished 态, 补 AppRestarted
        // + finished_at_unix, 然后 save。
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
            if let Ok(tasks) = mutex_or("spawn_task_drain:save", &state.tasks) {
                let _ = crate::db::save_with_trim(&state.tasks_file, &tasks);
            }
        }
    });
}
