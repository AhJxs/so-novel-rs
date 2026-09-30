//! axum Router 组装。

use axum::Router;
use axum::http::HeaderValue;
use axum::routing::{delete, get, post, put};
use axum_session::{SessionLayer, SessionNullPool, SessionStore};

use super::SharedState;
use super::spa_handler;
use crate::web::handlers;

/// CORS origin 白名单。
///
/// 白名单来自环境变量 `SO_NOVEL_CORS_ORIGINS`（逗号分隔），未设则只含 loopback。
/// **不放 `*`**：`GET /api/settings` 会暴露 `qidian_cookie`，任意 origin 跨域可读
/// 就是一条真实的密钥泄漏路径。
fn loopback_origins() -> Vec<HeaderValue> {
    use std::str::FromStr;
    let port = std::env::var("SO_NOVEL_WEB_PORT")
        .ok()
        .and_then(|s| u16::from_str(&s).ok())
        .unwrap_or(8080);

    let mut origins: Vec<HeaderValue> = match std::env::var("SO_NOVEL_CORS_ORIGINS") {
        Ok(raw) if !raw.trim().is_empty() => raw
            .split(',')
            .filter_map(|s| HeaderValue::from_str(s.trim()).ok())
            .collect(),
        _ => Vec::new(),
    };
    for host in ["localhost", "127.0.0.1"] {
        let url = format!("http://{host}:{port}");
        if let Ok(hv) = HeaderValue::from_str(&url)
            && !origins.contains(&hv)
        {
            origins.push(hv);
        }
    }
    origins
}
/// 构建 axum Router。
/// SPA 前端由 rust-embed 编译期嵌入，通过 `spa_handler` fallback 提供。
pub fn build_router(state: SharedState, session_store: SessionStore<SessionNullPool>) -> Router {
    let api = Router::new()
        .route("/search", post(handlers::search::search_create))
        .route("/search/{task_id}", get(handlers::search::search_status))
        .route("/search/{task_id}", delete(handlers::search::search_delete))
        .route("/book/detail", get(handlers::book::book_detail))
        .route("/book/toc", get(handlers::book::book_toc))
        .route("/download", post(handlers::download::download))
        .route("/tasks", get(handlers::tasks::tasks_list))
        .route("/tasks/{id}/cancel", post(handlers::tasks::task_cancel))
        .route("/tasks/{id}", delete(handlers::tasks::task_delete))
        .route("/library", get(handlers::library::library_list))
        .route(
            "/library/{filename}",
            delete(handlers::library::library_delete),
        )
        .route("/files/{filename}", get(handlers::library::file_download))
        .route("/sources", get(handlers::sources::sources_list))
        .route(
            "/sources/{id}/toggle",
            post(handlers::sources::source_toggle),
        )
        .route("/sources/{id}/test", post(handlers::sources::source_test))
        .route("/settings", get(handlers::settings::settings_get))
        .route("/settings", put(handlers::settings::settings_put))
        .route("/health", get(handlers::health::health))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback(spa_handler)
        .layer(SessionLayer::new(session_store))
        .layer(tower_http::cors::CorsLayer::new()
            // 默认仅放行 loopback（同源 / 本机反代）；远程部署用
            // `SO_NOVEL_CORS_ORIGINS`（逗号分隔）显式加白名单。
            .allow_origin(loopback_origins()))
}
