//! `SearchDelegate`: gpui-component List delegate，持有 page handle + 当前页 (index, `SearchResult`)。

use gpui::{App, Context, Entity, ParentElement, Styled, Window, px};
use gpui_component::list::{ListItem, ListState};
use gpui_component::{ActiveTheme as _, IndexPath, list::ListDelegate};

use crate::models::SearchResult;

use super::SearchPage;
use super::result_row;

/// `gpui-component::List` 的 delegate —— 把当前过滤下的 (index, `SearchResult`) 列表渲染成行。
///
/// 完全对齐 `library::LibraryDelegate` / `tasks::TasksDelegate` / `sources::SourcesDelegate` 模式：
/// - `page_items` 由 `SearchPage::render` 在每帧 render 前写入；`render_item` 直接取。
/// - 持有 `Entity<SearchPage>` handle 以便 row 内的详情/选章/全本按钮 → page 转发。
/// - 选中态交给 `ListItem::selected(...)` + `set_selected_index` 配对管理。
pub(super) struct SearchDelegate {
    /// 当前页要展示的条目，每条带"全局序号"（在完整 results 列表里的 0-based 位置）。
    /// 跨分页连续：page 0 → 0..29，page 1 → 30..59，等等。显示时 +1 变 1-based。
    pub(super) page_items: Vec<(usize, SearchResult)>,
    /// 当前选中项。`None` = 未选中。`set_selected_index` 写入，`render_item` 读出来
    /// 给 `ListItem::selected(...)` 用。
    pub(super) selected_index: Option<IndexPath>,
    /// 拿 `SearchPage` handle 用于按钮 `on_click` → 转发回 page。
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
