//! Tasks 页工具栏：5 个状态过滤 Button。
//!
//! 跟 sources.rs 状态过滤同款：`.small().ghost().selected(bool)`，label 拼 "i18n + 数量"。
//! label 在 render 里现取 `ts(...)` + 当前 counts，切语言自动同步。

use gpui::{Context, IntoElement, ParentElement, Styled};
use gpui_component::{
    Selectable, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
};

use crate::i18n::ts;

use super::TasksPage;
use super::summary::TaskFilter;

/// 5-Button 过滤组：「全部 / 运行中 / 已完成 / 失败 / 已取消」各带数量后缀。
pub(super) fn filter_buttons(
    filter: TaskFilter,
    counts: [usize; 5],
    cx: &Context<'_, TasksPage>,
) -> impl IntoElement {
    h_flex()
        .gap_1()
        .items_center()
        .children(TaskFilter::ALL.iter().enumerate().map(|(i, &f)| {
            let label = format!("{} {}", ts(f.label_key()), counts[i]);
            Button::new(("task-filter", i as u64))
                .small()
                .ghost()
                .selected(filter == f)
                .label(label)
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.set_filter(f, cx);
                }))
        }))
}
