//! 应用状态、状态结构体、业务方法集合。
//!
//! - `*_state` — 各页面状态结构体; `ops/*` — 跨状态业务方法; `events` — 后台通道排空。
//! - 入口 `AppModel` 持有所有状态 struct 实例, **UI 中立**（不依赖任何 GUI 框架）。

mod cover;
mod download;
pub(crate) mod events;
mod health;
mod library;
mod library_state;
mod list_cache;
mod persistence;
mod runtime;
mod search;
mod search_state;
mod sources;
mod sources_state;
pub use sources_state::SourcesFilterStatus;
pub(crate) mod tasks;
pub(crate) mod trace;
mod ui_event;
mod update;
mod update_state;

pub(crate) mod ops;

pub use cover::{CoverEntry, hash_short};
pub use library_state::{LibraryEntry, LibraryState, scan_library_dir};
pub use list_cache::{ListCache, ListCacheKey, PageKind, filter_signature};
pub use runtime::build_shared_runtime;
pub use search_state::{
    CoverEvent, DetailEvent, DetailState, SearchState, SourceSearchEvent, SourceStatus, TocEvent,
    TocState,
};
pub use sources_state::SourcesState;
pub use ui_event::UIEvent;
pub use update_state::{
    UpdateCheckResult, UpdateOutcome, UpdateState, check_github_latest_release,
};

use std::sync::Arc;

use anyhow::Result;
use tokio::runtime::Runtime;

use crate::config::{AppConfig, ConfigPaths};
use crate::core::DownloadTask;
use crate::core::bootstrap::load_context;
use crate::db::{SourcesConfig, load_tasks_from_file};
use crate::http::{HttpClients, SystemProxy, detect_system_proxy};
use crate::models::Rule;
use events::{WakeupHandle, WakeupReceiver};
use ops::OpsCtx;

/// 应用整体状态。UI 中立 —— 不依赖任何 GUI 框架, 由 `desktop` 层渲染。
pub struct AppModel {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub rules: Vec<Rule>,
    pub rule_load_error: Option<String>,
    pub config_load_error: Option<String>,

    /// 书源配置：活跃文件选择 + 禁用列表。
    pub sources_config: SourcesConfig,
    pub runtime: &'static Runtime,

    /// 共享 HTTP client 集合。复用连接池 + TLS session cache; 改 proxy / `unsafe_ssl` 时
    /// `HttpClients::rebuild_proxy` 整体替换实例。
    pub http: Arc<HttpClients>,

    /// 最近一次系统代理探测结果（设置页展示用）。
    ///
    /// 启动时探一次，之后由 [`Self::refresh_system_proxy`] 在「改代理模式」和「进入设置页」
    /// 两处刷新 —— 不做后台轮询，用户在 Clash 里改完要下次触发才生效。
    pub system_proxy: SystemProxy,

    pub search: SearchState,

    /// 活动 / 已完成的下载任务。最新加在末尾。
    pub tasks: Vec<DownloadTask>,
    next_task_id: u64,

    pub library: LibraryState,

    /// 书源管理页状态（连通性检测结果）。
    pub sources_state: SourcesState,
    pub update_state: UpdateState,

    /// 业务层 → UI 层的 [`UIEvent`] 队列, 由 `RootView::render` 排空并翻译成 notification。
    pub(crate) pending_ui_events: Vec<UIEvent>,

    pub list_cache: ListCache,
    pub wakeup: WakeupHandle,
}

impl AppModel {
    /// 构造函数。失败时调用方应向用户展示致命错误, 不要 panic。
    pub fn new() -> Result<Self> {
        Ok(Self::new_with_wakeup()?.0)
    }

    /// 构造 `AppModel` + 配套的 `WakeupReceiver`。
    pub fn new_with_wakeup() -> Result<(Self, WakeupReceiver)> {
        // 启动期公共资源 (paths / config / sources_config / rules / http) 统一走
        // `core::bootstrap::load_context`; 它加载失败时只 `tracing::warn!` + 兜底默认,
        // 所以 `config_load_error` / `rule_load_error` 保持 `None`。
        let ctx = load_context();
        let paths = ctx.paths;
        let config = ctx.config;
        let sources_config = ctx.sources_config;
        let rules = ctx.rules;
        let http = ctx.http;

        // 启动快照。**不需要** rebuild：`HttpClients::new` 构造时已经按
        // `resolve_proxy_url` 把系统代理算进去了，这里只是给 UI 留一份可读结果。
        let system_proxy = detect_system_proxy();

        let runtime = build_shared_runtime()?;

        let (tasks, next_task_id) = load_tasks_from_file(&paths.tasks_file);
        tracing::info!("从文件加载 {} 个历史下载任务", tasks.len());

        let (wakeup, rx) = events::new_wakeup();

        let model = Self {
            paths,
            config,
            rules,
            rule_load_error: None,
            config_load_error: None,
            sources_config,
            runtime,
            http,
            system_proxy,
            search: SearchState::default(),
            tasks,
            next_task_id,
            library: LibraryState::default(),
            sources_state: SourcesState::default(),
            update_state: UpdateState::default(),
            pending_ui_events: Vec::new(),
            list_cache: ListCache::new(),
            wakeup,
        };
        Ok((model, rx))
    }

    /// 重新探测系统代理，并让共享 HTTP client 跟上新结果。
    ///
    /// 触发点三处：启动（`new_with_wakeup` 快照，不重建）、改代理模式（`page_proxy`）、
    /// 进入设置页（`RootView::navigate`）。
    pub fn refresh_system_proxy(&mut self) {
        self.system_proxy = detect_system_proxy();
        // 结果变了的话，已构造的 client 里还钉着旧 proxy，必须整体换掉。
        // `rebuild_proxy` 按解析后的 URL 比对，没变即 no-op，所以多调不亏。
        if let Err(e) = self.http.rebuild_proxy(&self.config) {
            let msg = format!("HTTP client 重建失败: {e}");
            tracing::warn!("{msg}");
            self.push_error(msg);
        }
    }

    /// 内部：把一条 [`UIEvent`] 推入待处理队列。
    fn push_event(&mut self, ev: UIEvent) {
        self.pending_ui_events.push(ev);
    }

    pub fn push_info(&mut self, msg: impl Into<String>) {
        self.push_event(UIEvent::Info(msg.into()));
    }

    pub fn push_success(&mut self, msg: impl Into<String>) {
        self.push_event(UIEvent::Success(msg.into()));
    }

    pub fn push_warning(&mut self, msg: impl Into<String>) {
        self.push_event(UIEvent::Warning(msg.into()));
    }

    pub fn push_error(&mut self, msg: impl Into<String>) {
        self.push_event(UIEvent::Error(msg.into()));
    }

    /// 推一条**可点击**通知 —— 用户点 toast 时调 `cx.open_url(url)`。
    pub fn push_open_link(&mut self, msg: impl Into<String>, url: impl Into<String>) {
        self.push_event(UIEvent::OpenLink {
            message: msg.into(),
            url: url.into(),
        });
    }

    /// 构造 spawn 共享上下文。
    fn ops_ctx(&self) -> OpsCtx<'_> {
        OpsCtx {
            rules: &self.rules,
            config: &self.config,
            http: Arc::clone(&self.http),
            runtime: self.runtime,
            wakeup: &self.wakeup,
        }
    }
}
