//! axum Router 组装。

use axum::Router;
use axum::routing::{delete, get, post, put};
use axum_session::{SessionLayer, SessionNullPool, SessionStore};

use super::SharedState;
use crate::web::handlers;

/// 构建 axum Router。
pub fn build_router(state: SharedState, session_store: SessionStore<SessionNullPool>) -> Router {
    Router::new()
        // 前端
        .route("/", get(handlers::misc::index_page))
        .route("/favicon.ico", get(handlers::misc::favicon))
        .route("/logo.png", get(handlers::misc::logo))
        // 搜索
        .route("/api/search", get(handlers::search::search))
        // 书籍
        .route("/api/book/detail", get(handlers::book::book_detail))
        .route("/api/book/toc", get(handlers::book::book_toc))
        // 下载 + 任务
        .route("/api/download", post(handlers::download::download))
        .route("/api/tasks", get(handlers::download::tasks_list))
        .route("/api/tasks/{id}/cancel", post(handlers::download::task_cancel))
        // 书库 + 文件
        .route("/api/library", get(handlers::library::library_list))
        .route("/api/library/{filename}", delete(handlers::library::library_delete))
        .route("/api/files/{filename}", get(handlers::library::file_download))
        // 书源
        .route("/api/sources", get(handlers::misc::sources_list))
        .route("/api/sources/{id}/toggle", post(handlers::misc::source_toggle))
        .route("/api/sources/{id}/test", post(handlers::misc::source_test))
        // 设置
        .route("/api/settings", get(handlers::misc::settings_get))
        .route("/api/settings", put(handlers::misc::settings_put))
        // 认证
        .route("/api/auth", post(handlers::misc::auth_verify))
        .route("/api/auth/status", get(handlers::misc::auth_status))
        .route("/api/access-code", post(handlers::misc::set_access_code))
        // 健康检查
        .route("/api/health", get(handlers::misc::health))
        .with_state(state)
        .layer(SessionLayer::new(session_store))
        .layer(tower_http::cors::CorsLayer::permissive())
}
