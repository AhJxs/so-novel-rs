//! Library page 子模块共享的"借用视图" ctx。
//!
//! 跟 `settings::ctx::PageCtx<'a>` 同模式：每个 page 自建一个，不上 generic
//! （字段集每个 page 都不同，generic 徒增 trait bound）。
//!
//! 当前子模块（toolbar/row）大多直接收 `&Entity<...>` 形式参数，跟 spec 的
//! "统一 ctx 借用视图"略有偏差 —— 留着类型避免后续 refactor 再造。
//! 统一先 `#[allow(dead_code)]`。

use std::path::PathBuf;

use gpui::Entity;
use gpui_component::input::InputState;
use gpui_component::list::ListState;

use crate::app::AppModel;

use super::LibraryDelegate;

/// Watcher 任务命令：让任务内部 drop 旧 watcher 并 arm 到新路径上。
///
/// 当前只有 `SetPath` 一个调用方（`maybe_auto_scan` 检测到 `download_path` 变了 → 发），
/// `Stop` 预留未来"暂停监听"开关使用。
#[derive(Debug, Clone)]
pub(super) enum WatcherCmd {
    SetPath(PathBuf),
    #[allow(dead_code)]
    Stop,
}

/// Sender 别名（owner 持有 → 析构时 drop → 任务 `try_recv()` 收 Closed → 退出）。
pub(super) type WatcherCmdTx = smol::channel::Sender<WatcherCmd>;

/// `LibraryPage` 字段的借用视图，递给各子模块的 `render(ctx, ...)`。
#[allow(dead_code)]
pub(super) struct LibraryCtx<'a> {
    pub model: &'a Entity<AppModel>,
    pub filter_input: &'a Entity<InputState>,
    pub list_state: &'a Entity<ListState<LibraryDelegate>>,
    /// 当前文件类型过滤。`None` = "全部"。
    pub current_ext: &'a Option<String>,
}
