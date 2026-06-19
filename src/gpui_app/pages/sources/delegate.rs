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
pub(super) struct SourcesDelegate {
    pub(super) page_items: Vec<(usize, Rule)>,
    pub(super) health: HashMap<i32, SourceHealth>,
    pub(super) selected_index: Option<IndexPath>,
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
