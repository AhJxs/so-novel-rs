//! 应用状态、状态结构体、业务方法集合。
//!
//! 拆分子模块：
//! - `download_task`  / `search_state` / `library_state` / `sources_state` / `update_state` — 5 个状态结构体
//! - `cover` — UI 辅助（封面字节解码 + URI 生成）
//! - `runtime` — 自由辅助
//! - `crate::app::ops::download` / `crate::app::ops::search` / `crate::app::ops::sources` / `crate::app::ops::library` / `crate::app::ops::update` / `crate::app::ops::settings` — 业务方法
//!
//! 入口：`AppModel` 持有所有状态 struct 实例，UI 中立（不依赖任何 GUI 框架）。
//! 后台通道排空 + UI 重绘触发由 `crate::app::events` 负责。

mod cover;
mod download;
pub mod download_task;
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
mod tasks_init;
pub(crate) mod trace;
mod ui_event;
mod update;
mod update_state;

pub(crate) mod ops;

pub use cover::{CoverEntry, hash_short};
pub use download_task::DownloadTask;
pub use library_state::{LibraryEntry, LibraryState, scan_library_dir};
pub use list_cache::{ListCache, ListCacheKey, PageKind, filter_signature};
pub use runtime::build_shared_runtime;
pub use search_state::{
    CoverEvent, DetailEvent, DetailState, SearchState, SourceSearchEvent, SourceStatus, TocEvent,
    TocState,
};
pub use sources_state::SourcesState;
pub use tasks_init::load_tasks_from_file;
pub use ui_event::UIEvent;
pub use update_state::{
    UpdateCheckResult, UpdateOutcome, UpdateState, check_github_latest_release,
};

use std::sync::Arc;

use anyhow::Result;
use tokio::runtime::Runtime;

use crate::config::{AppConfig, ConfigPaths, load_config};
use crate::http::HttpClients;
use crate::i18n::{ts, ts_fmt};
use crate::models::{Rule, SearchResult};
use crate::db::SourcesConfig;
use events::{WakeupHandle, WakeupReceiver};
use ops::OpsCtx;

/// 应用整体状态。UI 中立结构 —— 不依赖任何 GUI 框架，由 `gpui_app` 层渲染。
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
    /// 连接池 + TLS session cache —— 改 proxy / unsafe_ssl 时
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

    /// 待推送到 gpui-component Notification 层的通知队列。
    ///
    /// `events::drain` 跑在 `AsyncApp::update_entity` 闭包里，**拿不到 `&mut Window`**；
    /// 而 `WindowExt::push_notification` 必须 `&mut Window` + `&mut App`。
    /// 解法：drain 把构造好的 [`UIEvent`] 推到这个 Vec，由 `RootView::render`（拿得到
    /// `&mut Window`）排空 + 翻译成 `gpui_component::notification::Notification` 再
    /// 真正 push 到 UI。
    ///
    /// 为什么用 plain enum：`app/` 想保持 UI 框架解耦（CLAUDE.md 明确要求）；`UIEvent`
    /// 是业务层 → UI 层的事件桥，零 `gpui` / `gpui_component` 依赖。
    pub(crate) pending_notifications: Vec<UIEvent>,

    /// 列表渲染缓存（Library / Search / Tasks 三页共用）。
    /// 详见 `crate::app::list_cache`。
    pub list_cache: ListCache,

    /// 后台 → drain_loop 的唤醒信号 sender。后台 producer 写入新数据时
    /// `notify()` 一下，让 drain_loop 立刻醒过来排空 + notify()，不必等
    /// 100ms 兜底。详见 `crate::app::events::WakeupHandle`。
    pub wakeup: WakeupHandle,
}

impl AppModel {
    /// UI 中立的构造函数。
    ///
    /// 返回 `Result`：初始化失败时（极少见），调用方应捕获并向用户展示
    /// 致命错误（如 `rfd::MessageDialog`），不要 panic。
    pub fn new() -> Result<Self> {
        let paths = ConfigPaths::discover();

        let (config, config_load_error) = match load_config(&paths.config_file) {
            Ok(c) => (c, None),
            Err(e) => {
                tracing::warn!("config load failed: {e:#}");
                (AppConfig::default(), Some(format!("{e:#}")))
            }
        };

        if !paths.config_file.exists() {
            if let Err(e) = crate::config::save_config(&paths.config_file, &config) {
                tracing::warn!("写入默认 config.toml 失败: {e:#}");
            } else {
                tracing::info!("首次启动：已生成 {}", paths.config_file.display());
            }
        }

        // 初始化规则目录（首次启动时复制默认规则文件）
        if let Err(e) = crate::db::init_rules_dir(&paths.rules_dir) {
            tracing::warn!("规则目录初始化失败: {e:#}");
        }

        // 加载书源配置
        let sources_config = crate::db::SourcesConfig::load(&paths.sources_config);
        if !paths.sources_config.exists() {
            if let Err(e) = sources_config.save(&paths.sources_config) {
                tracing::warn!("写入默认 sources_config.json 失败: {e:#}");
            }
        }

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

        let search = SearchState::default();

        // 共享 HTTP client 集合。构造失败（proxy URL 非法等）沿 Result 冒到
        // gpui_app 入口，由 rfd 弹致命错误对话框 —— 与 runtime 失败同等待遇。
        let http = Arc::new(HttpClients::new(&config)?);

        Ok(Self {
            paths,
            config,
            rules,
            rule_load_error,
            config_load_error,
            sources_config,
            runtime,
            http,
            search,
            tasks,
            next_task_id,
            library: LibraryState::default(),
            sources_state: SourcesState::default(),
            update_state: UpdateState::default(),
            pending_notifications: Vec::new(),
            list_cache: ListCache::new(),
            wakeup: {
                let (tx, _rx) = events::new_wakeup();
                tx
            },
        })
    }

    /// 构造 AppModel + 配套的 `WakeupReceiver`。
    ///
    /// 调用方（`gpui_app::run`）拿到 `WakeupReceiver` 后传给 `drain_loop`，
    /// 才能让 wakeup 通路生效。`AppModel::new` 内部建好 sender 后**丢弃**
    /// receiver（旧路径，纯 100ms tick，不接收主动唤醒；保留向后兼容）。
    pub fn new_with_wakeup() -> Result<(Self, WakeupReceiver)> {
        let (tx, rx) = events::new_wakeup();
        let mut model = Self::new()?;
        model.wakeup = tx;
        Ok((model, rx))
    }

    /// 推一条 info 级通知。语义见 [`Self::pending_notifications`]。
    pub fn push_info(&mut self, msg: impl Into<String>) {
        self.pending_notifications.push(UIEvent::Info(msg.into()));
    }

    /// 推一条 success 级通知。语义见 [`Self::pending_notifications`]。
    pub fn push_success(&mut self, msg: impl Into<String>) {
        self.pending_notifications
            .push(UIEvent::Success(msg.into()));
    }

    /// 推一条 warning 级通知。语义见 [`Self::pending_notifications`]。
    pub fn push_warning(&mut self, msg: impl Into<String>) {
        self.pending_notifications
            .push(UIEvent::Warning(msg.into()));
    }

    /// 推一条 error 级通知。语义见 [`Self::pending_notifications`]。
    pub fn push_error(&mut self, msg: impl Into<String>) {
        self.pending_notifications.push(UIEvent::Error(msg.into()));
    }

    /// 推一条**可点击**通知 —— 用户点 toast 时调 `cx.open_url(url)`（浏览器开链接）。
    ///
    /// 例：版本检查"有新版本"toast，message 显示 "有新版本 v0.3.0"，点击 → 跳
    /// `https://github.com/AhJxs/so-novel-rs/releases/latest`。`on_click` 在
    /// `gpui_app::root::ui_event_to_notification` 翻译层挂上。
    pub fn push_open_link(&mut self, msg: impl Into<String>, url: impl Into<String>) {
        self.pending_notifications.push(UIEvent::OpenLink {
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

    // 持久化 / 下载 / 搜索 / 书源 / 库 / 任务 / 更新 / 健康相关方法见
    // `super::{persistence, download, search, sources, library, tasks, update, health}`。
}
