//! Sources 页工具栏：名字过滤 Input + 活跃书源文件下拉 + 3-Button 状态过滤组。
//!
//! 状态过滤（全部 / 启用 / 禁用）：不走 SelectState（持有 options 翻译字段，切语言失效）。
//! 改用 3 个 Button，label 在 render 里现取 `ts(...)`，切语言自动同步。
//! 状态用 `selected` style 标记，存的是 enum 不带翻译。
//!
//! 名字过滤：placeholder 在 `InputState` 上（gpui-component 0.5.1 API 限制），
//! 切语言靠 `mod.rs` 顶部的 sentinel + `set_placeholder` 实时刷新。

use gpui::{Context, Entity, IntoElement, ParentElement, Styled, div, px};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    select::{SearchableVec, Select, SelectState},
};

use crate::desktop::model::SourcesFilterStatus;
use crate::i18n::ts;

use super::SourcesPage;

/// 输入行：文件名前缀 Search 图标 + Input + 活跃书源文件下拉 + 3-Button 状态过滤组。
pub(super) fn render(
    filter_input: &Entity<InputState>,
    rule_file_select: &Entity<SelectState<SearchableVec<String>>>,
    current_status: SourcesFilterStatus,
    cx: &Context<'_, SourcesPage>,
) -> impl IntoElement {
    h_flex()
        .gap_3()
        .items_center()
        .child(
            Input::new(filter_input).w(px(280.0)).prefix(
                Icon::new(IconName::Search)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            ),
        )
        // 活跃书源文件下拉框
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(ts("Sources.active_file.label")),
                )
                .child(Select::new(rule_file_select).w(px(200.0))),
        )
        .child(status_filter_buttons(current_status, cx))
}

/// 3 个 status 过滤 Button（全部 / 启用 / 禁用）。
fn status_filter_buttons(
    current_status: SourcesFilterStatus,
    cx: &Context<'_, SourcesPage>,
) -> impl IntoElement {
    h_flex()
        .gap_1()
        .items_center()
        .child(status_button(
            "status-all",
            ts("Sources.status.all"),
            SourcesFilterStatus::All,
            current_status,
            cx,
        ))
        .child(status_button(
            "status-enabled",
            ts("Sources.status.enabled"),
            SourcesFilterStatus::Enabled,
            current_status,
            cx,
        ))
        .child(status_button(
            "status-disabled",
            ts("Sources.status.disabled"),
            SourcesFilterStatus::Disabled,
            current_status,
            cx,
        ))
}

/// 单个 status 过滤 Button：点击 → `set_status_filter(new_status)`。
fn status_button(
    id: &'static str,
    label: gpui::SharedString,
    value: SourcesFilterStatus,
    current_status: SourcesFilterStatus,
    cx: &Context<'_, SourcesPage>,
) -> impl IntoElement {
    let selected = current_status == value;
    Button::new(id)
        .small()
        .ghost()
        .selected(selected)
        .label(label)
        .on_click(cx.listener(move |this, _, _window, cx| {
            this.set_status_filter(value, cx);
        }))
}
