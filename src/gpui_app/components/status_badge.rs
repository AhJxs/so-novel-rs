//! StatusBadge：薄封装 gpui-component `Badge`。
//!
//! gpui-component 本身已有 `Badge` + `Alert`，颜色自动走 `cx.theme().info / success / warning / error`。
//! 这里只做"业务命名 → Badge"的便捷构造器，不引入新调色板。

use gpui::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div};
use gpui_component::{ActiveTheme as _, badge::Badge};

/// 业务状态的 4 个语义色（与 gpui-component 主题色对齐）。
#[derive(Debug, Clone, Copy)]
pub enum StatusKind {
    Info,
    Success,
    Warning,
    Error,
    /// 中性 / 默认 — 没有特定语义，用 `muted_foreground` 兜底。
    Neutral,
}

impl StatusKind {
    fn color(self, cx: &App) -> gpui::Hsla {
        match self {
            Self::Info => cx.theme().info,
            Self::Success => cx.theme().success,
            Self::Warning => cx.theme().warning,
            Self::Error => cx.theme().danger,
            Self::Neutral => cx.theme().muted_foreground,
        }
    }
}

/// 状态徽章。`RenderOnce` — 一行小标签，用于 tasks / sources / books 等列表。
#[derive(IntoElement)]
pub struct StatusBadge {
    kind: StatusKind,
    label: SharedString,
}

impl StatusBadge {
    pub fn new(kind: StatusKind, label: impl Into<SharedString>) -> Self {
        Self {
            kind,
            label: label.into(),
        }
    }
}

impl RenderOnce for StatusBadge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        // gpui-component Badge 自带样式；这里只覆盖 .text_color 让"业务名 → 主题色"
        // 的映射集中在一处。
        div()
            .text_sm()
            .text_color(self.kind.color(cx))
            .child(Badge::new().child(self.label))
    }
}
