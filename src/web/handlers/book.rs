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
use super::super::error::WebError;
use crate::utils::lock::rw_read_or;

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
    let cfg = rw_read_or("book:cfg", &state.config)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let rule = {
        let rules = rw_read_or("book:rules", &state.rules)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        rules
            .iter()
            .find(|r| r.id == source_id)
            .cloned()
            .ok_or_else(|| (StatusCode::NOT_FOUND, "书源未找到".to_string()))?
    };
    Ok((cfg.clone(), rule))
}

pub async fn book_detail(
    State(state): State<SharedState>,
    Query(params): Query<BookDetailParams>,
) -> Result<Json<Book>, WebError> {
    let (config, rule) = extract_config_and_rule(&state, params.source_id)
        .map_err(|_| WebError::NotFound("书源未找到"))?;

    let source = Source::from(rule, &config);
    let client = state.http.for_rule(&source.rule);

    let cf =
        (!config.global.cf_bypass.trim().is_empty()).then_some(config.global.cf_bypass.as_str());
    let qc = (!config.cookie.qidian_cookie.trim().is_empty())
        .then_some(config.cookie.qidian_cookie.as_str());

    let book = parser::parse_book_detail(&client, &source.rule, &params.url, cf, qc).await?;

    Ok(Json(book))
}

pub async fn book_toc(
    State(state): State<SharedState>,
    Query(params): Query<BookDetailParams>,
) -> Result<Json<serde_json::Value>, WebError> {
    let (config, rule) = extract_config_and_rule(&state, params.source_id)
        .map_err(|_| WebError::NotFound("书源未找到"))?;

    let source = Source::from(rule, &config);
    let client = state.http.for_rule(&source.rule);
    let cancel = CancelToken::new();

    let (book, chapters) =
        crawler::resolve_book(&config, &client, &source, &params.url, &cancel).await?;

    Ok(Json(serde_json::json!({
        "book": book,
        "chapters": chapters,
    })))
}
