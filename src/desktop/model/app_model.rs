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
use crate::models::{DownloadTaskRecord, Rule};

/// 应用整体状态。UI 中立 —— 不依赖任何 GUI 框架, 由 `desktop` 层渲染。
///
/// 字段都是 `pub(crate)`：它们是界面之间共享的运行时状态，不是对外承诺的 API。读写约定
/// 是「页面只读、`ops` 层改」—— 所有会落盘或触发后台任务的改动都走
/// [`super::ops`] 里的事务函数，不要在页面里就地赋值。
pub struct AppModel {
    /// 配置与数据文件路径。
    pub(crate) paths: ConfigPaths,
    /// 已加载的应用配置。
    pub(crate) config: AppConfig,
    /// 当前活跃的书源规则；派发的 `Source` 与本列表共享同一批 `Arc`。
    pub(crate) rules: Vec<Arc<Rule>>,
    /// 书源加载失败的原因；成功时为 `None`。
    pub(crate) rule_load_error: Option<String>,

    /// 书源配置：活跃文件选择 + 禁用列表。
    pub(crate) sources_config: SourcesConfig,
    /// 运行数据存储（书架 / 下载任务 / 书源选择）。
    pub(crate) state: StateStore,
    /// 后台抓取共用的 Tokio 运行时。
    pub(crate) runtime: &'static Runtime,

    /// 共享 HTTP client 集合。复用连接池 + TLS session cache; 改 proxy / `unsafe_ssl` 时
    /// `HttpClients::rebuild_proxy` 整体替换实例。后台任务在 `spawn` 前用
    /// `HttpClients::for_rule` 取出 owned `reqwest::Client`, 不持有本字段的引用。
    pub(crate) http: HttpClients,

    /// 最近一次系统代理探测结果（设置页展示用）。
    ///
    /// 启动时探一次，之后由 [`Self::refresh_system_proxy`] 在「改代理模式」和「进入设置页」
    /// 两处刷新 —— 不做后台轮询，用户在 Clash 里改完要下次触发才生效。
    pub(crate) system_proxy: SystemProxy,

    /// 搜索页状态。
    pub(crate) search: SearchState,

    /// 活动 / 已完成的下载任务。最新加在末尾。
    pub(crate) tasks: Vec<DownloadTask>,
    pub(super) next_task_id: u64,
    /// 加载时 `state:tasks` 解析失败（原文已备份）：本次运行不再覆盖写。
    pub(super) tasks_readonly: bool,
    /// 加载时 `state:sources` 解析失败（原文已备份）：本次运行不再覆盖写。
    pub(super) sources_readonly: bool,
    /// 只读告警是否已发过，避免每次落盘都刷日志。
    pub(super) tasks_readonly_warned: bool,
    /// 只读告警是否已发过，避免每次落盘都刷日志。
    pub(super) sources_readonly_warned: bool,

    /// 下载文件库页状态。
    pub(crate) library: LibraryState,

    /// 书源管理页状态（连通性检测结果）。
    pub(crate) sources_state: SourcesState,
    /// 版本更新检查状态。
    pub(crate) update_state: UpdateState,

    /// 业务层 → UI 层的 [`UIEvent`] 队列, 由 `RootView::render` 排空并翻译成 notification。
    pub(crate) pending_ui_events: Vec<UIEvent>,

    /// 各列表的渲染缓存。
    pub(crate) list_cache: ListCache,
    /// 后台线程唤醒 UI 的句柄。
    pub(crate) wakeup: WakeupHandle,
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

        // 拿到运行数据目录 = 本进程是唯一实例，此时才能安全清理上次自更新留下的
        // `<exe>.old` / 暂存包（第二个实例若先清理会误删正在运行的下载）。
        crate::update::cleanup_after_update();

        // 2. 书源选择要先于规则加载：规则目录按活跃书源文件解析。
        let (sources_config, sources_readonly) =
            load_sources_config(&state, snapshot.sources.as_deref());
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
        let (LoadedTasks { tasks, max_id }, tasks_rewrite, tasks_readonly) =
            load_tasks(&state, snapshot.tasks.as_deref());
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
            sources_config,
            state,
            runtime,
            http,
            system_proxy,
            search: SearchState::default(),
            tasks,
            next_task_id,
            tasks_readonly,
            sources_readonly,
            tasks_readonly_warned: false,
            sources_readonly_warned: false,
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

/// 读取书源选择。解析失败时先把原始字节备份进 state 目录，再回落默认值。
///
/// 返回的 bool 表示「本次运行拒写 sources」：损坏原文已备份，若再把默认值写回，
/// 下次启动就看不到用户原来的选择了（与 [`LibraryStore`] 的 `readonly` 同语义）。
fn load_sources_config(state: &StateStore, bytes: Option<&[u8]>) -> (SourcesConfig, bool) {
    let Some(bytes) = bytes else {
        return (SourcesConfig::default(), false);
    };
    match serde_json::from_slice(bytes) {
        Ok(config) => (config, false),
        Err(error) => {
            tracing::warn!("书源选择数据解析失败，本次运行不再覆盖写入: {error}");
            backup_corrupt(state, "sources", bytes);
            (SourcesConfig::default(), true)
        }
    }
}

/// 读取下载任务。解析失败时先备份原文，再回落空表 + 本次运行拒写 tasks。
fn load_tasks(state: &StateStore, bytes: Option<&[u8]>) -> (LoadedTasks, Option<Vec<u8>>, bool) {
    if let Some(bytes) = bytes
        && serde_json::from_slice::<Vec<DownloadTaskRecord>>(bytes).is_err()
    {
        tracing::warn!("下载任务数据解析失败，本次运行不再覆盖写入");
        backup_corrupt(state, "tasks", bytes);
        return (
            LoadedTasks {
                tasks: Vec::new(),
                max_id: 0,
            },
            None,
            true,
        );
    }
    let (loaded, rewrite) = load_tasks_from_bytes(bytes);
    (loaded, rewrite, false)
}

/// 把无法解析的 blob 原文另存一份；备份失败也只记日志，只读标志照样生效。
fn backup_corrupt(state: &StateStore, name: &str, bytes: &[u8]) {
    if let Some(path) = state.backup_corrupt(name, bytes) {
        tracing::warn!("损坏的 {name} 数据已备份到 {}", path.display());
    } else {
        tracing::warn!("备份损坏的 {name} 数据失败，仍按只读处理");
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::{Path, PathBuf};

    use super::*;

    /// 临时目录里的真实 `StateStore`（不触碰 `~/.sonovel`）。
    fn temp_state() -> (tempfile::TempDir, StateStore) {
        let dir = tempfile::tempdir().unwrap();
        let (state, _) =
            StateStore::open(dir.path().to_path_buf(), build_shared_runtime().unwrap()).unwrap();
        (dir, state)
    }

    fn backups(dir: &Path, name: &str) -> Vec<PathBuf> {
        let prefix = format!("{name}.corrupt-");
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|file| file.to_string_lossy().starts_with(&prefix))
            })
            .collect()
    }

    #[test]
    fn corrupt_sources_blob_is_backed_up_and_readonly() {
        let (dir, state) = temp_state();
        let (config, readonly) = load_sources_config(&state, Some(b"{ not json"));

        assert!(readonly, "解析失败应标记本次运行拒写");
        assert_eq!(config.active_file, SourcesConfig::default().active_file);
        let backups = backups(dir.path(), "sources");
        assert_eq!(backups.len(), 1, "原文必须留一份备份");
        assert_eq!(std::fs::read(&backups[0]).unwrap(), b"{ not json");
        state.close();
    }

    #[test]
    fn valid_sources_blob_parses_and_is_writable() {
        let (_dir, state) = temp_state();
        let bytes = SourcesConfig {
            active_file: "cloudflare.json".to_string(),
            ..SourcesConfig::default()
        }
        .to_json()
        .unwrap();

        let (config, readonly) = load_sources_config(&state, Some(&bytes));
        assert!(!readonly);
        assert_eq!(config.active_file, "cloudflare.json");
        state.close();
    }

    #[test]
    fn corrupt_tasks_blob_is_backed_up_and_readonly() {
        let (dir, state) = temp_state();
        let (loaded, rewrite, readonly) = load_tasks(&state, Some(b"{ not json"));

        assert!(readonly, "解析失败应标记本次运行拒写");
        assert!(loaded.tasks.is_empty(), "损坏数据回落空表");
        assert_eq!(loaded.max_id, 0);
        assert!(rewrite.is_none());
        let backups = backups(dir.path(), "tasks");
        assert_eq!(backups.len(), 1, "原文必须留一份备份");
        assert_eq!(std::fs::read(&backups[0]).unwrap(), b"{ not json");
        state.close();
    }

    #[test]
    fn valid_tasks_blob_loads_without_readonly() {
        let (_dir, state) = temp_state();
        let bytes = serde_json::to_vec(&[DownloadTaskRecord {
            id: 7,
            origin: crate::models::SearchResult::default(),
            started_at_unix: 1,
            finished_at_unix: Some(2),
            book_meta: None,
            total_chapters: 1,
            completed: 1,
            failed: 0,
            last_chapter_title: String::new(),
            finished: Some(Ok(std::path::PathBuf::from("/tmp/a.epub"))),
            failures: Vec::new(),
        }])
        .unwrap();

        let (loaded, rewrite, readonly) = load_tasks(&state, Some(&bytes));
        assert!(!readonly);
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.max_id, 7);
        assert!(rewrite.is_none());
        state.close();
    }
}
