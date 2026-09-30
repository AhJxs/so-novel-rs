//! 通用分页页脚 —— 薄封装 gpui-kit 的 `component::pagination::Pagination`，页码计算 / 省略号 /
//! prev-next 全部委托给组件库，这里只保留两条项目自己的契约：
//! 1. 对外页码是 **0-based**（内部索引），渲染给用户时 +1；
//! 2. **不足一页就隐藏**：`page_count <= 1` 时渲染 `Empty`，列表下方完全不占位。判定放在本组件
//!    而不是各 caller 的 `.when(...)` —— 4 个 page 共用，不会漏。
//!
//! 无状态设计：不持有 `current` / `total` 副本，每帧由 caller 传入（同 `PageHeader`）。`on_change`
//! 拿到的仍是 0-based 页码。

use std::rc::Rc;

use gpui_kit::component::pagination::Pagination as ComponentPagination;
use gpui_kit::{App, Empty, IntoElement, RenderOnce, Styled, Window};

/// 统一的列表分页大小。4 个 page（library / sources / tasks / search）共用。
pub const PAGE_SIZE: usize = 30;

/// 一次分页的切片窗口：`start..end`（end 排他）。`current_page` 已被
/// [`compute_page_window`] 兜底回卷，永远 `start < end <= total`。
#[derive(Debug, Clone, Copy)]
pub struct PageSlice {
    pub start: usize,
    pub end: usize,
    pub total: usize,
    pub page_count: usize,
}

impl PageSlice {
    pub const fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// 算一页 `[start, end)` 区间，并**就地回卷** `current_page` 到合法范围（列表被过滤短了导致越界
/// → 回到最后一页）。调用方一般这样用：
/// ```ignore
/// let w = compute_page_window(total, &mut self.current_page);
/// let items: Vec<_> = results[w.start..w.end].iter().cloned().enumerate()
///     .map(|(i, r)| (w.start + i, r)).collect();
/// ```
pub fn compute_page_window(total: usize, current_page: &mut usize) -> PageSlice {
    let page_count = total.div_ceil(PAGE_SIZE);
    if page_count > 0 && *current_page >= page_count {
        *current_page = page_count - 1;
    }
    let start = *current_page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(total);
    PageSlice {
        start,
        end,
        total,
        page_count,
    }
}

/// 分页组件 `on_change` 回调的 trait alias：调用方传 `Rc<dyn Fn(...)>` —— `Rc<dyn Fn>` 自身
/// `Clone`，所以闭包不必是 `Clone + 'static`，每帧能复用同一份回调。
///
/// 第一参数是 `&usize`（对齐 `cx.listener` 的签名），**语义是 0-based 页码**（组件库内部 1-based）。
pub type PaginationOnChange = dyn Fn(&usize, &mut Window, &mut App) + 'static;

/// 分页组件：0-based 页码 + 「不足一页隐藏」的外部契约，内部委托组件库 `Pagination`。
pub struct Pagination {
    current: usize,
    total: usize,
    /// `Rc<dyn Fn(...)>` 让每帧重建组件库 `Pagination` 时复用同一份回调，只复制引用计数。
    on_change: Rc<PaginationOnChange>,
}

impl Pagination {
    /// `current` 是 **0-based** 当前页；`total` 是**总页数**（`PageSlice::page_count`），
    /// 不是条目数 —— `total <= 1`（含 `total == 0`）时整个页脚渲染成 `Empty`，不显示。
    pub fn new<F>(current: usize, total: usize, on_change: F) -> Self
    where
        F: Fn(&usize, &mut Window, &mut App) + 'static,
    {
        Self {
            current,
            total,
            // 把 `F` 擦成 `dyn Fn(...)`，调用方可以传任何形态的闭包。
            on_change: Rc::new(on_change) as Rc<PaginationOnChange>,
        }
    }
}

// `#[derive(IntoElement)]` 也能用于单态类型；手写等价产物更显式
// （derive 产物是 `ViewElement<Self>`，`RenderOnce` 经 blanket impl 自动成为 `View`）。
impl IntoElement for Pagination {
    type Element = gpui_kit::ViewElement<Self>;

    fn into_element(self) -> Self::Element {
        gpui_kit::ViewElement::new(self)
    }
}

impl RenderOnce for Pagination {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let Self {
            current,
            total,
            on_change,
        } = self;

        // 外部契约 2：**不足一页就隐藏**（`total` 是总页数）。用 `Empty` 而不是空 `div()`：
        // gpui 的 `Empty` 以 `display: Display::None` 请求布局，taffy 会把整棵子树从布局里
        // 摘掉，**连父容器 `gap_3()` 的间距都不留**；空 `div()` 会占一个真实 flex item，
        // 视觉上列表下方多出 12px（4 个 page 的容器都是 `v_flex().gap_3()`）。
        let total_pages = total.max(1);
        if total_pages <= 1 {
            return Empty.into_any_element();
        }

        // 与 `compute_page_window` 同样先 clamp：caller 传进来的 current 可能刚因过滤
        // 变短而越界，先自己 clamp 才能让下面 `new_page != current` 的判断准确。
        let current = current.min(total_pages - 1);

        ComponentPagination::new("pagination-footer")
            // 组件库是 1-based。
            .current_page(current + 1)
            .total_pages(total_pages)
            .on_click(move |page, window, cx| {
                // 组件库回调给 1-based 页码；对外契约是 0-based。同页 / 越界 /
                // disabled 的守卫组件库已做过，这里只做进制换算。
                let new_page = page.saturating_sub(1);
                if new_page != current {
                    on_change(&new_page, window, cx);
                }
            })
            // 组件库根节点已自带 `h_flex().gap_1().items_center().px_2().py_2()`，
            // 这里只补"占满宽度 + 右对齐"。
            .w_full()
            .justify_end()
            .into_any_element()
    }
}
