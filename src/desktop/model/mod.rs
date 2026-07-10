//! 应用状态、状态结构体、业务方法集合。
//!
//! 拆分子模块：
//! - `search_state` / `library_state` / `sources_state` / `update_state` — 4 个页面状态结构体
//! - `cover` — UI 辅助（封面字节解码 + URI 生成）
//! - `runtime` — 自由辅助
//! - `crate::desktop::model::ops::download` / `crate::desktop::model::ops::search` / `crate::desktop::model::ops::sources` / `crate::desktop::model::ops::library` / `crate::desktop::model::ops::update` / `crate::desktop::model::ops::settings` — 业务方法
//!
//! 入口：`AppModel` 持有所有状态 struct 实例，UI 中立（不依赖任何 GUI 框架）。
//! 后台通道排空 + UI 重绘触发由 `crate::desktop::model::events` 负责。

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
mod tasks;
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

use crate::config::{AppConfig, ConfigPaths, load_config};
use crate::core::DownloadTask;
use crate::db::{SourcesConfig, load_tasks_from_file};use crate::http::HttpClients;
use crate::models::Rule;
use events::{WakeupHandle, WakeupReceiver};
use ops::OpsCtx;

/// 应用整体状态。UI 中立结构 —— 不依赖任何 GUI 框架，由 `desktop` 层渲染。
pub struct AppModel {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub rules: Vec<Rule>,
    pub rule_load_error: Option<String>,
    pub config_load_error: Option<String>,

    /// 书源配置：活跃文件选择 + 禁用列表。
    pub sources_config: SourcesConfig,

    /// 后台任务运行时。所有 spawn 都走它。
    /// 通过 `Box::leak` 得到 `&'static Runtime`，永不 drop ——
    /// 见 `build_shared_runtime` 注释，规避 Runtime drop panic。
    pub runtime: &'static Runtime,

    /// 共享 HTTP client 集合。一次性构造，跨所有爬取任务复用
    /// 连接池 + TLS session cache —— 改 proxy / `unsafe_ssl` 时
    /// `HttpClients::rebuild_proxy` 会整体替换实例。
    pub http: Arc<HttpClients>,

    /// 搜索下载页状态。
    pub search: SearchState,

    /// 活动 / 已完成的下载任务。最新加在末尾。
    pub tasks: Vec<DownloadTask>,
    next_task_id: u64,

    /// 本地书库状态（首次进入 Library 页时延迟扫描）。
    pub library: LibraryState,

    /// 书源管理页状态（连通性检测结果）。
    pub sources_state: SourcesState,

    /// 版本更新检查状态。
    pub update_state: UpdateState,

    /// 业务层 → UI 层的 [`UIEvent`] 队列（notification toast + 可点击 `OpenLink`）。
    ///
    /// `events::drain` 跑在 `AsyncApp::update_entity` 闭包里，**拿不到 `&mut Window`**；
    /// 而 `WindowExt::push_notification` 必须 `&mut Window` + `&mut App`。
    /// 解法：drain 把构造好的 [`UIEvent`] 推到这个 Vec，由 `RootView::render`（拿得到
    /// `&mut Window`）排空 + 翻译成 `gpui_component::notification::Notification` 再
    /// 真正 push 到 UI。
    ///
    /// 为什么用 plain enum：`app/` 想保持 UI 框架解耦（CLAUDE.md 明确要求）；`UIEvent`
    /// 是业务层 → UI 层的事件桥，零 `gpui` / `gpui_component` 依赖。
    pub(crate) pending_ui_events: Vec<UIEvent>,

    /// 列表渲染缓存（Library / Search / Tasks 三页共用）。
    /// 详见 `crate::desktop::model::list_cache`。
    pub list_cache: ListCache,

    /// 后台 → `drain_loop` 的唤醒信号 sender。后台 producer 写入新数据时
    /// `notify()` 一下，让 `drain_loop` 立刻醒过来排空 + notify()，不必等
    /// 100ms 兜底。详见 `crate::desktop::model::events::WakeupHandle`。
    pub wakeup: WakeupHandle,
}

impl AppModel {
    /// UI 中立的构造函数。返回 `Result`：初始化失败时（极少见），调用方应
    /// 捕获并向用户展示致命错误（如 `rfd::MessageDialog`），不要 panic。
    ///
    /// 内部走 [`Self::new_with_wakeup`] 并丢弃 receiver ——
    /// 无主动唤醒通路，纯 100ms 兜底 tick。需要 wakeup 的调用方请直接用
    /// `new_with_wakeup`。
    pub fn new() -> Result<Self> {
        Ok(Self::new_with_wakeup()?.0)
    }

    /// 构造 `AppModel` + 配套的 `WakeupReceiver`（主构造函数）。
    ///
    /// 调用方（`desktop::run`）拿到 `WakeupReceiver` 后传给 `drain_loop`，
    /// wakeup 通路才生效：后台 producer 写入新数据时 `notify()`，drain_loop
    /// 立刻醒来排空，不必等 100ms 兜底。
    pub fn new_with_wakeup() -> Result<(Self, WakeupReceiver)> {
        let paths = ConfigPaths::discover();

        let (config, config_load_error) = Self::bootstrap_config(&paths);

        // 初始化规则目录（首次启动时复制默认规则文件）
        if let Err(e) = crate::db::init_rules_dir(&paths.rules_dir) {
            tracing::warn!("规则目录初始化失败: {e:#}");
        }

        let sources_config = Self::bootstrap_sources_config(&paths);

        let runtime = build_shared_runtime()?;

        let (rules, rule_load_error) =
            match crate::db::load_active_rules(&paths.rules_dir, &sources_config) {
                Ok(rs) => (rs, None),
                Err(e) => {
                    tracing::warn!("rules load failed: {e:#}");
                    (Vec::new(), Some(format!("{e:#}")))
                }
            };

        let (tasks, next_task_id) = load_tasks_from_file(&paths.tasks_file);
        tracing::info!("从文件加载 {} 个历史下载任务", tasks.len());

        // 共享 HTTP client 集合。构造失败（proxy URL 非法等）沿 Result 冒到
        // desktop 入口，由 rfd 弹致命错误对话框 —— 与 runtime 失败同等待遇。
        let http = Arc::new(HttpClients::new(&config)?);

        let (wakeup, rx) = events::new_wakeup();

        let model = Self {
            paths,
            config,
            rules,
            rule_load_error,
            config_load_error,
            sources_config,
            runtime,
            http,
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

    /// 加载 `config.toml`：失败则回落默认值并返回错误串；首次启动（文件不
    /// 存在）时写出默认配置。
    fn bootstrap_config(paths: &ConfigPaths) -> (AppConfig, Option<String>) {
        let (config, err) = match load_config(&paths.config_file) {
            Ok(c) => (c, None),
            Err(e) => {
                tracing::warn!("config load failed: {e:#}");
                (AppConfig::default(), Some(format!("{e:#}")))
            }
        };

        if !paths.config_file.exists() {
            match crate::config::save_config(&paths.config_file, &config) {
                Ok(()) => tracing::info!("首次启动：已生成 {}", paths.config_file.display()),
                Err(e) => tracing::warn!("写入默认 config.toml 失败: {e:#}"),
            }
        }

        (config, err)
    }

    /// 加载书源配置；首次启动（文件不存在）时写出默认。
    fn bootstrap_sources_config(paths: &ConfigPaths) -> SourcesConfig {
        let sources_config = SourcesConfig::load(&paths.sources_config);
        if !paths.sources_config.exists() {
            if let Err(e) = sources_config.save(&paths.sources_config) {
                tracing::warn!("写入默认 sources_config.json 失败: {e:#}");
            }
        }
        sources_config
    }

    /// 内部：把一条 [`UIEvent`] 推入待处理队列。语义见 [`Self::pending_ui_events`]。
    fn push_event(&mut self, ev: UIEvent) {
        self.pending_ui_events.push(ev);
    }

    /// 推一条 info 级通知。语义见 [`Self::pending_ui_events`]。
    pub fn push_info(&mut self, msg: impl Into<String>) {
        self.push_event(UIEvent::Info(msg.into()));
    }

    /// 推一条 success 级通知。语义见 [`Self::pending_ui_events`]。
    pub fn push_success(&mut self, msg: impl Into<String>) {
        self.push_event(UIEvent::Success(msg.into()));
    }

    /// 推一条 warning 级通知。语义见 [`Self::pending_ui_events`]。
    pub fn push_warning(&mut self, msg: impl Into<String>) {
        self.push_event(UIEvent::Warning(msg.into()));
    }

    /// 推一条 error 级通知。语义见 [`Self::pending_ui_events`]。
    pub fn push_error(&mut self, msg: impl Into<String>) {
        self.push_event(UIEvent::Error(msg.into()));
    }

    /// 推一条**可点击**通知 —— 用户点 toast 时调 `cx.open_url(url)`（浏览器开链接）。
    ///
    /// 例：版本检查"有新版本"toast，message 显示 "有新版本 v0.3.0"，点击 → 跳
    /// `https://github.com/AhJxs/so-novel-rs/releases/latest`。`on_click` 在
    /// `desktop::root::ui_event_to_notification` 翻译层挂上。
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
            // 共享 `&self.wakeup` —— `WakeupHandle::Clone` 是 cheap 的 Sender
            // 克隆，prod ucer 拿到的 sender 写入会直接唤醒 drain_loop。
            wakeup: &self.wakeup,
        }
    }
}
