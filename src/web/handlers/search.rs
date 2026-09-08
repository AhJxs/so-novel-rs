//! 搜索 API（任务轮询模型）。
//!
//! `POST /api/search` 创建内存态搜索任务并立即返回 `task_id`；
//! `GET /api/search/{task_id}` 轮询当前累计状态；`DELETE` 显式清理。
//! 替代旧 SSE 流式实现。crawler 复用 `search_streaming` 的 mpsc 通道，仅消费端改为累计进 task。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::core::{config_helpers, search as core_search};
use crate::i18n::ts_for_locale;
use crate::models::SearchResult;
use crate::utils::lock::{mutex_or, rw_read_or};
use crate::utils::time::now_unix_secs;
use crate::web::error::WebError;
use crate::web::error::read_state_or_json;
use crate::web::error_code::ErrorCode;
use crate::web::locale::Locale;
use crate::web::{SearchStatus, SearchTask, SharedState, SourceSearchError};

/// 搜索任务 TTL（秒）：超时后 `POST /api/search` 会 sweep 掉。
/// `now_unix_secs()` 返回 i64，这里同型避免每次比较转类型。
const SEARCH_TTL_SECS: i64 = 600;

/// `POST /api/search` 请求体。
#[derive(Deserialize)]
pub struct SearchCreateRequest {
    pub keyword: String,
    pub source_id: Option<i32>,
    pub limit: Option<i32>,
}

/// `POST /api/search` 响应体。
#[derive(Serialize)]
pub struct SearchCreateResponse {
    pub task_id: u64,
}

/// 把 [`crate::parser::SearchError`] 映射到稳定的 [`ErrorCode`]（沿用旧实现）。
const fn search_err_code(e: &crate::parser::SearchError) -> ErrorCode {
    use crate::parser::SearchError;
    use ErrorCode as C;
    match e {
        SearchError::SearchDisabled => C::SearchDisabled,
        SearchError::SourceDisabled => C::SourceDisabled,
        SearchError::Http(_) => C::SearchHttp,
        SearchError::Cloudflare(_) => C::SearchCloudflare,
        SearchError::Parse(_) | SearchError::Selector(_) => C::SearchParse,
    }
}

/// `POST /api/search` — 创建搜索任务。
///
/// 校验 keyword → sweep 过期任务 → mint id → 插入 `state.search_tasks` →
/// spawn crawler（`search_streaming` 持有 tx，消费端循环累计进 task）→ 立即返回 `task_id`。
pub async fn search_create(
    Locale(locale): Locale,
    State(state): State<SharedState>,
    Json(req): Json<SearchCreateRequest>,
) -> Result<(StatusCode, Json<SearchCreateResponse>), WebError> {
    let keyword = req.keyword.trim().to_string();
    if keyword.is_empty() {
        return Err(WebError::BadRequest("search_keyword_empty"));
    }
    let config = read_state_or_json("search:cfg", || {
        Ok(rw_read_or("search:cfg", &state.config)?.clone())
    })?;
    let rules = read_state_or_json("search:rules", || {
        Ok(rw_read_or("search:rules", &state.rules)?.clone())
    })?;
    let http = Arc::clone(&state.http);

    let sources = core_search::select_sources(&rules, &config, req.source_id);
    let limit = req.limit.map(|v| v.max(0) as usize).filter(|v| *v > 0);
    let cf_bypass = config_helpers::cf_bypass(&config);

    // sweep 过期 + mint id + 插入
    let task_id = {
        let mut tasks = mutex_or("search:sweep", &state.search_tasks)?;
        let now = now_unix_secs();
        tasks.retain(|_, t| now.saturating_sub(t.created_at_unix) < SEARCH_TTL_SECS);
        let mut next = mutex_or("search:next_id", &state.next_search_id)?;
        let id = *next;
        *next += 1;
        drop(next);
        tasks.insert(
            id,
            SearchTask {
                id,
                keyword: keyword.clone(),
                created_at_unix: now,
                status: SearchStatus::Running,
                sources_total: sources.len(),
                sources_done: 0,
                results: Vec::new(),
                source_errors: Vec::new(),
            },
        );
        id
    };

    // spawn：crawler 独立子任务持有 tx；外层循环消费 rx 累计进 task。
    let (tx, rx) = mpsc::unbounded_channel::<crate::crawler::search::SourceSearchOutcome>();
    let http_for_crawler = Arc::clone(&http);
    let state_for_spawn = Arc::clone(&state);
    tokio::spawn(async move {
        tokio::spawn(async move {
            crate::crawler::search::search_streaming(
                http_for_crawler,
                sources,
                keyword,
                limit,
                cf_bypass,
                tx,
            )
            .await;
        });
        let mut rx = rx;
        while let Some(outcome) = rx.recv().await {
            let err_msg = match &outcome.result {
                Ok(_) => None,
                Err(e) => Some(ts_for_locale(locale, search_err_code(e).key())),
            };
            if let Ok(mut tasks) = state_for_spawn.search_tasks.lock() {
                if let Some(task) = tasks.get_mut(&task_id) {
                    task.sources_done += 1;
                    if let Ok(list) = &outcome.result {
                        task.results.extend(list.iter().cloned());
                    }
                    if let Some(msg) = err_msg {
                        task.source_errors.push(SourceSearchError {
                            source_id: outcome.source_id,
                            source_name: outcome.source_name.clone(),
                            error: msg,
                        });
                    }
                    if task.sources_done >= task.sources_total {
                        task.status = SearchStatus::Done;
                    }
                }
            }
        }
        // crawler 退出（tx drop）→ rx 关闭 → 0 源场景兜底标 done。
        if let Ok(mut tasks) = state_for_spawn.search_tasks.lock() {
            if let Some(task) = tasks.get_mut(&task_id)
                && task.status == SearchStatus::Running
                && task.sources_total == 0
            {
                task.status = SearchStatus::Done;
            }
        }
    });

    Ok((StatusCode::CREATED, Json(SearchCreateResponse { task_id })))
}

/// `GET /api/search/{task_id}` 响应体（每轮轮询返回当前累计）。
#[derive(Serialize)]
pub struct SearchStatusResponse {
    pub status: SearchStatus,
    pub total_sources: usize,
    pub done_sources: usize,
    pub results: Vec<SearchResult>,
    pub source_errors: Vec<SourceSearchError>,
}

/// `GET /api/search/{task_id}` — 轮询当前累计状态。
pub async fn search_status(
    State(state): State<SharedState>,
    Path(task_id): Path<u64>,
) -> Result<Json<SearchStatusResponse>, WebError> {
    let tasks =
        read_state_or_json("search:status", || mutex_or("search:status", &state.search_tasks))?;
    let Some(task) = tasks.get(&task_id) else {
        return Err(WebError::NotFound("search_task"));
    };
    Ok(Json(SearchStatusResponse {
        status: task.status,
        total_sources: task.sources_total,
        done_sources: task.sources_done,
        results: task.results.clone(),
        source_errors: task.source_errors.clone(),
    }))
}

/// `DELETE /api/search/{task_id}` — 丢弃搜索任务（幂等，不存在也 204）。
pub async fn search_delete(
    State(state): State<SharedState>,
    Path(task_id): Path<u64>,
) -> Result<StatusCode, WebError> {
    mutex_or("search:delete", &state.search_tasks)?.remove(&task_id);
    Ok(StatusCode::NO_CONTENT)
}
