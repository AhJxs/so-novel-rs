//! 应用状态、状态结构体、业务方法集合。
//!
//! 拆分子模块：
//! - `download_task`  / `search_state` / `library_state` / `sources_state` / `update_state` — 5 个状态结构体
//! - `cover` — UI 辅助（封面字节解码 + URI 生成）
//! - `now` / `runtime` — 自由辅助
//! - `crate::app::ops::download` / `crate::app::ops::search` / `crate::app::ops::sources` / `crate::app::ops::library` / `crate::app::ops::update` / `crate::app::ops::settings` — 业务方法
//!
//! 入口：`AppModel` 持有所有状态 struct 实例，UI 中立（不依赖任何 GUI 框架）。
//! 后台通道排空 + UI 重绘触发由 `crate::app::events` 负责。

mod cover;
mod download_task;
pub(crate) mod events;
mod library_state;
mod list_cache;
mod now;
mod runtime;
mod search_state;
mod sources_state;
pub use sources_state::SourcesFilterStatus;
mod tasks_init;
pub(crate) mod trace;
mod ui_event;
mod update_state;

pub(crate) mod ops;

pub use cover::{CoverEntry, hash_short};
pub use download_task::DownloadTask;
pub use library_state::{LibraryEntry, LibraryState, scan_library_dir};
pub use list_cache::{ListCache, ListCacheKey, PageKind, filter_signature};
pub use now::now_unix_secs;
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
use crate::models::{Book, Chapter, Rule, SearchResult};
use crate::persistent::SourcesConfig;
use events::{WakeupHandle, WakeupReceiver};
use ops::{
    OpsCtx, add_sources_from_file, clear_finished_tasks, delete_library_entry, delete_source,
    persist_settings, refresh_library, select_search_result, spawn_download, spawn_download_range,
    spawn_health_check, spawn_resolve_toc, spawn_search, spawn_update_check, switch_active_file,
    toggle_source_disabled,
};

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
    pub pending_notifications: Vec<UIEvent>,

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
        if let Err(e) = crate::persistent::init_rules_dir(&paths.rules_dir) {
            tracing::warn!("规则目录初始化失败: {e:#}");
        }

        // 加载书源配置
        let sources_config = crate::persistent::SourcesConfig::load(&paths.sources_config);
        if !paths.sources_config.exists() {
            if let Err(e) = sources_config.save(&paths.sources_config) {
                tracing::warn!("写入默认 sources_config.json 失败: {e:#}");
            }
        }

        let runtime = build_shared_runtime()?;

        let (rules, rule_load_error) =
            match crate::persistent::load_active_rules(&paths.rules_dir, &sources_config) {
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

    /// 保存所有任务到文件，并自动清理超额的已完成任务。
    ///
    /// 性能改造：原实现把所有 `std::fs::*` + `fsync` 同步跑在调用线程上
    ///（通常是 UI 线程 / drain_loop），下载密集时一次 fsync 可让 UI 顿
    /// 几十毫秒。现改为：
    /// 1. UI 线程：把 `tasks` 转为 `DownloadTaskRecord` Vec，clone 出来
    ///    （仅 `origin` 里有 `String` / 路径，深拷贝代价不大）；
    /// 2. `runtime.spawn(spawn_blocking(...))` 把 trim + write + fsync + rename
    ///    派到 tokio blocking pool；
    /// 3. UI 线程**不**等待结果 —— 失败只 warn；下一次保存会覆盖同一文件，
    ///    数据丢失最坏情况是一次（tasks.json 偶尔丢一次不致命，tasks
    ///    持久化本来就是 best-effort）。
    ///
    /// **不等待结果**的副作用：本次 trim 删除的记录**下一次** `drain` 才同步
    /// 清理 `self.tasks`。换言之，UI 上可能多显示若干被 trim 的旧任务一帧。
    /// 这个权衡是值得的 —— 顿 UI 比显示一帧 stale 数据更糟。
    pub fn save_tasks_to_file(&mut self) {
        // 1. UI 线程：构造 record Vec（clone）并 spawn 异步保存。
        let records: Vec<crate::models::DownloadTaskRecord> =
            self.tasks.iter().map(|t| t.to_record()).collect();
        let path = self.paths.tasks_file.clone();

        self.runtime.spawn(async move {
            // 2. blocking pool：trim + write + fsync + rename。
            //    失败仅 warn —— tasks.json 的丢失可由下一次保存覆盖。
            let mut records = records;
            if let Err(e) = crate::persistent::save_with_trim(&path, &mut records) {
                tracing::warn!("保存任务到文件失败: {e:#}");
                return;
            }
            // 保存成功：trim 已就地修改 records（删除了超额项）。下一次 drain
            // 不会重做这一步 —— 我们**不再**回写 record_ids 到 self.tasks（已
            // 在 spawn 里跑过，AppModel 拿不到引用）。这是"best-effort 持久化"
            // 语义：丢失的旧任务在内存里再多留一会儿，下次手动操作（删除任务等）
            // 会自然清理。
            let trimmed = records.len();
            tracing::debug!("tasks.json 持久化成功，保留 {trimmed} 条记录");
        });
        // 立即返回，UI 线程不阻塞。
    }

    /// 保存书源配置到文件。
    pub fn save_sources_config(&self) {
        if let Err(e) = self.sources_config.save(&self.paths.sources_config) {
            tracing::warn!("保存书源配置失败: {e:#}");
        }
    }

    /// 派一个新的下载任务。返回新任务 id。
    pub fn spawn_download(&mut self, target: SearchResult) -> u64 {
        let ctx = self.ops_ctx();
        let (id, task) = spawn_download(&ctx, self.next_task_id, target);
        self.next_task_id += 1;
        self.tasks.push(task);
        self.save_tasks_to_file();
        id
    }

    /// 派一个 TOC 预取任务（获取元数据 + 章节列表，不开始下载）。
    pub fn spawn_resolve_toc(&mut self, target: &SearchResult) {
        let ctx = self.ops_ctx();
        let rx = spawn_resolve_toc(&ctx, target);
        self.search.toc_rx = Some(rx);
    }

    /// 派一个指定章节范围的下载任务。跳过 resolve 阶段，直接进入下载。
    /// 返回新任务 id。
    pub fn spawn_download_range(
        &mut self,
        target: SearchResult,
        book: Book,
        chapters: Vec<Chapter>,
    ) -> u64 {
        let ctx = self.ops_ctx();
        let (id, task) = spawn_download_range(&ctx, self.next_task_id, target, book, chapters);
        self.next_task_id += 1;
        self.tasks.push(task);
        self.save_tasks_to_file();
        id
    }

    /// 派聚合搜索任务。
    pub fn spawn_search(&mut self) -> bool {
        spawn_search(
            &self.rules,
            &self.config,
            Arc::clone(&self.http),
            self.runtime,
            &mut self.search,
        )
    }

    /// 选中某条搜索结果。
    pub fn select_search_result(&mut self, idx: usize) {
        select_search_result(
            &self.rules,
            &self.config,
            Arc::clone(&self.http),
            self.runtime,
            &mut self.search,
            idx,
        );
    }

    /// 切换书源禁用状态。
    pub fn toggle_source_disabled(&mut self, source_url: &str) {
        toggle_source_disabled(&mut self.sources_config, &mut self.rules, source_url);
        self.sources_state.clear_health();
        self.save_sources_config();
    }

    /// 从 JSON 文件导入书源。
    ///
    /// 自动复制文件到 `~/.sonovel/rules/`，重名则覆盖。
    /// 反馈给用户的 toast 显示导入的文件名。
    pub fn add_sources_from_file(&mut self, path: &std::path::Path) {
        match add_sources_from_file(
            &self.paths.rules_dir,
            &mut self.sources_config,
            &mut self.rules,
            &mut self.rule_load_error,
            path,
        ) {
            Ok(result) => {
                let msg =
                    crate::i18n::ts_fmt("Sources.import.result", &[("filename", &result.filename)])
                        .to_string();
                self.sources_state.clear_health();
                // 如果导入的就是当前活跃文件，rule 集合已被重载：旧搜索结果的
                // `source_id` 在新 rule 里可能指向错源（详见 `switch_active_file`
                // 同款注释）。只清这一种情况 —— 导入非活跃文件不影响 rule 集合。
                if result.reloaded_active {
                    self.search.clear_results_and_caches();
                    self.list_cache.clear();
                }
                self.push_success(msg);
                self.save_sources_config();
            }
            Err(msg) => {
                if msg.starts_with("文件内容为空") || msg.starts_with("文件中未找到有效")
                {
                    self.push_warning(msg);
                } else {
                    self.push_error(msg);
                }
            }
        }
    }

    /// 删除一条书源。
    pub fn delete_source(&mut self, source_url: &str) {
        match delete_source(
            &self.paths.rules_dir,
            &self.sources_config,
            &mut self.rules,
            &mut self.sources_state,
            source_url,
        ) {
            Ok(true) => {
                self.push_success(ts_fmt("Toasts.delete_source_ok", &[("url", source_url)]))
            }
            Ok(false) => self.push_warning(ts("Toasts.delete_source_missing")),
            Err(msg) => self.push_error(msg),
        }
    }

    /// 切换活跃书源文件。
    pub fn switch_active_file(&mut self, filename: &str) {
        match switch_active_file(
            &self.paths.rules_dir,
            &mut self.sources_config,
            &mut self.rules,
            &mut self.rule_load_error,
            filename,
        ) {
            Ok(()) => {
                self.sources_state.clear_health();
                // rule 集合整体替换 → 旧搜索结果的 `source_id` 在新 rule 里
                // 可能指向完全不同的源（数值 ID 在不同文件里不复用）。直接清空
                // 避免用户点了旧结果去下载，结果跑到错源上。
                self.search.clear_results_and_caches();
                // 源切换会让所有 `*_version` 翻篇；旧 cache entry 的 `data_version`
                // 必然对不上新值，但清空更稳（避免 stale 占用 + 缩小内存）。
                self.list_cache.clear();
                self.push_success(ts_fmt(
                    "Toasts.switch_source_file_ok",
                    &[("filename", filename)],
                ));
                self.save_sources_config();
            }
            Err(msg) => self.push_error(msg),
        }
    }

    /// 把当前 config 写回 config.toml。
    ///
    /// **Auto-save 模式**：每个 setter 改完字段后立即调本方法写盘 —— 没有"立即保存"
    /// 按钮，没有 dirty 概念。成功时静默（tracing::debug 留痕），失败时弹 error
    /// notification 让用户知道（极少见：磁盘满 / 权限问题 / 路径不存在等）。
    ///
    /// 每个 setter 改完字段后立即调本方法（auto-save），无需「立即保存」按钮。
    /// 如果以后想加 debounce 写入（比如连续拖动 number input），可以在 setter 里加
    /// cx.spawn(timer 500ms) 合并多次 persist_settings 调用 —— 单次写盘本来就很快
    /// （小 TOML 几 ms），目前不做 debounce。
    pub fn persist_settings(&mut self) {
        if let Err(msg) = persist_settings(&self.config, &self.paths.config_file) {
            tracing::warn!("自动保存 config.toml 失败: {msg}");
            self.push_error(msg);
            return;
        }
        tracing::debug!("config.toml 自动保存成功");

        // proxy / unsafe_ssl 改了 → 重建共享 HTTP client。
        // `rebuild_proxy` 内部按 `(proxy_enabled, proxy_host, proxy_port)` 三元组
        // 比对，未变则 no-op；其它字段（theme / language / timeout 等）不触发。
        // 重建失败：config 已写盘但客户端拿的是旧配置 → 推 error 让用户知道；
        // 下次重启 / 再次触发 persist_settings 会重试。
        if let Err(e) = self.http.rebuild_proxy(&self.config) {
            let msg = format!("HTTP client 重建失败（配置已保存）: {e}");
            tracing::warn!("{msg}");
            self.push_error(msg);
        }
    }

    /// 派一个连通性检测任务。
    pub fn spawn_health_check(&mut self) {
        if self.rules.is_empty() {
            self.push_warning(ts("Toasts.no_sources_detected"));
            return;
        }
        spawn_health_check(
            &self.rules,
            Arc::clone(&self.http),
            self.runtime,
            &mut self.sources_state,
        );
    }

    /// 手动检查 GitHub release 是否有新版本。
    pub fn spawn_update_check(&mut self) {
        spawn_update_check(
            &self.config,
            Arc::clone(&self.http),
            self.runtime,
            &mut self.update_state,
        );
    }

    /// 扫描下载目录。
    pub fn refresh_library(&mut self) {
        refresh_library(&mut self.library, &self.config.download_path);
    }

    /// 异步扫描下载目录。
    ///
    /// 阻塞的 `read_dir` / `metadata` 跑在 `tokio::task::spawn_blocking`（共享 tokio
    /// runtime），结果通过 smol channel 回到主线程，由 `events::drain` 排空。
    /// 重复触发会被 `scan_in_flight` 拦截。`scanned_dir` 路径解析在主线程做（轻量）。
    pub fn refresh_library_async(&mut self) {
        if self.library.scan_in_flight {
            return;
        }
        let dir_raw = std::path::PathBuf::from(&self.config.download_path);
        let abs = if dir_raw.is_absolute() {
            dir_raw
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(&dir_raw))
                .unwrap_or(dir_raw)
        };

        // 先在主线程重置 entries / scanned_dir / pending_delete（轻量、即时反馈）。
        self.library.scanned_dir = Some(abs.clone());
        self.library.entries.clear();
        self.library.last_error = None;
        self.library.pending_delete = None;

        if !abs.exists() {
            return;
        }

        let (tx, rx) = smol::channel::unbounded::<crate::app::library_state::LibraryScanEvent>();
        self.library.scan_rx = Some(rx);
        self.library.scan_in_flight = true;

        // 借用 self.runtime 启动 spawn_blocking，调用阻塞的 std::fs —— 共享 tokio
        // runtime（已 leaked），进程结束才 drop。
        let runtime = self.runtime;
        runtime.spawn(async move {
            // tokio 的 spawn_blocking 隔离阻塞 IO，不阻塞 reactor。
            let result = tokio::task::spawn_blocking(move || {
                crate::app::library_state::scan_library_dir(&abs)
            })
            .await;
            let event = match result {
                Ok(Ok(entries)) => Ok(entries),
                Ok(Err(io_err)) => Err(crate::i18n::ts_fmt(
                    "Toasts.library_scan_failed",
                    &[("err", &io_err.to_string())],
                )
                .to_string()),
                Err(join_err) => Err(format!("scan task join failed: {join_err}")),
            };
            // receiver 可能已被 drop（AppModel 销毁）—— send 在 channel 关闭时
            // 静默失败，符合"没人听就不发"原则。
            let _ = tx.send(event).await;
        });
    }

    /// 真正删除一个本地文件。
    pub fn delete_library_entry(&mut self, path: &std::path::Path) {
        match delete_library_entry(&mut self.library, &self.config.download_path, path) {
            Ok(msg) if !msg.is_empty() => self.push_success(msg),
            Ok(_) => {}
            Err(msg) => self.push_error(msg),
        }
    }

    /// 清掉所有已结束的任务。
    pub fn clear_finished_tasks(&mut self) {
        let before = self.tasks.len();
        clear_finished_tasks(&mut self.tasks);
        let removed = before - self.tasks.len();
        if removed > 0 {
            self.save_tasks_to_file();
            self.push_success(ts_fmt(
                "Toasts.clear_tasks_ok",
                &[("n", &removed.to_string())],
            ));
        }
    }

    /// 删除单条任务记录（仅已结束的，运行中跳过）。
    ///
    /// 内存 `tasks` retain 移除 + 文件保存。运行中的任务不能删（会留下孤儿后台
    /// 任务 + cancel token 丢失），调用方（UI）本就只对已结束任务显示删除按钮，这里再兜底。
    /// 返回是否真的删了（false = 任务还在跑或不存在）。
    pub fn delete_task(&mut self, id: u64) -> bool {
        // 兜底：运行中的不删；不存在的也跳过。
        let Some(task) = self.tasks.iter().find(|t| t.id == id) else {
            return false;
        };
        if task.is_running() {
            return false;
        }
        self.tasks.retain(|t| t.id != id);
        self.save_tasks_to_file();
        true
    }
}
