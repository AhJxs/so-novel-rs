//! 应用状态、状态结构体、业务方法集合。
//!
//! 拆分子模块：
//! - `download_task`  / `search_state` / `library_state` / `sources_state` / `update_state` — 5 个状态结构体
//! - `cover` — UI 辅助（封面字节解码 + URI 生成）
//! - `now` / `runtime` / `tasks_db` — 自由辅助
//! - `crate::app::ops::download` / `crate::app::ops::search` / `crate::app::ops::sources` / `crate::app::ops::library` / `crate::app::ops::update` / `crate::app::ops::settings` — 业务方法
//!
//! 入口：`AppModel`（Stage 2 起替代旧的 `SoNovelApp`）持有所有状态 struct 实例。
//!
//! Stage 2 起为 GPUI 迁移做改造：
//! - 旧的 `SoNovelApp` 名字已重命名为 `AppModel`。
//! - 不再 `impl eframe::App`；帧轮询逻辑迁到 `crate::app::events`（Stage 3）。

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
mod update_state;

pub(crate) mod ops;

pub use cover::{hash_short, CoverEntry};
pub use download_task::DownloadTask;
pub use library_state::{scan_library_dir, LibraryEntry, LibraryState};
pub use now::now_unix_secs;
pub use runtime::build_shared_runtime;
pub use search_state::{
    CoverEvent, DetailEvent, DetailState, SearchState, SourceSearchEvent, SourceStatus, TocEvent,
    TocState,
};
pub use sources_state::SourcesState;
pub use tasks_db::load_tasks_from_db;
pub use update_state::{
    check_github_latest_release, UpdateCheckResult, UpdateOutcome, UpdateState,
};

use tokio::runtime::Runtime;

use crate::config::{load_config, AppConfig, ConfigPaths};
use crate::db::Db;
use crate::models::Rule;

/// 应用整体状态。Stage 2 起为 UI 中立结构（不再 `impl eframe::App`）。
///
/// 字段含义未变（仅名字从 `SoNovelApp` → `AppModel`），便于逐 stage 渐进迁移。
pub struct AppModel {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub rules: Vec<Rule>,
    pub rule_load_error: Option<String>,
    pub config_load_error: Option<String>,

    /// 用户对书源的禁用 / 启用覆写。toggle 后立即写 `sonovel.db`
    /// 的 `source_overrides` 表；UI 这里持有的副本仅用于显示状态。
    pub source_overrides: crate::rules::SourceOverrides,

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
    /// 解法：drain 把构造好的 [`gpui_component::notification::Notification`] 推到这个 Vec，
    /// 由 `RootView::render`（拿得到 `&mut Window`）排空 + 调 `push_notification`。
    pub pending_notifications: Vec<gpui_component::notification::Notification>,
}

impl Default for AppModel {
    fn default() -> Self {
        Self::new()
    }
}

impl AppModel {
    /// UI 中立的构造函数。
    pub fn new() -> Self {
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

        let db = match Db::open(&paths.db_file) {
            Ok(db) => db,
            Err(e) => {
                tracing::warn!("sonovel.db 打开失败: {e:#}");
                Db::open_in_memory().unwrap_or_else(|e| {
                    panic!("既开不了磁盘 DB 也开不了内存 DB：{e}")
                })
            }
        };

        let (rules, rule_load_error) = match crate::rules::load_rules_from_db(db.conn()) {
            Ok(rs) => (rs, None),
            Err(e) => {
                tracing::warn!("rules load failed: {e:#}");
                (Vec::new(), Some(format!("{e:#}")))
            }
        };
        let source_overrides = crate::rules::SourceOverrides::load_from_db(db.conn());

        let (tasks, next_task_id) = load_tasks_from_db(&db);
        tracing::info!("从 DB 加载 {} 个历史下载任务", tasks.len());

        let search = SearchState::default();

        Self {
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
        }
    }

    /// 推一条通用 [`gpui_component::notification::Notification`] 到 UI 通知队列。
    ///
    /// 适用于需要 `.on_click(...)` / `.title(...)` 等 builder 方法的复杂场景；
    /// 简单纯文本通知优先用 [`Self::push_info_notification`] /
    /// [`Self::push_success_notification`] / [`Self::push_warning_notification`] /
    /// [`Self::push_error_notification`]。
    ///
    /// 实际 `window.push_notification` 由 [`crate::gpui_app::RootView::render`] 排空
    /// `pending_notifications` 后调 —— [`crate::app::events::drain`] 跑在
    /// `AsyncApp::update_entity` 闭包里，**拿不到 `&mut Window`**。
    pub fn push_notification(
        &mut self,
        notification: gpui_component::notification::Notification,
    ) {
        self.pending_notifications.push(notification);
    }

    /// 推一条 info 级通知。语义见 [`Self::push_notification`]。
    pub fn push_info_notification(&mut self, msg: impl Into<gpui::SharedString>) {
        self.push_notification(gpui_component::notification::Notification::info(msg));
    }

    /// 推一条 success 级通知。语义见 [`Self::push_notification`]。
    pub fn push_success_notification(&mut self, msg: impl Into<gpui::SharedString>) {
        self.push_notification(gpui_component::notification::Notification::success(msg));
    }

    /// 推一条 warning 级通知。语义见 [`Self::push_notification`]。
    pub fn push_warning_notification(&mut self, msg: impl Into<gpui::SharedString>) {
        self.push_notification(gpui_component::notification::Notification::warning(msg));
    }

    /// 推一条 error 级通知。语义见 [`Self::push_notification`]。
    pub fn push_error_notification(&mut self, msg: impl Into<gpui::SharedString>) {
        self.push_notification(gpui_component::notification::Notification::error(msg));
    }

    /// 派一个新的下载任务。返回新任务 id。
    pub fn spawn_download(&mut self, target: crate::models::SearchResult) -> u64 {
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
    pub fn spawn_resolve_toc(&mut self, target: &crate::models::SearchResult) {
        let rx = crate::app::ops::spawn_resolve_toc(
            &self.rules,
            &self.config,
            self.runtime,
            target,
        );
        self.search.toc_rx = Some(rx);
    }

    /// 派一个指定章节范围的下载任务。跳过 resolve 阶段，直接进入下载。
    /// 返回新任务 id。
    pub fn spawn_download_range(
        &mut self,
        target: crate::models::SearchResult,
        book: crate::models::Book,
        chapters: Vec<crate::models::Chapter>,
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
        crate::app::ops::spawn_search(
            &self.rules,
            &self.config,
            self.runtime,
            &mut self.search,
        )
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
                let msg = crate::gpui_app::i18n::ts_fmt(
                    "Sources.import.result",
                    &[
                        ("inserted", &result.inserted.to_string()),
                        ("skipped", &result.skipped.to_string()),
                    ],
                )
                .to_string();
                if result.inserted == 0 && result.skipped > 0 {
                    self.push_warning_notification(msg);
                } else {
                    self.push_success_notification(msg);
                }
            }
            Err(msg) => {
                if msg.starts_with("文件内容为空")
                    || msg.starts_with("文件中未找到有效")
                {
                    self.push_warning_notification(msg);
                } else {
                    self.push_error_notification(msg);
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
            Ok(true) => self.push_success_notification(format!("已删除书源 #{source_id}")),
            Ok(false) => self.push_warning_notification("书源已不存在"),
            Err(msg) => self.push_error_notification(msg),
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
        if let Err(msg) = crate::app::ops::persist_settings(&self.config, &self.paths.config_file)
        {
            tracing::warn!("自动保存 config.toml 失败: {msg}");
            self.push_error_notification(msg);
        } else {
            tracing::debug!("config.toml 自动保存成功");
        }
    }

    /// 派一个连通性检测任务。
    pub fn spawn_health_check(&mut self) {
        if self.rules.is_empty() {
            self.push_warning_notification("没有可检测的书源");
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

    /// 真正删除一个本地文件。
    pub fn delete_library_entry(&mut self, path: &std::path::Path) {
        match crate::app::ops::delete_library_entry(
            &mut self.library,
            &self.config.download_path,
            path,
        ) {
            Ok(msg) if !msg.is_empty() => self.push_success_notification(msg),
            Ok(_) => {}
            Err(msg) => self.push_error_notification(msg),
        }
    }

    /// 清掉所有已结束的任务。
    pub fn clear_finished_tasks(&mut self) {
        let before = self.tasks.len();
        crate::app::ops::clear_finished_tasks(&mut self.tasks, &self.db);
        let removed = before - self.tasks.len();
        if removed > 0 {
            self.push_success_notification(format!("已清除 {removed} 条记录"));
        }
    }

    /// 把单条任务 upsert 到 DB。
    pub fn save_task_to_db(&self, task: &DownloadTask) {
        crate::app::ops::save_task_to_db(&self.db, task);
    }
}

