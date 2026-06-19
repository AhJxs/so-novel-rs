//! TasksDelegate: gpui-component List delegate，持有 page handle + 当前页 TaskSummary。

use gpui::{App, Context, Entity, ParentElement, Styled, Window};
use gpui_component::list::{ListItem, ListState};
use gpui_component::{ActiveTheme as _, IndexPath, list::ListDelegate};

use super::TasksPage;
use super::row;
use super::summary::TaskSummary;

/// `gpui-component::List` 的 delegate —— 把当前过滤下的 `TaskSummary` 列表渲染成行。
pub(super) struct TasksDelegate {
    /// 当前过滤下要展示的任务。
    pub(super) page_items: Vec<TaskSummary>,
    /// 当前选中项。`None` = 未选中。
    pub(super) selected_index: Option<IndexPath>,
    /// 拿 TasksPage handle 用于动作按钮 → 转发回 page。
    pub(super) page_handle: Entity<TasksPage>,
}

impl TasksDelegate {
    pub(super) fn new(page_handle: Entity<TasksPage>) -> Self {
        Self {
            page_items: Vec::new(),
            selected_index: None,
            page_handle,
        }
    }
}

impl ListDelegate for TasksDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.page_items.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let task = self.page_items.get(ix.row)?.clone();
        Some(
            ListItem::new(ix)
                .selected(Some(ix) == self.selected_index)
                .rounded(cx.theme().radius)
                .mb(gpui::px(4.))
                .child(row::render(task, self.page_handle.clone(), &mut *cx)),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected_index = ix;
        cx.notify();
    }
}
