//! 通用页面顶栏：左侧 title + subtitle，右侧 actions slot。

use gpui::{
    App, Div, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::ActiveTheme as _;

/// 页面顶栏。最常见的 `<h1> + 副标题 + 右侧按钮组` 模式。
#[derive(IntoElement)]
pub struct PageHeader {
    title: SharedString,
    subtitle: Option<SharedString>,
    actions: Vec<Div>,
}

impl PageHeader {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            actions: Vec::new(),
        }
    }

    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// 添加一个右侧 action。`Slot` 是 `gpui::Div` — 调用方自己 `Button::new(...)` 后
    /// `.into_any_element()` 转 `Div` 即可（gpui 的 Div 可以套任何 element）。
    #[must_use]
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(div().child(action));
        self
    }
}

impl RenderOnce for PageHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .pb_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(cx.theme().foreground)
                            .child(self.title),
                    )
                    .when_some(self.subtitle, |this, sub| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(sub),
                        )
                    }),
            )
            .when(!self.actions.is_empty(), |this| {
                this.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .children(self.actions),
                )
            })
    }
}
