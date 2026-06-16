//! 应用状态、状态结构体、业务方法集合。
//!
//! 拆分子模块：
//! - `download_task`  / `search_state` / `library_state` / `sources_state` / `update_state` — 5 个状态结构体
//! - `cover` / `toast` — UI 辅助
//! - `now` / `runtime` / `tasks_db` — 自由辅助
//! - `crate::app::ops::download` / `crate::app::ops::search` / `crate::app::ops::sources` / `crate::app::ops::library` / `crate::app::ops::update` / `crate::app::ops::settings` — 业务方法
//!
//! 入口：`AppModel`（Stage 2 起替代旧的 `SoNovelApp`）持有所有状态 struct 实例。
//!
//! Stage 2 起为 GPUI 迁移做改造：
//! - 旧的 `SoNovelApp` 名字已重命名为 `AppModel`。
//! - 不再 `impl eframe::App`；帧轮询逻辑迁到 `crate::app::events`（Stage 3）。
//! - 字体 / 图片 loader / Material Symbols 仍依赖 egui，封装在
//!   [`AppModel::install_egui_assets`] 中；旧 `crate::ui` 代码若仍在用，可单独调用。
//!   GPUI 路径不再触达该函数。
//! - `current_page` / `window_chrome_applied` / `last_dark_mode` 是旧 egui 渲染期
//!   字段，保留是给 `crate::ui` 兜底编译；Stage 11 整体删除。

mod cover;
mod download_task;
pub(crate) mod events;
mod library_state;
mod now;
mod runtime;
mod search_state;
mod sources_state;
mod tasks_db;
mod toast;
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
pub use toast::ToastKind;
pub use update_state::{check_github_latest_release, UpdateCheckResult, UpdateState};

use std::time::Instant;

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

    /// 设置页改动后尚未写盘的标记。任一控件 `.changed()` 都置 true，
    /// 设置页在 UI 末尾统一 `persist_settings()`。避免每改一个字段就写一次盘。
    pub settings_dirty: bool,

    /// 持久化层（SQLite）。下载任务记录全走这里。
    pub db: Db,

    /// 顶部状态栏的临时消息（保存成功 / 加载失败等）。
    pub toast: Option<(String, ToastKind, Instant)>,

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
    /// UI 中立的构造函数。不再需要 `eframe::CreationContext`。
    ///
    /// 旧 egui 路径需要字体 / 图片 loader 时，调用方应再调
    /// [`AppModel::install_egui_assets`]。GPUI 路径不调它。
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

        let initial_banner_dismissed = tasks.iter().map(|t| t.id).max();

        let search = SearchState {
            banner_dismissed_for: initial_banner_dismissed,
            ..SearchState::default()
        };

        Self {
            paths,
            config,
            rules,
            rule_load_error,
            config_load_error,
            source_overrides,
            settings_dirty: false,
            db,
            toast: None,
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

    /// 安装旧 egui 需要的字体 / 图片 loader / Material Symbols 字体。
    ///
    /// Stage 11 后**已删除** — 旧 egui GUI 整体移除，仅留 stub 占位以备未来若有
    /// 第三方调用方仍依赖此 API。返回 `()` 不做任何事。
    #[allow(dead_code)]
    pub fn install_egui_assets(&self) {
        // 旧实现已删除：见 git history Stage 11 之前。
    }

    pub fn show_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ToastKind::Info, Instant::now()));
    }

    pub fn show_toast_success(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ToastKind::Success, Instant::now()));
    }

    pub fn show_toast_warn(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ToastKind::Warn, Instant::now()));
    }

    pub fn show_toast_error(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ToastKind::Error, Instant::now()));
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
    pub fn add_sources_from_file(&mut self, path: &std::path::Path) {
        match crate::app::ops::add_sources_from_file(
            &mut self.db,
            &mut self.rules,
            &mut self.rule_load_error,
            path,
        ) {
            Ok(n) => self.show_toast_success(format!("已导入 {n} 个书源")),
            Err(msg) => {
                if msg.starts_with("文件内容为空")
                    || msg.starts_with("文件中未找到有效")
                {
                    self.show_toast_warn(msg);
                } else {
                    self.show_toast_error(msg);
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
            Ok(true) => self.show_toast_success(format!("已删除书源 #{source_id}")),
            Ok(false) => self.show_toast_warn("书源已不存在"),
            Err(msg) => self.show_toast_error(msg),
        }
    }

    /// 把当前 config 写回 config.toml。
    ///
    /// **Auto-save 模式**：每个 setter 改完字段后立即调本方法写盘 —— 没有"立即保存"
    /// 按钮，没有 dirty 概念。成功时静默（tracing::debug 留痕），失败时弹 error toast
    /// 让用户知道（极少见：磁盘满 / 权限问题 / 路径不存在等）。
    ///
    /// `settings_dirty` 字段保留只是为了兼容（其他代码可能读），但**本方法不再检查它**。
    /// 如果以后想加 debounce 写入（比如连续拖动 number input），可以在 setter 里加
    /// cx.spawn(timer 500ms) 合并多次 persist_settings 调用 —— 单次写盘本来就很快
    /// （小 TOML 几 ms），目前不做 debounce。
    pub fn persist_settings(&mut self) {
        self.settings_dirty = false;
        if let Err(msg) = crate::app::ops::persist_settings(&self.config, &self.paths.config_file)
        {
            tracing::warn!("自动保存 config.toml 失败: {msg}");
            self.show_toast_error(msg);
        } else {
            tracing::debug!("config.toml 自动保存成功");
        }
    }

    /// 派一个连通性检测任务。
    pub fn spawn_health_check(&mut self) {
        if self.rules.is_empty() {
            self.show_toast_warn("没有可检测的书源");
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
            Ok(msg) if !msg.is_empty() => self.show_toast_success(msg),
            Ok(_) => {}
            Err(msg) => self.show_toast_error(msg),
        }
    }

    /// 清掉所有已结束的任务。
    pub fn clear_finished_tasks(&mut self) {
        let before = self.tasks.len();
        crate::app::ops::clear_finished_tasks(&mut self.tasks, &self.db);
        let removed = before - self.tasks.len();
        if removed > 0 {
            self.show_toast_success(format!("已清除 {removed} 条记录"));
        }
    }

    /// 把单条任务 upsert 到 DB。
    pub fn save_task_to_db(&self, task: &DownloadTask) {
        crate::app::ops::save_task_to_db(&self.db, task);
    }
}

