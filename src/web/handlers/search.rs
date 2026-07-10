//! 搜索 API（SSE 流式）。

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::Sse;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::core::{config_helpers, search as core_search};
use crate::models::SearchResult;

use super::super::SharedState;
use super::super::error::read_state_or_sse;
use crate::utils::lock::rw_read_or;

type BoxedSearchStream =
    std::pin::Pin<Box<dyn Stream<Item = Result<axum::response::sse::Event, Infallible>> + Send>>;

/// 在 SSE search handler 入口拿到 poisoned lock 时，把错误以 `result` event
/// (error 字段) + `done` 形式给前端，避免连接哑断。
///
/// Phase 3.8：本函数仍保留原签名，作为 [`read_state_or_sse`] 的 `make_stream` 回调；
/// 真正消除的是入口处的 match-IIFE 模板。
fn lock_failure_stream(status: u16, msg: &str) -> Sse<BoxedSearchStream> {
    let reason = format!("[{status}] {msg}");
    let stream = async_stream::stream! {
        let event = SearchEvent {
            source_id: 0,
            source_name: String::new(),
            results: vec![],
            error: Some(reason),
        };
        let data = serde_json::to_string(&event).unwrap_or_default();
        yield Ok(axum::response::sse::Event::default()
            .event("result")
            .data(data));
        let done = serde_json::to_string(&SearchDoneEvent { total: 0 }).unwrap_or_default();
        yield Ok(axum::response::sse::Event::default()
            .event("done")
            .data(done));
    };
    Sse::new(Box::pin(stream))
}

#[derive(Deserialize)]
pub struct SearchParams {
    pub keyword: String,
    pub source_id: Option<i32>,
    pub limit: Option<i32>,
}

#[derive(Serialize)]
struct SearchEvent {
    source_id: i32,
    source_name: String,
    results: Vec<SearchResult>,
    error: Option<String>,
}

#[derive(Serialize)]
struct SearchDoneEvent {
    total: usize,
}

pub async fn search(
    State(state): State<SharedState>,
    Query(params): Query<SearchParams>,
) -> Sse<BoxedSearchStream> {
    let keyword = params.keyword.trim().to_string();
    // SSE handler 直接返 `Sse<...>` 而非 `Result<_, _>`, 不能用 `?` 早返;
    // 这里用 `match ... { Err(sse) => return sse }` 模式 (跟原 match-IIFE 等价).
    let config = match read_state_or_sse("search:cfg", lock_failure_stream, || {
        Ok(rw_read_or("search:cfg", &state.config)?.clone())
    }) {
        Ok(v) => v,
        Err(sse) => return sse,
    };
    let rules = match read_state_or_sse("search:rules", lock_failure_stream, || {
        Ok(rw_read_or("search:rules", &state.rules)?.clone())
    }) {
        Ok(v) => v,
        Err(sse) => return sse,
    };
    let http = Arc::clone(&state.http);

    let sources = core_search::select_sources(&rules, &config, params.source_id);

    // 注意：web 这里只取 query param 的 limit，**不**自动 fallback 到
    // cfg.source.search_limit —— 这是 web API 的契约（CLI / 桌面会走 fallback，
    // 见 `core_search::effective_limit`）。保留原行为以免改动前端契约。
    let limit = params.limit.map(|v| v.max(0) as usize).filter(|v| *v > 0);

    let cf_bypass = config_helpers::cf_bypass(&config);

    let (tx, rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::crawler::search::SourceSearchOutcome>();

    let http_clone = Arc::clone(&http);
    tokio::spawn(async move {
        crate::crawler::search::search_streaming(
            http_clone, sources, keyword, limit, cf_bypass, tx,
        )
        .await;
    });

    let stream = async_stream::stream! {
        let mut rx = rx;
        let mut total = 0usize;
        while let Some(outcome) = rx.recv().await {
            total += 1;
            let event = match outcome.result {
                Ok(list) => SearchEvent {
                    source_id: outcome.source_id,
                    source_name: outcome.source_name,
                    results: list,
                    error: None,
                },
                Err(e) => SearchEvent {
                    source_id: outcome.source_id,
                    source_name: outcome.source_name,
                    results: vec![],
                    error: Some(format!("{e:#}")),
                },
            };
            let data = serde_json::to_string(&event).unwrap_or_default();
            yield Ok(axum::response::sse::Event::default()
                .event("result")
                .data(data));
        }
        let done = serde_json::to_string(&SearchDoneEvent { total }).unwrap_or_default();
        yield Ok(axum::response::sse::Event::default()
            .event("done")
            .data(done));
    };

    Sse::new(Box::pin(stream))
}
