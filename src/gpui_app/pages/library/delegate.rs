//! LibraryDelegate: gpui-component List delegate，持有 page handle + 当前页 (index, LibraryEntry)。

use gpui::{App, Context, Entity, ParentElement, Styled, Window, px};
use gpui_component::list::{ListItem, ListState};
use gpui_component::{ActiveTheme as _, IndexPath, list::ListDelegate};

use crate::app::LibraryEntry;

use super::LibraryPage;
use super::row;

/// `gpui-component::List` 的 delegate —— 把当前页的 `LibraryEntry` 切片渲染成行。
///
/// 设计要点（参考 `docs/docs/components/list.md`）：
/// - `page_items` 由 `LibraryPage::render` 在每帧 render 前写入；`render_item` 直接
///   从这个 Vec 里取，不重新计算过滤 / 排序 / 分页。
/// - 持有 `Entity<LibraryPage>` handle 以便 row 的删除按钮 → `LibraryPage::prompt_delete`
///   打开 dialog。
/// - **选中态完全交给 `ListItem::selected(...)` + `set_selected_index` 配对管理**：
///   - `set_selected_index(ix)` 是 List 在用户点击 / 键盘上下移动时回调的方法，
///     我们存到 `selected_index` 字段 + `cx.notify()` 触发 List 重渲染。
///   - `render_item` 通过 `ListItem::selected(Some(ix) == self.selected_index)` 把选中
///     状态交给 ListItem 内置样式（hover 背景 + selected 背景 + 边框都来自 `list_item.rs`
///     paint 逻辑，不要自己叠 h_flex + .hover + .border_b_1`）。
/// - **不要**在 row 上叠自定义 hover / selected —— ListItem 已经提供
///   `cx.theme().list_hover` / `list_active` / `list_active_border` 三套样式。
///
/// 完全对齐 `tasks::TasksDelegate` / `sources::SourcesDelegate` / `search::SearchDelegate`
/// 模式（PR6 抽出来后 4 个 page 共用一套 delegate 结构）。
pub(super) struct LibraryDelegate {
    /// 拿 LibraryPage handle 用于 row 的删除按钮 → `prompt_delete` 转发。
    pub(super) page: Entity<LibraryPage>,
    /// 当前页要展示的条目，每条带"全局序号"（在完整 filtered 列表里的 0-based 位置）。
    /// 由 `LibraryPage::render` 在每帧 render 前写入；`render_item` 直接读取。
    /// 跨分页连续：page 0 → 0..29，page 1 → 30..59，etc. —— 显示时 +1 变 1-based 给人看。
    pub(super) page_items: Vec<(usize, LibraryEntry)>,
    /// 当前选中项。`None` = 未选中。`set_selected_index` 写入，
    /// `render_item` 读出来给 `ListItem::selected(...)` 用。
    pub(super) selected_index: Option<IndexPath>,
}

impl LibraryDelegate {
    pub(super) fn new(page: Entity<LibraryPage>) -> Self {
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
            // `ListItem::new` 接受 `impl Into<ElementId>` —— 用 `IndexPath` 直接做 id，
            // 跟 docs `list.md` 里所有示例一致（`ListItem::new(ix)`）。
            ListItem::new(ix)
                // 选中态交给 ListItem：它内部根据 `self.selected` 决定画
                // `cx.theme().list_active` 背景 + `list_active_border` 边框。
                .selected(Some(ix) == self.selected_index)
                // 圆角：参考 `crates/story/src/stories/list_story.rs:88-93` 的
                // CompanyListItem。ListItem 内部选中边框是 absolute 定位（覆盖整个
                // ListItem 矩形），加 `.rounded(...)` 后通过 `overflow: hidden` 把
                // 选中边框裁成圆角，否则选中样式是方角。
                .rounded(cx.theme().radius)
                // 行间距：gpui-component 0.5.1 的 `v_virtual_list` 内部**不支持** row gap
                // （源码 `list.rs` paint 处注释："would not able to have gap_y,
                // because the section header, footer is always have rendered as a
                // empty child item"）。替代方案：每个 ListItem 自己 `.mb(px(4.))`，
                // flex 自然按 margin 撑开。最后一行多 4px 底 margin 跟 List 的
                // `py(px(4.))` 重叠（外间距相加），视觉上等价于统一 4px，不影响。
                .mb(px(4.))
                // 用 global_index 而不是 ix.row 作为 row 内部 id 的序号后缀 —— 跨页
                // 连续（page 0 的最后一行 id=(lib-open, 29) 和 page 1 的第一行
                // id=(lib-open, 30) 不会撞）。render_row 内部再用 global_index + 1
                // 渲染给人看的 1-based 序号列。
                .child(row::render_row(global_index, &entry, &self.page, &mut *cx)),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected_index = ix;
        // 必须 notify —— List 才会调 `render_item` 重绘新选中的 ListItem 的高亮。
        // 文档里 MyListDelegate 的 set_selected_index 也调了 cx.notify()。
        cx.notify();
    }
}
