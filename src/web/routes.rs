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
/// 解析环境变量 `SO_NOVEL_CORS_ORIGINS`（逗号分隔的 origin 列表，例如
/// `https://reader.example.com,https://admin.example.com`）。
/// 未设置 / 解析失败时回退到 loopback（`http://localhost:<port>` / `http://127.0.0.1:<port>`，
/// 端口取 `SO_NOVEL_WEB_PORT` 或 8080 默认）。
///
/// 设计动机：原 `CorsLayer::permissive()` 允许**任意** origin 跨域读 API
/// —— 配合 `GET /api/settings` 暴露 `qidian_cookie` 明文，是个真实密钥泄漏路径。
/// 现在 CORS 只放已知白名单 + loopback，跨域请求会被浏览器拦截。
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
    // 总是包含 loopback（覆盖未配环境变量场景）。
    for host in ["localhost", "127.0.0.1"] {
        let url = format!("http://{host}:{port}");
        if let Ok(hv) = HeaderValue::from_str(&url) {
            if !origins.contains(&hv) {
                origins.push(hv);
            }
        }
    }
    origins
}
/// 构建 axum Router。
/// SPA 前端由 rust-embed 编译期嵌入，通过 `spa_handler` fallback 提供。
pub fn build_router(state: SharedState, session_store: SessionStore<SessionNullPool>) -> Router {
    let api = Router::new()
        // 搜索
        .route("/search", get(handlers::search::search))
        .route("/book/detail", get(handlers::book::book_detail))
        .route("/book/toc", get(handlers::book::book_toc))
        // 下载 + 任务
        .route("/download", post(handlers::download::download))
        .route("/tasks", get(handlers::download::tasks_list))
        .route("/tasks/{id}/cancel", post(handlers::download::task_cancel))
        .route("/tasks/{id}", delete(handlers::download::task_delete))
        // 书库 + 文件
        .route("/library", get(handlers::library::library_list))
        .route("/library/{filename}", delete(handlers::library::library_delete))
        .route("/files/{filename}", get(handlers::library::file_download))
        // 书源
        .route("/sources", get(handlers::sources::sources_list))
        .route("/sources/{id}/toggle", post(handlers::sources::source_toggle))
        .route("/sources/{id}/test", post(handlers::sources::source_test))
        // 设置
        .route("/settings", get(handlers::settings::settings_get))
        .route("/settings", put(handlers::settings::settings_put))
        // 健康检查
        .route("/health", get(handlers::health::health))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback(spa_handler)
        .layer(SessionLayer::new(session_store))
        .layer(tower_http::cors::CorsLayer::new()
            // 默认仅放行 loopback（同源 / 本机反代）。
            // 远程部署时通过环境变量 `SO_NOVEL_CORS_ORIGINS` 配置（逗号分隔），
            // 例如 `SO_NOVEL_CORS_ORIGINS=https://reader.example.com`。
            // 不放 `*`：避免任意第三方网页跨域读 /api/settings 等敏感响应。
            .allow_origin(loopback_origins()))
}
