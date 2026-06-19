//! 单条书源行渲染（5 列：序号 / name + lang tag / URL / 健康状态 / Switch / Delete）。
//!
//! 跟 library.rs::render_row 同模式：固定宽 + flex_1 撑满剩余的列布局。
//! 宽度总和（不含 flex_1 的 book/url）：48 + 80 + 90 + 100 + 4×gap ≈ 360 + gap。
//! 1200px 窗口下 book + url 拿到 ~800px，足够显示大多数 URL。

use gpui::prelude::FluentBuilder as _;
use gpui::{App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div, px};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable, StyledExt,
    button::{Button, ButtonVariants as _},
    h_flex,
    link::Link,
    switch::Switch,
    tag::Tag,
};

use crate::crawler::health::{HealthStatus, SourceHealth};
use crate::gpui_app::components::{StatusBadge, StatusKind, truncate};
use crate::i18n::ts;
use crate::models::Rule;

use super::SourcesPage;

/// 渲染一条书源行（5 列：序号 / name + lang tag / url / 健康状态 Badge / Switch / Delete）。
pub(super) fn render(
    index: usize,
    rule: &Rule,
    health: Option<&SourceHealth>,
    page: Entity<SourcesPage>,
    cx: &mut App,
) -> impl IntoElement {
    let name = truncate(&rule.name, 30);
    let lang_display = if rule.language.is_empty() {
        SharedString::from("--")
    } else {
        SharedString::from(rule.language.to_uppercase())
    };
    // 是否需要代理
    let need_proxy = rule.need_proxy;

    h_flex()
        .px_2()
        .py_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .items_center()
        // ---- 序号列 ----
        .child(
            div()
                .w(px(48.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("#{}", index + 1)),
        )
        // ---- 书名 + 语言 tag ----
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.))
                .items_center()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_x_hidden()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(div().whitespace_nowrap().text_ellipsis().child(name))
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .child(Tag::secondary().small().child(lang_display))
                                .when(need_proxy, |d| {
                                    d.child(
                                        Tag::secondary().small().child(ts("Sources.tag.proxy")),
                                    )
                                }),
                        ),
                ),
        )
        // ---- URL（可点击跳浏览器；Link 内置 link 色 + 下划线 + hover 反馈）----
        .child(
            Link::new(("source-url", index as u64))
                .href(SharedString::from(rule.url.clone()))
                .w(px(250.))
                .text_xs()
                .overflow_x_hidden()
                .child(truncate(&rule.url, 60)),
        )
        // ---- 健康状态 Badge ----
        .child(div().w(px(150.)).justify_end().child({
            let (badge_kind, label) = match health {
                None => (StatusKind::Neutral, ts("Sources.health.not_tested").to_string()),
                Some(h) => (health_status_kind_from(h.classify()), h.label()),
            };
            StatusBadge::new(badge_kind, label)
        }))
        // ---- 启用开关 ----
        .child({
            let page_for_switch = page.clone();
            let rule_id = rule.id;
            Switch::new(("src-switch", index as u64))
                .checked(!rule.disabled)
                .on_click(move |checked, _window, cx| {
                    let want_disabled = !*checked;
                    page_for_switch.update(cx, |p, cx| {
                        p.model.update(cx, |m, _cx| {
                            // 只在 model 当前状态与 UI 期望不一致时才 toggle（避免重复触发）。
                            if m.rules.iter().find(|r| r.id == rule_id).map(|r| r.disabled)
                                != Some(want_disabled)
                            {
                                m.toggle_source_disabled(rule_id);
                            }
                        });
                    });
                })
        })
        // ---- 删除按钮（点一次弹 Dialog 二次确认 —— 跟 library.rs `prompt_delete` 同模式）----
        .child({
            let page_for_del = page.clone();
            let rule_id = rule.id;
            Button::new(("src-del", index as u64))
                .small()
                .danger()
                .icon(Icon::new(IconName::Delete))
                .label(ts("Sources.action.delete"))
                .on_click(move |_, window: &mut Window, cx| {
                    page_for_del.update(cx, |p, cx| {
                        p.prompt_delete(rule_id, window, cx);
                    });
                })
        })
}

/// HealthStatus (domain) → StatusKind (UI theme) 映射。
///
/// `crawler::health` 不依赖 `gpui_app`（layering 解耦），所以这层映射留在 UI 侧。
fn health_status_kind_from(status: HealthStatus) -> StatusKind {
    use HealthStatus as H;
    match status {
        H::Ok => StatusKind::Success,
        H::Redirect => StatusKind::Info,
        H::BadResponse => StatusKind::Warning,
        H::ProbeError => StatusKind::Error,
        H::NetworkError => StatusKind::Warning,
    }
}
