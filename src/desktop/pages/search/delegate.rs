//! `SearchDelegate`: `gpui_kit::component::list::List` 的 delegate，持有 page handle + 当前页条目。

use gpui_kit::component::list::{ListItem, ListState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, list::ListDelegate};
use gpui_kit::{App, Context, Entity, ParentElement, Styled, Window, px};

use crate::models::SearchResult;

use super::SearchPage;
use super::result_row;

/// `List` 的 delegate —— 把当前页的 (index, `SearchResult`) 渲染成行。
///
/// 与 library / tasks / sources 的 delegate 同模式：`page_items` 由 `SearchPage::render`
/// 每帧写入，`render_item` 直接取；选中态由 `ListItem::selected` + `set_selected_index` 配对管。
pub(super) struct SearchDelegate {
    /// 当前页条目，每条带全局 0-based 序号（跨分页连续）。显示时 +1。
    pub(super) page_items: Vec<(usize, SearchResult)>,
    /// 当前选中项，`None` = 未选中。
    pub(super) selected_index: Option<IndexPath>,
    /// 给按钮 `on_click` 转发回 page 用。
    pub(super) page: Entity<SearchPage>,
}

impl SearchDelegate {
    pub(super) const fn new(page: Entity<SearchPage>) -> Self {
        Self {
            page_items: Vec::new(),
            selected_index: None,
            page,
        }
    }
}

impl ListDelegate for SearchDelegate {
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
        let (global_index, result) = self.page_items.get(ix.row)?.clone();
        let page = self.page.clone();
        Some(
            ListItem::new(ix)
                .selected(Some(ix) == self.selected_index)
                .rounded(cx.theme().radius)
                .mb(px(4.))
                .child(result_row::render(global_index, &result, page, &*cx)),
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
