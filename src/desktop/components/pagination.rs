//! 通用分页页脚 —— 薄封装 gpui-kit 组件库自带的 `component::pagination::Pagination`。
//!
//! ## 为什么从"手写"改成"封装组件库"
//!
//! 项目早期（gpui-kit 0.6.0）组件库还没有分页组件，所以自己写了一套 prev /
//! 数字按钮 / 省略号 / next。**0.6.6 起组件库已自带 `Pagination`**（1-based，
//! `gpui_base::PaginationState` 负责 clamp / 边界 disabled / 页码区间计算），
//! 手写那套的计算逻辑就全部可以删掉了。
//!
//! 现在这里只保留**项目自己的两条外部契约**，页码计算 / 省略号 / prev-next
//! 全部委托给组件库：
//! 1. **页码 0-based**（内部索引），渲染给用户时 +1；
//! 2. **不足一页就隐藏**：`page_count <= 1`（即列表条目数 ≤ [`PAGE_SIZE`]）时渲染
//!    `Empty`，列表下方完全不占位。判定放在本组件里而不是每个 caller 的
//!    `.when(...)` —— 4 个 page 共用同一条件，写在这里不会漏、不会走偏。
//!
//! 调用方（4 个 page）的 `Pagination::new(current, page_count, on_change)` 签名
//! 不变，`on_change` 拿到的仍是 **0-based** 页码。
//!
//! ## 外观约定
//!
//! 走组件库**默认（非 compact）**渲染，跟 gpui-kit 其它应用保持一致：
//! - 页码按钮 + 省略号：省略号是**可点击的下拉菜单**，能直接跳到被折叠的任意页
//!   （组件库 `MAX_ELLIPSIS_MENU_PAGES = 100` 限制每次最多列 100 个隐藏页）；
//! - 当前页用 `outline()` 描边高亮；
//! - prev / next 带组件库 `ui.yml` 的文案 `Pagination.previous` / `Pagination.next`
//!   （随 `component::set_locale` 走），所以是"文字 + 图标"而不是纯图标；
//! - 右对齐（`.w_full().justify_end()`），组件库默认给根节点 `px_2().py_2()`。
//!
//! 用法（`library.rs` 的 pattern）：
//! ```ignore
//! use crate::desktop::components::Pagination;
//!
//! Pagination::new(
//!     self.current_page,
//!     w.page_count,
//!     cx.listener(|this, &new_page, _window, cx| {
//!         this.current_page = new_page;
//!         cx.notify();
//!     }),
//! )
//! ```
//!
//! `cx.listener` 返回的闭包本身就是 `Clone + 'static`（内部捕获的是 entity handle，
//! 那是 `Entity<T>`，实现了 `Clone`）。如果 caller 持有的是不可 Clone 的状态，把
//! 它包成 `Rc::new(...) as Rc<dyn Fn(usize, &mut Window, &mut App) + 'static>` 即可
//! —— `Rc<dyn Fn(...)>` 自身实现 `Clone`。
//!
//! 无状态设计：组件不持有 `current` / `total` 副本，全部由 caller 在每帧 render 前
//! 传入；状态机留在 caller 的 struct 里（一般是 `current_page: usize` 字段）。这样
//! 跟 `PageHeader` 一致 —— 简单、可测试、跨页面复用零成本。

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

/// 算一页 `[start, end)` 区间。**就地回卷** `current_page` 到合法范围
/// （清空 results 后越界 → 回到最后一页），并返回切片信息。
///
/// 调用方一般这样用：
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

/// 分页组件 `on_change` 回调的 trait alias：调用方传 `Rc<dyn Fn(...)>` —— `Rc<dyn Fn>`
/// 自身 `Clone`（Rc 总是 Clone），调用方捕获的 listener 即使不是 `Clone + 'static`
/// 也能塞进来（wrap 一层 Rc 即可）。
///
/// 不直接要求 `F: Clone + 'static` 是不行的：`cx.listener` 返回 `impl Fn + 'static`
/// （不是 `Clone + Fn + 'static`）—— 组件库 `on_click` 内部持有 `Rc<dyn Fn>`，
/// 我们的闭包需要能跨帧复用同一份回调。
///
/// **回调第一参数是 `&usize`** 而非 `usize`，跟 `cx.listener` 的统一签名
/// `Fn(&E, &mut Window, &mut App)` 对齐 —— 调用方通常用
/// `cx.listener(|this, &new_page, _, _| { ... })` 直接传，无需拆 `&`。
/// **语义是 0-based 页码**（组件库内部是 1-based，见 [`Pagination::render`] 的换算）。
pub type PaginationOnChange = dyn Fn(&usize, &mut Window, &mut App) + 'static;

/// 分页组件：0-based 页码 + 「不足一页隐藏」的外部契约，内部委托组件库 `Pagination`。
pub struct Pagination {
    current: usize,
    total: usize,
    /// `Rc<dyn Fn(...)>` 让本组件在每帧重建组件库 `Pagination` 时能复用同一份回调，
    /// 只复制引用计数（不是回调对象本身）。
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
            // 把 `F` 擦成 `dyn Fn(...)` —— 调用方可以传任何形态的闭包，
            // 内部统一用 `Rc<dyn Fn>` 持有。
            on_change: Rc::new(on_change) as Rc<PaginationOnChange>,
        }
    }
}

// `#[derive(IntoElement)]` 对泛型类型工作得很好，但对单态类型 `Pagination`（没有
// 泛型参数了）也能直接 derive。这里手动写 impl 等价于 derive 产物，更显式。
// gpui-kit 里 derive 产物是 `ViewElement<Self>`（`RenderOnce` 类型经 blanket
// `impl<T: RenderOnce> View for T` 自动成为 `View`）。
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

        // 外部契约 2：**不足一页就隐藏**。`total` 是总页数（`PageSlice::page_count`），
        // ≤ 1 说明列表条目数 ≤ `PAGE_SIZE`，页脚没有存在意义 —— 返回 `Empty`。
        //
        // 用 `Empty` 而不是空 `div()`：gpui 的 `Empty` 以 `display: Display::None`
        // 请求布局，taffy 把这棵子树整个从布局里摘掉 —— **连父容器 `gap_3()` 的
        // 间距都不会多留**（4 个 page 的容器都是 `v_flex().gap_3()`）。空 `div()`
        // 会被当成一个真实 flex item 而多占一个 gap，视觉上就是列表下方多出 12px。
        let total_pages = total.max(1);
        if total_pages <= 1 {
            return Empty.into_any_element();
        }

        // 与 `compute_page_window` 同样的回卷兜底：caller 传进来的 current 可能因为
        // 「列表刚被过滤短了」而越界，组件库 `PaginationState::new` 也会 clamp，
        // 但我们自己先 clamp 是为了让 `on_change` 的 `new_page != current` 判断准确。
        let current = current.min(total_pages - 1);

        ComponentPagination::new("pagination-footer")
            // 组件库是 1-based。
            .current_page(current + 1)
            .total_pages(total_pages)
            .on_click(move |page, window, cx| {
                // 组件库回调给的是 1-based 页码；对外契约是 0-based。
                // 组件库 `PaginationState::request_page` 已经做过
                // "同页 / 越界 / disabled 不回调" 的守卫，这里只做进制换算。
                let new_page = page.saturating_sub(1);
                if new_page != current {
                    on_change(&new_page, window, cx);
                }
            })
            // 旧手写版是 `div().flex().flex_row().w_full().justify_end().items_center().gap_1()`；
            // 组件库根节点已自带 `h_flex().gap_1().items_center().px_2().py_2()`，
            // 这里只补"占满宽度 + 右对齐"。
            .w_full()
            .justify_end()
            .into_any_element()
    }
}
