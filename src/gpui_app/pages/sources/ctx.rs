//! Sources page 子模块共享的"借用视图" ctx。
//!
//! 跟 `settings::ctx::PageCtx` / `library::ctx::LibraryCtx` / `tasks::ctx::TasksCtx`
//! 同模式：每个 page 自建一个，不上 generic（字段集每个 page 都不同，generic 徒增 trait bound）。
//!
//! 当前子模块（toolbar/row/delegate）大多直接收 `&Entity<...>` 形式参数，跟 spec 的
//! "统一 ctx 借用视图"略有偏差 —— 留着类型避免后续 refactor 再造。
//! 统一先 `#[allow(dead_code)]`。

use gpui::Entity;
use gpui::SharedString;
use gpui_component::input::InputState;
use gpui_component::list::ListState;

use crate::app::{AppModel, SourcesFilterStatus};

use super::SourcesDelegate;

/// `SourcesPage` 字段的借用视图，递给各子模块的 `render(ctx, ...)`。
#[allow(dead_code)]
pub(super) struct SourcesCtx<'a> {
    pub model: &'a Entity<AppModel>,
    pub filter_input: &'a Entity<InputState>,
    pub list_state: &'a Entity<ListState<SourcesDelegate>>,
    pub current_status: SourcesFilterStatus,
    /// i18n sentinel：上次 render 时 `Sources.filter.placeholder` 的翻译结果。
    /// 切语言 → `ts()` 返回新值 → render 里检测到不一致 → `set_placeholder` 刷新。
    pub last_seen_placeholder: &'a SharedString,
}
