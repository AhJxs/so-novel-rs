//! 书籍详情 + 目录（TOC）API。

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use serde::Deserialize;

use crate::config::AppConfig;
use crate::crawler::{self, CancelToken};
use crate::models::Source;
use crate::models::{Book, Rule};
use crate::parser;

use super::super::SharedState;

#[derive(Deserialize)]
pub struct BookDetailParams {
    url: String,
    source_id: i32,
}

/// 从共享状态中提取配置和指定书源。
fn extract_config_and_rule(
    state: &SharedState,
    source_id: i32,
) -> Result<(AppConfig, Rule), (StatusCode, String)> {
    let cfg = state.config.read().unwrap();
    let rules = state.rules.read().unwrap();
    let rule = rules
        .iter()
        .find(|r| r.id == source_id)
        .cloned()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "书源未找到".to_string()))?;
    Ok((cfg.clone(), rule))
}

pub async fn book_detail(
    State(state): State<SharedState>,
    Query(params): Query<BookDetailParams>,
) -> Result<Json<Book>, (StatusCode, String)> {
    let (config, rule) = extract_config_and_rule(&state, params.source_id)?;

    let source = Source::from(rule, &config);
    let client = state.http.for_rule(&source.rule);

    let cf = (!config.cf_bypass.trim().is_empty()).then_some(config.cf_bypass.as_str());
    let qc = (!config.qidian_cookie.trim().is_empty()).then_some(config.qidian_cookie.as_str());

    let book = parser::parse_book_detail(&client, &source.rule, &params.url, cf, qc)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;

    Ok(Json(book))
}

pub async fn book_toc(
    State(state): State<SharedState>,
    Query(params): Query<BookDetailParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (config, rule) = extract_config_and_rule(&state, params.source_id)?;

    let source = Source::from(rule, &config);
    let client = state.http.for_rule(&source.rule);
    let cancel = CancelToken::new();

    let (book, chapters) = crawler::resolve_book(&config, &client, &source, &params.url, &cancel)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;

    Ok(Json(serde_json::json!({
        "book": book,
        "chapters": chapters,
    })))
}
