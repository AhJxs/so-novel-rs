//! Web 服务模块：axum HTTP 服务器 + 单页前端。
//!
//! 提供 REST API 和 SSE 推送，让用户通过浏览器搜索、下载小说。
//! 与 CLI 模式同构，直接调用底层 crawler / parser / export 函数。

mod handlers;
mod routes;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use axum_session::{SessionConfig, SessionNullPool, SessionStore};
use serde::Serialize;
use tokio::sync::broadcast;

use crate::config::AppConfig;
use crate::crawler::{CancelToken, Progress};
use crate::db::Db;
use crate::http::HttpClients;
use crate::models::Rule;

/// Web 服务共享状态。
pub struct WebState {
    pub config: RwLock<AppConfig>,
    pub http: Arc<HttpClients>,
    pub rules: RwLock<Vec<Rule>>,
    pub db: Mutex<Db>,
    pub download_path: PathBuf,
    /// 活跃的下载任务：task_id → (CancelToken, progress broadcast sender)。
    pub active_downloads: Mutex<HashMap<u64, ActiveDownload>>,
    pub next_task_id: Mutex<u64>,
    /// 访问码（仅存内存，启动时为空，用户通过 Web UI 设置）。
    pub access_code: Mutex<String>,
}

/// 任务状态。
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TaskStatus {
    Downloading,
    Finished,
    Failed,
    Cancelled,
}

/// 单个活跃下载任务的状态。
pub struct ActiveDownload {
    pub cancel: CancelToken,
    pub progress_tx: broadcast::Sender<Progress>,
    pub filename: Option<String>,
    pub book_name: Option<String>,
    pub total_chapters: usize,
    pub current_chapter: u32,
    pub status: TaskStatus,
}

/// 所有 handler 共享的状态类型别名。
pub type SharedState = Arc<WebState>;

impl WebState {
    pub fn new(config: AppConfig, http: Arc<HttpClients>, rules: Vec<Rule>, db: Db) -> Self {
        let download_path = PathBuf::from(&config.download_path);
        Self {
            config: RwLock::new(config),
            http,
            rules: RwLock::new(rules),
            db: Mutex::new(db),
            download_path,
            active_downloads: Mutex::new(HashMap::new()),
            next_task_id: Mutex::new(1),
            access_code: Mutex::new(String::new()),
        }
    }
}

/// 启动 Web 服务器。阻塞当前线程直到进程退出。
pub fn run(
    config: AppConfig,
    http: Arc<HttpClients>,
    rules: Vec<Rule>,
    db: Db,
    host: String,
    port: u16,
) -> Result<()> {
    let state: SharedState = Arc::new(WebState::new(config, http, rules, db));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("so-novel-web")
        .build()
        .context("构造 web tokio runtime 失败")?;

    rt.block_on(async move {
        let session_config = SessionConfig::default();
        let session_store = SessionStore::<SessionNullPool>::new(None, session_config)
            .await
            .unwrap();
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
