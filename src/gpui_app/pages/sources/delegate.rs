//! SourcesDelegate: gpui-component List delegate，持有 page handle + 当前页 (index, Rule) + health map。

use std::collections::HashMap;

use gpui::{App, Context, Entity, ParentElement, Styled, Window, px};
use gpui_component::list::{ListItem, ListState};
use gpui_component::{ActiveTheme as _, IndexPath, list::ListDelegate};

use crate::crawler::health::SourceHealth;
use crate::models::Rule;

use super::SourcesPage;
use super::row;

/// `gpui-component::List` 的 delegate —— 把当前过滤下的 (index, Rule) 列表渲染成行。
///
/// 完全对齐 `library::LibraryDelegate` / `tasks::TasksDelegate` 模式：
/// - `page_items` 由 `SourcesPage::render` 在每帧 render 前写入；`render_item` 直接取。
/// - `health: HashMap<i32, SourceHealth>` 也在 render 推过来 —— row 渲染时按 `rule.id` 查找。
/// - 持有 `Entity<SourcesPage>` handle 以便动作按钮 → 转发回 page。
/// - 选中态交给 `ListItem::selected(...)` + `set_selected_index` 配对管理。
pub(super) struct SourcesDelegate {
    /// 当前页要展示的条目，每条带"全局序号"（在完整 filtered 列表里的 0-based 位置）。
    /// 跨分页连续：page 0 → 0..29，page 1 → 30..59，等等。显示时 +1 变 1-based 给人看。
    pub(super) page_items: Vec<(usize, Rule)>,
    /// source_id → 探测结果（AppModel.sources_state.health 的快照）。
    /// 每次 render 推过来 —— row 渲染时按 `rule.id` 查找。
    pub(super) health: HashMap<i32, SourceHealth>,
    /// 当前选中项（List 内置 hover / selected 样式管理）。
    pub(super) selected_index: Option<IndexPath>,
    /// 拿 SourcesPage handle 用于删除按钮 → `prompt_delete` 转发。
    pub(super) page_handle: Entity<SourcesPage>,
}

impl SourcesDelegate {
    pub(super) fn new(page_handle: Entity<SourcesPage>) -> Self {
        Self {
            page_items: Vec::new(),
            health: HashMap::new(),
            selected_index: None,
            page_handle,
        }
    }
}

impl ListDelegate for SourcesDelegate {
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
        let (global_index, rule) = self.page_items.get(ix.row)?.clone();
        let health_status = self.health.get(&rule.id).cloned();
        Some(
            ListItem::new(ix)
                .selected(Some(ix) == self.selected_index)
                .rounded(cx.theme().radius)
                .mb(px(4.))
                .child(row::render(
                    global_index,
                    &rule,
                    health_status.as_ref(),
                    self.page_handle.clone(),
                    &mut *cx,
                )),
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
