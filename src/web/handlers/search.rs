//! 搜索 API（SSE 流式）。

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::Sse;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::models::SearchResult;
use crate::rules::Source;

use super::super::SharedState;

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
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let keyword = params.keyword.trim().to_string();
    let (config, http, rules) = {
        let cfg = state.config.read().unwrap();
        let rules = state.rules.read().unwrap();
        (cfg.clone(), Arc::clone(&state.http), rules.clone())
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
            .filter(|r| r.is_search_enabled())
            .map(|r| Source::from(r, &config))
            .collect()
    };

    let limit = params.limit.map(|v| v.max(0) as usize).filter(|v| *v > 0);

    let cf_bypass = if config.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(config.cf_bypass.clone())
    };

    let (tx, rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::crawler::search::SourceSearchOutcome>();

    let http_clone = Arc::clone(&http);
    let cfg_clone = config.clone();
    tokio::spawn(async move {
        crate::crawler::search::search_streaming(
            &cfg_clone, http_clone, sources, keyword, limit, cf_bypass, tx,
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

    Sse::new(stream)
}
