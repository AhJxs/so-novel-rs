//! axum Router 组装。

use axum::Router;
use axum::routing::{delete, get, post, put};
use axum_session::{SessionLayer, SessionNullPool, SessionStore};

use super::SharedState;
use super::spa_handler;
use crate::web::handlers;

/// 构建 axum Router。
/// SPA 前端由 rust-embed 编译期嵌入，通过 `spa_handler` fallback 提供。
pub fn build_router(state: SharedState, session_store: SessionStore<SessionNullPool>) -> Router {
    let api = Router::new()
        // 搜索
        .route("/search", get(handlers::search::search))
        // 书籍
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
        .route("/sources", get(handlers::misc::sources_list))
        .route("/sources/{id}/toggle", post(handlers::misc::source_toggle))
        .route("/sources/{id}/test", post(handlers::misc::source_test))
        // 设置
        .route("/settings", get(handlers::misc::settings_get))
        .route("/settings", put(handlers::misc::settings_put))
        // 健康检查
        .route("/health", get(handlers::misc::health))
        .with_state(state);

    Router::new()
        .nest("/api", api)
        .fallback(spa_handler)
        .layer(SessionLayer::new(session_store))
        .layer(tower_http::cors::CorsLayer::permissive())
}
