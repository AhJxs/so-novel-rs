//! Web 服务模块：axum HTTP 服务器 + 单页前端。
//!
//! 提供 REST API + 任务轮询（搜索/下载均为「建任务 → 轮询状态」），
//! 让用户通过浏览器搜索、下载小说。
//! 与 CLI 模式同构，直接调用底层 crawler / parser / export 函数。

mod error;
mod error_code;
mod handlers;
pub mod locale;
mod routes;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use axum_session::{SessionConfig, SessionNullPool, SessionStore};
use serde::Serialize;

use crate::config::AppConfig;
use crate::core::DownloadTask;
use crate::db::SourcesConfig;
use crate::http::HttpClients;
use crate::models::Rule;

#[cfg(feature = "web")]
use axum::{
    body::Body,
    extract::Request,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
#[cfg(feature = "web")]
use rust_embed::RustEmbed;

/// 编译期嵌入 `web-ui/apps/web/dist/` 下所有静态文件。
#[cfg(feature = "web")]
#[derive(RustEmbed)]
#[folder = "web-ui/apps/web/dist/"]
pub struct Assets;

#[cfg(feature = "web")]
pub async fn spa_handler(req: Request<Body>) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref().to_owned())],
                file.data,
            )
                .into_response()
        }
        None => match Assets::get("index.html") {
            Some(f) => Html(f.data).into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        },
    }
}

/// `WebState::new` / `web::run` 的额外初始化参数（避免参数过多）。
pub struct WebInitParams {
    pub sources_config: SourcesConfig,
    pub sources_config_path: PathBuf,
    /// 启动时由调用方从 `tasks.json` 反序列化后传入；空 vec 表示无历史。
    pub tasks: Vec<DownloadTask>,
    pub tasks_file: PathBuf,
    pub next_task_id: u64,
}

/// Web 服务共享状态。
///
/// 任务存储是**单源** `Vec<DownloadTask>`（跟 GPUI 同型）：早先的
/// 「活跃 map + 历史 vec + bridge task」双 store 会因时序窗口不一致而互相覆盖，
/// 才改成持久化字段与运行期字段（`rx` / `cancel` / `cancelling`）同在一个 struct。
pub struct WebState {
    pub config: RwLock<AppConfig>,
    pub http: Arc<HttpClients>,
    pub rules: RwLock<Vec<Rule>>,
    pub download_path: PathBuf,
    /// **单源真相**：每个任务（活跃 + 已结束）的所有状态都在这里。
    pub tasks: Mutex<Vec<DownloadTask>>,
    pub next_task_id: Mutex<u64>,
    /// 内存态搜索任务注册表（搜索是瞬态，不落盘）。
    pub search_tasks: Mutex<HashMap<u64, SearchTask>>,
    /// 搜索任务 id 计数器（与 `next_task_id` 独立）。
    pub next_search_id: Mutex<u64>,
    /// 访问码（仅存内存，启动时为空，用户通过 Web UI 设置）。
    pub access_code: Mutex<String>,
    /// 书源配置（禁用列表等）；toggle 时必须同步更新并持久化。
    pub sources_config: RwLock<SourcesConfig>,
    /// `sources_config.json` 磁盘路径。
    pub sources_config_path: PathBuf,
    /// `tasks.json` 磁盘路径。
    pub tasks_file: PathBuf,
}

/// 任务状态（API 返回用，与 `DownloadTask::finished` 1:1 映射）。
///
/// 只在序列化层给前端用 —— 后端内部统一用 `DownloadTask::finished`，避免字符串语义。
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TaskStatus {
    Downloading,
    Finished,
    Failed,
    Cancelled,
}

/// 搜索任务状态（API 返回用）。搜索是内存态，进程重启即失。
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SearchStatus {
    Running,
    Done,
}

/// 单源搜索失败信息（`GET /api/search/{id}` 的 `source_errors` 项）。
#[derive(Clone, Serialize)]
pub struct SourceSearchError {
    pub source_id: i32,
    pub source_name: String,
    /// 按请求 locale 翻译的错误文案（不泄漏内部 cause）。
    pub error: String,
}

/// 内存态搜索任务（**不持久化**）。
pub struct SearchTask {
    pub id: u64,
    pub keyword: String,
    pub created_at_unix: i64,
    pub status: SearchStatus,
    pub sources_total: usize,
    pub sources_done: usize,
    pub results: Vec<crate::models::SearchResult>,
    pub source_errors: Vec<SourceSearchError>,
}

/// 所有 handler 共享的状态类型别名。
pub type SharedState = Arc<WebState>;

impl WebState {
    pub fn new(
        config: AppConfig,
        http: Arc<HttpClients>,
        rules: Vec<Rule>,
        params: WebInitParams,
    ) -> Self {
        let download_path = PathBuf::from(&config.download.download_path);
        Self {
            config: RwLock::new(config),
            http,
            rules: RwLock::new(rules),
            download_path,
            tasks: Mutex::new(params.tasks),
            next_task_id: Mutex::new(params.next_task_id),
            search_tasks: Mutex::new(HashMap::new()),
            next_search_id: Mutex::new(1),
            access_code: Mutex::new(String::new()),
            sources_config: RwLock::new(params.sources_config),
            sources_config_path: params.sources_config_path,
            tasks_file: params.tasks_file,
        }
    }
}

/// 启动 Web 服务器。阻塞当前线程直到进程退出。
pub fn run(
    config: AppConfig,
    http: Arc<HttpClients>,
    rules: Vec<Rule>,
    params: WebInitParams,
    host: String,
    port: u16,
) -> Result<()> {
    let state: SharedState = Arc::new(WebState::new(config, http, rules, params));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("so-novel-web")
        .build()
        .context("构造 web tokio runtime 失败")?;

    rt.block_on(async move {
        let session_config = SessionConfig::default();
        let session_store = match SessionStore::<SessionNullPool>::new(None, session_config).await {
            Ok(s) => s,
            Err(e) => {
                return Err(anyhow::anyhow!("构造 SessionStore 失败: {e}"));
            }
        };
        let router = routes::build_router(state, session_store);
        let addr = format!("{host}:{port}");
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .with_context(|| format!("绑定 {addr} 失败"))?;
        tracing::info!("Web 服务已启动: http://{addr}");
        axum::serve(listener, router)
            .await
            .context("axum serve 失败")
    })
}
