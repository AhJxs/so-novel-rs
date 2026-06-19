//! Tasks page 子模块共享的"借用视图" ctx。
//!
//! 跟 `settings::ctx::PageCtx` / `library::ctx::LibraryCtx` 同模式：每个 page 自建一个，
//! 不上 generic（字段集每个 page 都不同，generic 徒增 trait bound）。

use gpui::Entity;
use gpui_component::list::ListState;

use crate::app::AppModel;

use super::TasksDelegate;
use super::summary::TaskFilter;

/// `TasksPage` 字段的借用视图，递给各子模块的 `render(ctx, ...)`。
#[allow(dead_code)]
pub(super) struct TasksCtx<'a> {
    pub model: &'a Entity<AppModel>,
    pub list_state: &'a Entity<ListState<TasksDelegate>>,
    pub filter: TaskFilter,
    pub current_page: &'a mut usize,
}
