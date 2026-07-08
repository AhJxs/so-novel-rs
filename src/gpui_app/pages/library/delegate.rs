//! `LibraryDelegate`: gpui-component List delegate，持有 page handle + 当前页 (index, `LibraryEntry`)。

use gpui::{App, Context, Entity, ParentElement, Styled, Window, px};
use gpui_component::list::{ListItem, ListState};
use gpui_component::{ActiveTheme as _, IndexPath, list::ListDelegate};

use crate::app::LibraryEntry;

use super::LibraryPage;
use super::row;

/// `gpui-component::List` 的 delegate —— 把当前页的 `LibraryEntry` 切片渲染成行。
/// 完全对齐 `tasks::TasksDelegate` / `sources::SourcesDelegate` / `search::SearchDelegate`
/// 模式（PR6 抽出来后 4 个 page 共用一套 delegate 结构）。
pub(super) struct LibraryDelegate {
    pub(super) page: Entity<LibraryPage>,
    pub(super) page_items: Vec<(usize, LibraryEntry)>,
    pub(super) selected_index: Option<IndexPath>,
}

impl LibraryDelegate {
    pub(super) const fn new(page: Entity<LibraryPage>) -> Self {
        Self {
            page,
            page_items: Vec::new(),
            selected_index: None,
        }
    }
}

impl ListDelegate for LibraryDelegate {
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
        let (global_index, entry) = self.page_items.get(ix.row)?.clone();
        Some(
            ListItem::new(ix)
                .selected(Some(ix) == self.selected_index)
                .rounded(cx.theme().radius)
                .mb(px(4.))
                .child(row::render_row(global_index, &entry, &self.page, &*cx)),
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
