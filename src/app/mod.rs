//! 应用状态、状态结构体、业务方法集合。
//!
//! 拆分子模块：
//! - `download_task`  / `search_state` / `library_state` / `sources_state` / `update_state` — 5 个状态结构体
//! - `cover` — UI 辅助（封面字节解码 + URI 生成）
//! - `now` / `runtime` / `tasks_db` — 自由辅助
//! - `crate::app::ops::download` / `crate::app::ops::search` / `crate::app::ops::sources` / `crate::app::ops::library` / `crate::app::ops::update` / `crate::app::ops::settings` — 业务方法
//!
//! 入口：`AppModel` 持有所有状态 struct 实例，UI 中立（不依赖任何 GUI 框架）。
//! 后台通道排空 + UI 重绘触发由 `crate::app::events` 负责。

mod cover;
mod download_task;
pub(crate) mod events;
mod library_state;
mod now;
mod runtime;
mod search_state;
mod sources_state;
pub use sources_state::SourcesFilterStatus;
mod tasks_db;
mod ui_event;
mod update_state;

pub(crate) mod ops;

pub use cover::{CoverEntry, hash_short};
pub use download_task::DownloadTask;
pub use library_state::{LibraryEntry, LibraryState, scan_library_dir};
pub use now::now_unix_secs;
pub use runtime::build_shared_runtime;
pub use search_state::{
    CoverEvent, DetailEvent, DetailState, SearchState, SourceSearchEvent, SourceStatus, TocEvent,
    TocState,
};
pub use sources_state::SourcesState;
pub use tasks_db::load_tasks_from_db;
pub use ui_event::UIEvent;
pub use update_state::{
    UpdateCheckResult, UpdateOutcome, UpdateState, check_github_latest_release,
};

use anyhow::{Context, Result};
use tokio::runtime::Runtime;

use crate::config::{AppConfig, ConfigPaths, load_config};
use crate::db::Db;
use crate::i18n::{ts, ts_fmt};
use crate::models::{Book, Chapter, Rule, SearchResult};
use crate::rules::SourceOverrides;

/// 应用整体状态。UI 中立结构 —— 不依赖任何 GUI 框架，由 `gpui_app` 层渲染。
pub struct AppModel {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub rules: Vec<Rule>,
    pub rule_load_error: Option<String>,
    pub config_load_error: Option<String>,

    /// 用户对书源的禁用 / 启用覆写。toggle 后立即写 `sonovel.db`
    /// 的 `source_overrides` 表；UI 这里持有的副本仅用于显示状态。
    pub source_overrides: SourceOverrides,

    /// 持久化层（SQLite）。下载任务记录全走这里。
    pub db: Db,

    /// 后台任务运行时。所有 spawn 都走它。
    /// 通过 `Box::leak` 得到 `&'static Runtime`，永不 drop ——
    /// 见 `build_shared_runtime` 注释，规避 Runtime drop panic。
    pub runtime: &'static Runtime,

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
}

impl AppModel {
    /// UI 中立的构造函数。
    ///
    /// 返回 `Result`：磁盘 DB 与内存 DB 都打不开时（极少见，通常是磁盘权限或
    /// sqlite 损坏到连 in-memory 都建不出来），调用方应捕获并向用户展示
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

        let runtime = build_shared_runtime();

        let mut db = match Db::open(&paths.db_file) {
            Ok(db) => db,
            Err(e) => {
                tracing::warn!("sonovel.db 打开失败，回退到内存 DB（重启会丢失任务记录）: {e:#}");
                // in-memory DB 创建在标准 rusqlite 几乎不会失败；如果失败，
                // 把错误向上抛，让 gpui_app 入口弹致命错误对话框。
                Db::open_in_memory().with_context(|| {
                    format!("无法初始化持久化数据库：磁盘 DB 打开失败，且内存 DB 也建不出来：{e:#}")
                })?
            }
        };

        let (rules, rule_load_error) = match crate::rules::load_rules_from_db(db.conn_mut()) {
            Ok(rs) => (rs, None),
            Err(e) => {
                tracing::warn!("rules load failed: {e:#}");
                (Vec::new(), Some(format!("{e:#}")))
            }
        };
        let source_overrides = SourceOverrides::load_from_db(db.conn());

        let (tasks, next_task_id) = load_tasks_from_db(&db);
        tracing::info!("从 DB 加载 {} 个历史下载任务", tasks.len());

        let search = SearchState::default();

        Ok(Self {
            paths,
            config,
            rules,
            rule_load_error,
            config_load_error,
            source_overrides,
            db,
            runtime,
            search,
            tasks,
            next_task_id,
            library: LibraryState::default(),
            sources_state: SourcesState::default(),
            update_state: UpdateState::default(),
            pending_notifications: Vec::new(),
        })
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

    /// 派一个新的下载任务。返回新任务 id。
    pub fn spawn_download(&mut self, target: SearchResult) -> u64 {
        let (id, task) = crate::app::ops::spawn_download(
            &self.rules,
            &self.config,
            self.runtime,
            &mut self.next_task_id,
            target,
        );
        crate::app::ops::save_task_to_db(&self.db, &task);
        self.tasks.push(task);
        id
    }

    /// 派一个 TOC 预取任务（获取元数据 + 章节列表，不开始下载）。
    pub fn spawn_resolve_toc(&mut self, target: &SearchResult) {
        let rx =
            crate::app::ops::spawn_resolve_toc(&self.rules, &self.config, self.runtime, target);
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
        let (id, task) = crate::app::ops::spawn_download_range(
            &self.rules,
            &self.config,
            self.runtime,
            &mut self.next_task_id,
            target,
            book,
            chapters,
        );
        crate::app::ops::save_task_to_db(&self.db, &task);
        self.tasks.push(task);
        id
    }

    /// 派聚合搜索任务。
    pub fn spawn_search(&mut self) -> bool {
        crate::app::ops::spawn_search(&self.rules, &self.config, self.runtime, &mut self.search)
    }

    /// 选中某条搜索结果。
    pub fn select_search_result(&mut self, idx: usize) {
        crate::app::ops::select_search_result(
            &self.rules,
            &self.config,
            self.runtime,
            &mut self.search,
            idx,
        );
    }

    /// 切换书源禁用状态。
    pub fn toggle_source_disabled(&mut self, source_id: i32) {
        crate::app::ops::toggle_source_disabled(
            &self.db,
            &mut self.source_overrides,
            &mut self.rules,
            source_id,
        );
    }

    /// 从 JSON 文件导入书源。
    ///
    /// 自动按 `url` 去重 —— DB 已有的 url（忽略大小写、首尾空格）跳过，不重复插入。
    /// 反馈给用户的 toast 同时显示"导入 X / 跳过 Y"。
    pub fn add_sources_from_file(&mut self, path: &std::path::Path) {
        match crate::app::ops::add_sources_from_file(
            &mut self.db,
            &mut self.rules,
            &mut self.rule_load_error,
            path,
        ) {
            Ok(result) => {
                // i18n 模板：`"Imported {inserted}, skipped {skipped} duplicates"`
                // 全部重复（inserted=0）降级为 warning，否则 success。
                let msg = crate::i18n::ts_fmt(
                    "Sources.import.result",
                    &[
                        ("inserted", &result.inserted.to_string()),
                        ("skipped", &result.skipped.to_string()),
                    ],
                )
                .to_string();
                if result.inserted == 0 && result.skipped > 0 {
                    self.push_warning(msg);
                } else {
                    self.push_success(msg);
                }
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
    pub fn delete_source(&mut self, source_id: i32) {
        match crate::app::ops::delete_source(
            &mut self.db,
            &mut self.rules,
            &mut self.source_overrides,
            &mut self.sources_state,
            source_id,
        ) {
            Ok(true) => self.push_success(ts_fmt(
                "Toasts.delete_source_ok",
                &[("id", &source_id.to_string())],
            )),
            Ok(false) => self.push_warning(ts("Toasts.delete_source_missing")),
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
        if let Err(msg) = crate::app::ops::persist_settings(&self.config, &self.paths.config_file) {
            tracing::warn!("自动保存 config.toml 失败: {msg}");
            self.push_error(msg);
        } else {
            tracing::debug!("config.toml 自动保存成功");
        }
    }

    /// 派一个连通性检测任务。
    pub fn spawn_health_check(&mut self) {
        if self.rules.is_empty() {
            self.push_warning(ts("Toasts.no_sources_detected"));
            return;
        }
        crate::app::ops::spawn_health_check(
            &self.rules,
            &self.config,
            self.runtime,
            &mut self.sources_state,
        );
    }

    /// 手动检查 GitHub release 是否有新版本。
    pub fn spawn_update_check(&mut self) {
        crate::app::ops::spawn_update_check(&self.config, self.runtime, &mut self.update_state);
    }

    /// 扫描下载目录。
    pub fn refresh_library(&mut self) {
        crate::app::ops::refresh_library(&mut self.library, &self.config.download_path);
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
        match crate::app::ops::delete_library_entry(
            &mut self.library,
            &self.config.download_path,
            path,
        ) {
            Ok(msg) if !msg.is_empty() => self.push_success(msg),
            Ok(_) => {}
            Err(msg) => self.push_error(msg),
        }
    }

    /// 清掉所有已结束的任务。
    pub fn clear_finished_tasks(&mut self) {
        let before = self.tasks.len();
        crate::app::ops::clear_finished_tasks(&mut self.tasks, &self.db);
        let removed = before - self.tasks.len();
        if removed > 0 {
            self.push_success(ts_fmt(
                "Toasts.clear_tasks_ok",
                &[("n", &removed.to_string())],
            ));
        }
    }

    /// 删除单条任务记录（仅已结束的，运行中跳过）。
    ///
    /// 内存 `tasks` retain 移除 + DB `delete_one`。运行中的任务不能删（会留下孤儿后台
    /// 任务 + cancel token 丢失），调用方（UI）本就只对已结束任务显示删除按钮，这里再兜底。
    /// 返回是否真的删了（false = 任务还在跑或不存在）。
    pub fn delete_task(&mut self, id: u64) -> bool {
        // 兜底：运行中的不删。
        if self.tasks.iter().any(|t| t.id == id && t.is_running()) {
            return false;
        }
        let existed = self.tasks.iter().any(|t| t.id == id);
        if !existed {
            return false;
        }
        self.tasks.retain(|t| t.id != id);
        if let Err(e) = crate::db::tasks::delete_one(self.db.conn(), id) {
            tracing::warn!("delete_task db delete_one failed for id={id}: {e:#}");
        }
        true
    }

    /// 把单条任务 upsert 到 DB。
    pub fn save_task_to_db(&self, task: &DownloadTask) {
        crate::app::ops::save_task_to_db(&self.db, task);
    }
}
