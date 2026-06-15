//! 应用状态、状态结构体、业务方法集合。
//!
//! 拆分子模块：
//! - `download_task`  / `search_state` / `library_state` / `sources_state` / `update_state` — 5 个状态结构体
//! - `cover` / `toast` — UI 辅助
//! - `now` / `runtime` / `tasks_db` — 自由辅助
//! - `crate::app::ops::download` / `crate::app::ops::search` / `crate::app::ops::sources` / `crate::app::ops::library` / `crate::app::ops::update` / `crate::app::ops::settings` — 业务方法
//!
//! 入口：`super::SoNovelApp`（在 `src/app.rs` 中定义）持有所有状态 struct 实例。

mod cover;
mod download_task;
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

use std::time::{Duration, Instant};

use tokio::runtime::Runtime;

use crate::config::{load_config, AppConfig, ConfigPaths};
use crate::db::Db;
use crate::models::Rule;
use crate::ui::nav::NavPage;
use crate::design_system::{font, frame};

/// 应用整体状态。任何 UI 访问的字段都集中在这里，便于持久化与测试。
pub struct SoNovelApp {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub rules: Vec<Rule>,
    pub rule_load_error: Option<String>,
    pub config_load_error: Option<String>,

    /// 用户对书源的禁用 / 启用覆写。toggle 后立即写 `sonovel.db`
    /// 的 `source_overrides` 表；UI 这里持有的副本仅用于显示状态。
    pub source_overrides: crate::rules::SourceOverrides,

    pub current_page: NavPage,

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

    /// 是否已对 OS 窗口应用 DWM 圆角 + 沉浸式暗色。
    pub window_chrome_applied: bool,

    /// 上一帧的 dark_mode 状态。
    pub last_dark_mode: bool,

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
}

impl SoNovelApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // 注入中文字体（egui 默认字体不含 CJK，否则会显示豆腐块）。
        font::install_cjk_fonts(&cc.egui_ctx);
        // 注册 Material Symbols 圆角图标字体（vendor 在 src/material_icons/）。
        crate::material_icons::initialize(&cc.egui_ctx);
        // 安装 egui_extras 的图片 loader（PNG/JPEG/SVG/GIF/...）。
        egui_extras::install_image_loaders(&cc.egui_ctx);

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

        // 应用持久化的主题偏好
        cc.egui_ctx.set_theme(config.theme.to_theme_preference());

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
            current_page: NavPage::Search,
            settings_dirty: false,
            db,
            toast: None,
            runtime,
            window_chrome_applied: false,
            last_dark_mode: false,
            search,
            tasks,
            next_task_id,
            library: LibraryState::default(),
            sources_state: SourcesState::default(),
            update_state: UpdateState::default(),
        }
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
    pub fn persist_settings(&mut self) {
        if !self.settings_dirty {
            return;
        }
        self.settings_dirty = false;
        match crate::app::ops::persist_settings(&self.config, &self.paths.config_file) {
            Ok(_) => self.show_toast_success("已保存到 config.toml"),
            Err(msg) => {
                tracing::warn!("自动保存 config.toml 失败: {msg}");
                self.show_toast_error(msg);
                self.settings_dirty = true;
            }
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

impl eframe::App for SoNovelApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // 0. 首次 update：把 OS 窗口设为 Windows 11 圆角 + 沉浸式暗色标题栏。
        let dark = ui.ctx().global_style().visuals.dark_mode;
        let need_chrome = !self.window_chrome_applied || self.last_dark_mode != dark;
        if need_chrome {
            if let Some(hwnd) = crate::window::platform::extract_hwnd(frame) {
                crate::window::platform::apply_windows11_chrome(hwnd, dark);
            }
            self.window_chrome_applied = true;
            self.last_dark_mode = dark;
        }

        // 1. 排空所有后台通道。任何事件都触发一次 repaint。
        let mut any_progress = self.search.drain();
        let to_fetch = std::mem::take(&mut self.search.pending_cover_prefetch);
        for (sid, url) in to_fetch {
            self.search
                .spawn_cover_download(sid, &url, &self.config, self.runtime);
        }
        for t in self.tasks.iter_mut() {
            let was_running = t.is_running();
            any_progress |= t.drain();
            if was_running && !t.is_running() {
                let rec = t.to_record();
                if let Err(e) = crate::db::tasks::upsert(self.db.conn(), &rec) {
                    tracing::warn!("save task on finish failed: {e}");
                }
            }
        }
        any_progress |= self.sources_state.drain();
        if self.update_state.drain() {
            if let Some(err) = &self.update_state.error {
                self.show_toast_error(format!("检查更新失败: {err}"));
            } else if let Some(latest) = &self.update_state.latest_version {
                let current = env!("CARGO_PKG_VERSION");
                if latest.trim_start_matches('v') == current {
                    self.show_toast_success("已是最新版本");
                } else {
                    self.show_toast_warn(format!("新版本 {latest} 可用"));
                }
            }
        }
        let ctx = ui.ctx().clone();
        if any_progress {
            ctx.request_repaint();
        }
        let any_running = self.search.running
            || self.sources_state.running
            || self.tasks.iter().any(|t| t.is_running());
        if any_running {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        // 2. 渲染顶层 UI
        crate::ui::title_bar::show(ui, &ctx);

        let visuals = ctx.global_style().visuals.clone();
        egui::Panel::top("nav")
            .frame(frame::content_frame(&visuals))
            .show_inside(ui, |ui| {
                crate::ui::nav::show_in_panel(ui, self);
            });

        egui::CentralPanel::default()
            .frame(frame::content_frame(&visuals))
            .show_inside(ui, |ui| {
                crate::ui::pages::show(ui, self);
            });

        crate::ui::title_bar::handle_window_resize(&ctx);

        // toast 自动消失
        if let Some((_, _, t)) = self.toast {
            if t.elapsed() > Duration::from_secs(4) {
                self.toast = None;
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

