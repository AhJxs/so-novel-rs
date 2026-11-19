use std::sync::Arc;

use anyhow::Result;
use tokio::runtime::Runtime;

use super::events::{self, WakeupHandle, WakeupReceiver};
use super::ops::OpsContext;
use super::{
    LibraryState, ListCache, SearchState, SourcesState, UIEvent, UpdateState, build_shared_runtime,
};
use crate::config::{AppConfig, ConfigPaths};
use crate::core::DownloadTask;
use crate::core::bootstrap::load_context;
use crate::db::{
    KEY_SOURCES, KEY_TASKS, LoadedTasks, SourcesConfig, StateOpenError, StateStore,
    load_tasks_from_bytes,
};
use crate::http::{HttpClients, SystemProxy, detect_system_proxy};
use crate::library::LibraryStore;
use crate::models::Rule;

/// 应用整体状态。UI 中立 —— 不依赖任何 GUI 框架, 由 `desktop` 层渲染。
pub struct AppModel {
    /// 配置与数据文件路径。
    pub paths: ConfigPaths,
    /// 已加载的应用配置。
    pub config: AppConfig,
    /// 当前活跃的书源规则；派发的 `Source` 与本列表共享同一批 `Arc`。
    pub rules: Vec<Arc<Rule>>,
    /// 书源加载失败的原因；成功时为 `None`。
    pub rule_load_error: Option<String>,
    /// 配置读取失败的原因；成功时为 `None`。
    pub config_load_error: Option<String>,

    /// 书源配置：活跃文件选择 + 禁用列表。
    pub sources_config: SourcesConfig,
    /// 运行数据存储（书架 / 下载任务 / 书源选择）。
    pub state: StateStore,
    /// 后台抓取共用的 Tokio 运行时。
    pub runtime: &'static Runtime,

    /// 共享 HTTP client 集合。复用连接池 + TLS session cache; 改 proxy / `unsafe_ssl` 时
    /// `HttpClients::rebuild_proxy` 整体替换实例。后台任务在 `spawn` 前用
    /// `HttpClients::for_rule` 取出 owned `reqwest::Client`, 不持有本字段的引用。
    pub http: HttpClients,

    /// 最近一次系统代理探测结果（设置页展示用）。
    ///
    /// 启动时探一次，之后由 [`Self::refresh_system_proxy`] 在「改代理模式」和「进入设置页」
    /// 两处刷新 —— 不做后台轮询，用户在 Clash 里改完要下次触发才生效。
    pub system_proxy: SystemProxy,

    /// 搜索页状态。
    pub search: SearchState,

    /// 活动 / 已完成的下载任务。最新加在末尾。
    pub tasks: Vec<DownloadTask>,
    pub(super) next_task_id: u64,

    /// 下载文件库页状态。
    pub library: LibraryState,

    /// 书源管理页状态（连通性检测结果）。
    pub sources_state: SourcesState,
    /// 版本更新检查状态。
    pub update_state: UpdateState,

    /// 业务层 → UI 层的 [`UIEvent`] 队列, 由 `RootView::render` 排空并翻译成 notification。
    pub(crate) pending_ui_events: Vec<UIEvent>,

    /// 各列表的渲染缓存。
    pub list_cache: ListCache,
    /// 后台线程唤醒 UI 的句柄。
    pub wakeup: WakeupHandle,
}

impl AppModel {
    /// 构造函数。失败时调用方应向用户展示致命错误, 不要 panic。
    ///
    /// # Errors
    ///
    /// tokio runtime 构造或历史任务加载失败时返回错误（见 [`Self::new_with_wakeup`]）。
    pub fn new() -> Result<Self> {
        Ok(Self::new_with_wakeup()?.0)
    }

    /// 构造 `AppModel` + 配套的 `WakeupReceiver`。
    ///
    /// 运行数据存在 `~/.sonovel/state`（SurrealKV）。库在启动时独占打开并保持到退出：
    /// 第二个实例打不开目录锁，这里返回「应用已在运行」的致命错误。
    ///
    /// # Errors
    ///
    /// tokio runtime 构造失败（资源耗尽）、或另一个实例已占用运行数据目录时返回错误。
    pub fn new_with_wakeup() -> Result<(Self, WakeupReceiver)> {
        let paths = ConfigPaths::discover();
        let runtime = build_shared_runtime()?;

        // 1. 打开运行数据。锁冲突 = 已经有窗口在跑，启动直接失败（单实例）。
        let (state, snapshot) =
            StateStore::open(paths.state_dir.clone(), runtime).map_err(|error| match error {
                StateOpenError::Locked => {
                    anyhow::anyhow!(rust_i18n::t!("Library.error.already_running").to_string())
                }
                StateOpenError::Other(error) => error,
            })?;

        // 2. 书源选择要先于规则加载：规则目录按活跃书源文件解析。
        let sources_config = snapshot
            .sources
            .as_deref()
            .map_or_else(SourcesConfig::default, SourcesConfig::from_json);
        if snapshot.sources.is_none() {
            // 首次运行：把默认选择写进存储（顺带写入 schema 版本）。
            match sources_config.to_json() {
                Ok(bytes) => {
                    if let Err(error) = state.put(KEY_SOURCES, &bytes) {
                        tracing::warn!("写入默认书源选择失败: {error:#}");
                    }
                }
                Err(error) => tracing::warn!("序列化默认书源选择失败: {error:#}"),
            }
        }

        // 3. 下载任务：未完成的标成「应用重启中断」，需要时立刻回写。
        let (LoadedTasks { tasks, max_id }, tasks_rewrite) =
            load_tasks_from_bytes(snapshot.tasks.as_deref());
        tracing::info!("从运行数据加载 {} 个历史下载任务", tasks.len());
        if let Some(rewrite) = tasks_rewrite
            && let Err(error) = state.put(KEY_TASKS, &rewrite)
        {
            tracing::warn!("回写中断任务失败: {error:#}");
        }
        let next_task_id = max_id + 1;

        // 4. 书架。损坏的 blob 备份到 state 目录，再以空库启动。
        let (library_store, library_warning) = match snapshot.library.as_deref() {
            Some(bytes) => LibraryStore::from_json(bytes, &paths.state_dir),
            None => (LibraryStore::default(), None),
        };

        // 5. 公共资源 (config / rules / http)。失败只 warn + 默认，不挡住启动。
        let ctx = load_context(paths, sources_config);
        let paths = ctx.paths;
        let config = ctx.config;
        let sources_config = ctx.sources_config;
        let rules = ctx.rules;
        let http = ctx.http;

        // 启动快照。**不需要** rebuild：`HttpClients::new` 构造时已经按
        // `resolve_proxy_url` 把系统代理算进去了，这里只是给 UI 留一份可读结果。
        let system_proxy = detect_system_proxy();

        let (wakeup, rx) = events::new_wakeup();

        let mut model = Self {
            paths,
            config,
            rules,
            rule_load_error: None,
            config_load_error: None,
            sources_config,
            state,
            runtime,
            http,
            system_proxy,
            search: SearchState::default(),
            tasks,
            next_task_id,
            library: LibraryState::new(library_store),
            sources_state: SourcesState::default(),
            update_state: UpdateState::default(),
            pending_ui_events: Vec::new(),
            list_cache: ListCache::new(),
            wakeup,
        };
        if let Some(error) = snapshot.error.clone() {
            // 库打不开 / 读不出：这次运行不保存，提示用户去删 state 目录。
            model.push_error(error);
        } else if snapshot.read_only {
            model.push_error(rust_i18n::t!("Library.error.state_read_only").to_string());
        }
        if let Some(warning) = library_warning {
            model.push_error(warning);
        }
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

    /// 排入一条普通信息通知。
    pub fn push_info(&mut self, msg: String) {
        self.push_event(UIEvent::Info(msg));
    }

    /// 排入一条成功通知。
    pub fn push_success(&mut self, msg: String) {
        self.push_event(UIEvent::Success(msg));
    }

    /// 排入一条警告通知。
    pub fn push_warning(&mut self, msg: String) {
        self.push_event(UIEvent::Warning(msg));
    }

    /// 排入一条错误通知。
    pub fn push_error(&mut self, msg: String) {
        self.push_event(UIEvent::Error(msg));
    }

    /// 推一条**可点击**通知 —— 用户点 toast 时调 `cx.open_url(url)`。
    pub fn push_open_link(&mut self, msg: String, url: String) {
        self.push_event(UIEvent::OpenLink { message: msg, url });
    }

    /// 构造 spawn 共享上下文。
    pub(super) fn ops_ctx(&self) -> OpsContext<'_> {
        OpsContext {
            rules: &self.rules,
            config: &self.config,
            http: &self.http,
            runtime: self.runtime,
            wakeup: &self.wakeup,
        }
    }
}
