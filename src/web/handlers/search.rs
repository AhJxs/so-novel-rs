//! 搜索 API（SSE 流式）。

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Sse;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::models::SearchResult;
use crate::models::Source;

use super::super::SharedState;
use crate::utils::lock::rw_read_or;

type BoxedSearchStream =
    std::pin::Pin<Box<dyn Stream<Item = Result<axum::response::sse::Event, Infallible>> + Send>>;

/// 在 SSE search handler 入口拿到 poisoned lock 时，把错误以 `result` event
/// (error 字段) + `done` 形式给前端，避免连接哑断。
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
    let (config, http, rules) = match (|| -> Result<_, (StatusCode, String)> {
        let cfg = rw_read_or("search:cfg", &state.config)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        let rules = rw_read_or("search:rules", &state.rules)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok((cfg.clone(), Arc::clone(&state.http), rules.clone()))
    })() {
        Ok(v) => v,
        Err((code, msg)) => return lock_failure_stream(code.as_u16(), &msg),
    };

    let sources: Vec<Source> = if let Some(id) = params.source_id {
        rules
            .into_iter()
            .filter(|r| r.id == id)
            .map(|r| Source::from(r, &config))
            .collect()
    } else {
        rules
            .into_iter()
            .filter(crate::models::rule::Rule::is_search_enabled)
            .map(|r| Source::from(r, &config))
            .collect()
    };

    let limit = params.limit.map(|v| v.max(0) as usize).filter(|v| *v > 0);

    let cf_bypass = if config.global.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(config.global.cf_bypass)
    };

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
