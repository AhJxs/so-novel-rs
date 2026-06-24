//! 单条书源行渲染（4 列：序号 / name + lang tag / URL / 健康状态 / Switch）。
//!
//! 跟 library.rs::render_row 同模式：固定宽 + flex_1 撑满剩余的列布局。

use gpui::prelude::FluentBuilder as _;
use gpui::{App, Entity, IntoElement, ParentElement, SharedString, Styled, div, px};
use gpui_component::{
    ActiveTheme as _, Sizable, StyledExt, h_flex, link::Link, switch::Switch, tag::Tag,
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
        .child(
            // URL 列：可点击 Link，点击 → 浏览器打开对应书源首页。
            // `Link::new().href(...)` 内置 on_click 调 cx.open_url，跟 detail_dialog.rs 同模式。
            // Link 不实现 Sizable，按项目惯例把字号挂在**外层 div** 上（text_xs 等价于"small"）。
            div()
                .w(px(250.))
                .text_xs()
                .overflow_x_hidden()
                .child(
                    Link::new(("src-url", index as u64))
                        .href(rule.url.clone())
                        .child(truncate(&rule.url, 60)),
                ),
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
            let rule_url = rule.url.clone();
            Switch::new(("src-switch", index as u64))
                .checked(!rule.disabled)
                .on_click(move |checked, _window, cx| {
                    let want_disabled = !*checked;
                    page_for_switch.update(cx, |p, cx| {
                        p.model.update(cx, |m, _cx| {
                            // 只在 model 当前状态与 UI 期望不一致时才 toggle（避免重复触发）。
                            if m.rules.iter().find(|r| r.url == rule_url).map(|r| r.disabled)
                                != Some(want_disabled)
                            {
                                m.toggle_source_disabled(&rule_url);
                            }
                        });
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
