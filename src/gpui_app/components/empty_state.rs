//! 通用空态组件：图标 + 标题 + 副标题。
//!
//! 用在：Library 空目录、Sources 0 书源、Tasks 0 任务、Search 0 结果 等。
//! 颜色全部走 `cx.theme()`；不写自定义调色板。

use gpui::{
    App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder as _,
};
use gpui_component::{ActiveTheme as _, Icon, IconName, Sizable};

/// 空态展示。`RenderOnce` — 没有内部状态，构造即可用。
#[derive(IntoElement)]
pub struct EmptyState {
    icon: IconName,
    title: SharedString,
    subtitle: Option<SharedString>,
}

impl EmptyState {
    pub fn new(icon: IconName, title: impl Into<SharedString>) -> Self {
        Self {
            icon,
            title: title.into(),
            subtitle: None,
        }
    }

    #[must_use]
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .py_8()
            .child(
                Icon::new(self.icon)
                    .large()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .text_base()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(cx.theme().foreground)
                    .child(self.title),
            )
            .when_some(self.subtitle, |this, sub| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(sub),
                )
            })
    }
}
