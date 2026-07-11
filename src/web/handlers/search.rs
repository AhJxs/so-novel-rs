//! 搜索 API（SSE 流式）。

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::Sse;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::core::{config_helpers, search as core_search};
use crate::i18n::ts_for_locale;
use crate::models::SearchResult;
use crate::web::error_code::ErrorCode;

use super::super::SharedState;
use super::super::error::read_state_or_sse;
use crate::utils::lock::rw_read_or;
use crate::web::locale::Locale;

type BoxedSearchStream =
    std::pin::Pin<Box<dyn Stream<Item = Result<axum::response::sse::Event, Infallible>> + Send>>;

/// 在 SSE search handler 入口拿到 poisoned lock 时，把错误以 `result` event
/// (error 字段) + `done` 形式给前端，避免连接哑断。
///
/// `error` 字段是按请求 locale 翻译的 [`ErrorCode::Internal`] 文案
/// （`WebErrors.internal` —— "Internal server error" / "内部错误" / "內部錯誤"）。
/// 原始 cause 字符串仅进 `tracing::warn!`，不外泄。
fn lock_failure_stream(status: u16, msg: &str, locale: &str) -> Sse<BoxedSearchStream> {
    let reason = ts_for_locale(locale, ErrorCode::Internal.key());
    tracing::warn!(status, cause = msg, "search SSE state read failed");
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

/// 把 [`crate::parser::SearchError`] 映射到稳定的 [`ErrorCode`]。
///
/// 与 [`crate::web::error::WebError::code`] 的 `Self::Search(_)` 分支同语义 ——
/// 抽取出来供 SSE 路径用（不能直接构造 `WebError` + 拿 message，会走 `WebError`
/// 渲染路径，跟 SSE event schema 不匹配；这里直接拿 key 翻译）。
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

pub async fn search(
    Locale(locale): Locale,
    State(state): State<SharedState>,
    Query(params): Query<SearchParams>,
) -> Sse<BoxedSearchStream> {
    let keyword = params.keyword.trim().to_string();
    // SSE handler 直接返 `Sse<...>` 而非 `Result<_, _>`, 不能用 `?` 早返;
    // 这里用 `match ... { Err(sse) => return sse }` 模式 (跟原 match-IIFE 等价).
    let config = match read_state_or_sse("search:cfg", locale, lock_failure_stream, || {
        Ok(rw_read_or("search:cfg", &state.config)?.clone())
    }) {
        Ok(v) => v,
        Err(sse) => return sse,
    };
    let rules = match read_state_or_sse("search:rules", locale, lock_failure_stream, || {
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
                Err(e) => {
                    // per-source 错误：原 `format!("{e:#}")` 泄漏内部 cause 给前端
                    // （"HTTP error: 502 Bad Gateway: upstream connect error..."）。
                    // 改成 ErrorCode → 按 locale 翻译成稳定文案，cause 进日志。
                    let code = search_err_code(&e);
                    tracing::warn!(
                        source_id = outcome.source_id,
                        cause = %format!("{e:#}"),
                        key = code.key(),
                        "search source error"
                    );
                    SearchEvent {
                        source_id: outcome.source_id,
                        source_name: outcome.source_name,
                        results: vec![],
                        error: Some(ts_for_locale(locale, code.key())),
                    }
                }
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
