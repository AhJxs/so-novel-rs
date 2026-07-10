//! 通用分页页脚：prev / 数字按钮（带省略号）/ next。
//!
//! 通用约定：
//! - **页码是 0-based**（内部索引），渲染成 1-based 给用户看。
//! - **永远渲染**，不去 hide。即使 `total_pages == 1` 也显示一个数字按钮（当前页）
//!   + 两端 disabled 的 prev/next，给用户"这是完整列表"的视觉锚点。
//! - 当前页用 `Button::selected(true)` 高亮（来自 `Selectable` trait）。
//! - prev / next 在边界时 `.disabled(true)`。
//! - 页数 ≤ 7：全显示；> 7：1 … current-1 current current+1 … N，省略号 `…` 用静态 div。
//!
//! 用法（`library.rs` 的 pattern）：
//! ```ignore
//! use crate::desktop::components::Pagination;
//!
//! Pagination::new(self.current_page, page_count)
//!     .on_change(cx.listener(|this, &new_page, _w, _cx| {
//!         this.current_page = new_page;
//!         _cx.notify();
//!     }))
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

use gpui::{
    AnyElement, App, ClickEvent, Component, IntoElement, ParentElement, RenderOnce, Styled, Window,
    div,
};
use gpui_component::{
    ActiveTheme as _, Disableable, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants as _},
};

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
/// （不是 `Clone + Fn + 'static`）—— 内部 `prev_btn` / `next_btn` / 每个数字按钮的
/// `on_click` slot 都需要各自 capture 一份，回调必须可共享。
///
/// **回调第一参数是 `&usize`** 而非 `usize`，跟 `cx.listener` 的统一签名
/// `Fn(&E, &mut Window, &mut App)` 对齐 —— 调用方通常用
/// `cx.listener(|this, &new_page, _, _| { ... })` 直接传，无需拆 `&`。
pub type PaginationOnChange = dyn Fn(&usize, &mut Window, &mut App) + 'static;

/// 分页组件。
pub struct Pagination {
    current: usize,
    total: usize,
    /// `Rc<dyn Fn(...)>` 让 prev / next / N 个数字按钮各自 `Rc::clone(&on_change)`
    /// 共享一份回调，只复制引用计数（不是回调对象本身）。
    on_change: Rc<PaginationOnChange>,
}

impl Pagination {
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
impl IntoElement for Pagination {
    type Element = Component<Self>;

    fn into_element(self) -> Self::Element {
        Component::new(self)
    }
}

impl RenderOnce for Pagination {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            current,
            total,
            on_change,
        } = self;

        // 始终渲染（用户要求"不要隐藏分页组件"）。total=0 时 total_pages=1，
        // 只有一个 disabled 的 prev/next + 一个高亮的数字按钮"1"，视觉上仍然传达
        // "完整列表已展示"的信息。
        let total_pages = total.max(1);
        let prev_disabled = current == 0;
        let next_disabled = current + 1 >= total_pages;

        let prev_btn = Button::new("page-prev")
            .ghost()
            .icon(IconName::ChevronLeft)
            .disabled(prev_disabled)
            .on_click({
                let on_change = Rc::clone(&on_change);
                move |_ev: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let new_page = current.saturating_sub(1);
                    if new_page != current {
                        on_change(&new_page, window, cx);
                    }
                }
            });

        let next_btn = Button::new("page-next")
            .ghost()
            .icon(IconName::ChevronRight)
            .disabled(next_disabled)
            .on_click({
                let on_change = Rc::clone(&on_change);
                move |_ev: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let new_page = (current + 1).min(total_pages - 1);
                    if new_page != current {
                        on_change(&new_page, window, cx);
                    }
                }
            });

        div()
            .flex()
            .flex_row()
            .w_full()
            .justify_end()
            .items_center()
            .gap_1()
            .child(prev_btn)
            .children(render_page_buttons(current, total_pages, cx, &on_change))
            .child(next_btn)
    }
}

/// 数字按钮 + 省略号。`usize::MAX` 是 sentinel，渲染成 `…`。
fn render_page_buttons(
    current: usize,
    total_pages: usize,
    cx: &App,
    on_change: &Rc<PaginationOnChange>,
) -> Vec<AnyElement> {
    // 简单策略：≤ 7 页全显示；> 7 页显示 1 … current-1 current current+1 … N。
    let pages: Vec<usize> = if total_pages <= 7 {
        (0..total_pages).collect()
    } else {
        let mut v = vec![0usize];
        if current > 2 {
            v.push(usize::MAX); // 左侧省略号
        }
        for p in current.saturating_sub(1)..=(current + 1).min(total_pages - 1) {
            if p != 0 && p != total_pages - 1 && !v.contains(&p) {
                v.push(p);
            }
        }
        if current + 2 < total_pages - 1 {
            v.push(usize::MAX); // 右侧省略号
        }
        if !v.contains(&(total_pages - 1)) {
            v.push(total_pages - 1);
        }
        v
    };

    pages
        .into_iter()
        .map(|p| {
            if p == usize::MAX {
                // 省略号：不可点击的静态文本。
                div()
                    .px_2()
                    .text_color(cx.theme().muted_foreground)
                    .child("…")
                    .into_any_element()
            } else {
                let on_change = on_change.clone();
                Button::new(("page-n", p as u64))
                    .ghost()
                    .small()
                    .selected(p == current)
                    .label((p + 1).to_string())
                    .on_click(move |_ev: &ClickEvent, window: &mut Window, cx: &mut App| {
                        if p != current {
                            on_change(&p, window, cx);
                        }
                    })
                    .into_any_element()
            }
        })
        .collect()
}
