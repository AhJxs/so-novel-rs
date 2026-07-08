//! Web 服务模块：axum HTTP 服务器 + 单页前端。
//!
//! 提供 REST API 和 SSE 推送，让用户通过浏览器搜索、下载小说。
//! 与 CLI 模式同构，直接调用底层 crawler / parser / export 函数。

mod error;
mod handlers;
mod routes;

#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use axum_session::{SessionConfig, SessionNullPool, SessionStore};
use serde::Serialize;

use crate::app::DownloadTask;
use crate::config::AppConfig;
use crate::http::HttpClients;
use crate::models::Rule;
use crate::db::SourcesConfig;

// ── rust-embed：编译期把 web-ui/dist/ 嵌入二进制 ──────────────────────
#[cfg(feature = "web")]
use axum::{
    body::Body,
    extract::Request,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
};
#[cfg(feature = "web")]
use rust_embed::RustEmbed;

/// 编译期嵌入 `web-ui/dist/` 下所有静态文件。
/// `include-exclude` feature 会按 .gitignore 跳过 node_modules/src/ 等。
#[cfg(feature = "web")]
#[derive(RustEmbed)]
#[folder = "web-ui/dist/"]
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

/// `WebState::new` / `web::run` 的额外初始化参数，避免参数过多。
pub struct WebInitParams {
    pub sources_config: SourcesConfig,
    pub sources_config_path: PathBuf,
    /// 启动时从 `tasks.json` 反序列化的任务列表（包括已完成 + 上次未结束的）；
    /// 由调用方 (`load_tasks`) 读文件后传入。空 vec 表示无历史。
    pub tasks: Vec<DownloadTask>,
    pub tasks_file: PathBuf,
    pub next_task_id: u64,
}

/// Web 服务共享状态。
///
/// ## 任务存储是单源的 `Vec<DownloadTask>`
///
/// 旧实现是双 store：`HashMap<u64, ActiveDownload>`（活跃内存态）+ `Vec<DownloadTaskRecord>`
/// （持久化历史）+ 一个跨线程的 bridge task 把两者同步 + merge 时还要兜底
/// "active 拿 None/0 别把历史里正确的元数据盖掉" —— 这类问题反复出现，本质就是
/// 双 store 各自维护同一份数据但时序窗口不一致。
///
/// 现在 web 跟 GPUI 一样只持一个 `Vec<DownloadTask>`：
/// - 持久化字段全在 record-like fields 上（id / origin / started_at_unix / ...）
/// - 运行期字段 `rx` / `cancel` / `cancelling` 也只是这个 struct 的一部分
/// - 每个下载一个 per-task drain tokio task 排空 mpsc rx（详见
///   `crate::web::handlers::download::spawn_task_drain`），同时负责
///   "drain 到的事件 → SSE broadcast_tx"，把"状态更新"和"事件转发"合并到一处。
///
/// `tasks_file` 只是磁盘路径，`save_tasks_to_file` 用它做原子写入。
pub struct WebState {
    pub config: RwLock<AppConfig>,
    pub http: Arc<HttpClients>,
    pub rules: RwLock<Vec<Rule>>,
    pub download_path: PathBuf,
    /// **单源真相**：每个任务（活跃 + 已结束）的所有状态都在这里。
    /// 跟 `crate::app::AppModel::tasks` 同型 —— web 和 GUI 用的是同一个类型。
    pub tasks: Mutex<Vec<DownloadTask>>,
    pub next_task_id: Mutex<u64>,
    /// 访问码（仅存内存，启动时为空，用户通过 Web UI 设置）。
    pub access_code: Mutex<String>,
    /// 书源配置（禁用列表等），toggle 时需要同步更新并持久化。
    pub sources_config: RwLock<SourcesConfig>,
    /// `sources_config.json` 磁盘路径。
    pub sources_config_path: PathBuf,
    /// `tasks.json` 磁盘路径。
    pub tasks_file: PathBuf,
}

/// 任务状态（API 返回用，跟 DownloadTaskRecord.finished 1:1 映射）。
///
/// 仅在序列化层暴露给前端用 —— 后端内部统一用 `DownloadTask::finished`
/// (`Option<Result<PathBuf, FinishedReason>>`)，避免字符串语义。
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TaskStatus {
    Downloading,
    Finished,
    Failed,
    Cancelled,
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
            access_code: Mutex::new(String::new()),
            sources_config: RwLock::new(params.sources_config),
            sources_config_path: params.sources_config_path,
            tasks_file: params.tasks_file,
        }
    }

    /// 把当前 `tasks` vec 序列化为 `DownloadTaskRecord` vec，写到磁盘。
    ///
    /// 触发时机：每次某任务终结（drain 线程从 rx Disconnected 或 crawler 写 finished）。
    /// 失败仅 warn —— tasks.json 偶尔丢一次不致命（best-effort 持久化，跟 GPUI 同语义）。
    ///
    /// **不**在内存 `tasks` 上反映 trim —— `save_with_trim` 只在 records vec 上做
    /// 修剪，in-memory `DownloadTask` 维持原样（next drain 才会再次被列出）。
    /// GPUI 走的也是这个语义（见 `AppModel::save_tasks_to_file` 的注释）。
    pub fn save_tasks_to_file(&self) {
        // Poisoned lock = 之前持锁的线程 panic 了：磁盘保存是 best-effort，
        // 这里吃掉 poison 错误 + log，不要再炸一次。
        let mut records: Vec<crate::models::DownloadTaskRecord> = match self.tasks.lock() {
            Ok(tasks) => {
                let r = tasks.iter().map(|t| t.to_record()).collect();
                drop(tasks);
                r
            }
            Err(e) => {
                tracing::error!("save_tasks_to_file: tasks Mutex poisoned: {e}");
                return;
            }
        };
        if let Err(e) = crate::db::save_with_trim(&self.tasks_file, &mut records) {
            tracing::warn!("保存 tasks.json 失败: {e}");
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
