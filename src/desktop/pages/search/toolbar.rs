//! 搜索页工具栏，拆成两个子区域：`toolbar_row`（关键词 Input + 书源 Select + 搜索 Button）
//! 和 `source_status_row`（每个源的 status badge + 进度 spinner），由 `mod.rs::impl Render`
//! 依次挂上。

use gpui_kit::component::{
    ActiveTheme as _, Disableable, Icon, IconName, Sizable,
    button::Button,
    h_flex,
    input::{Input, InputState},
    select::{SearchableVec, Select, SelectState},
    spinner::Spinner,
    tag::Tag,
};
use gpui_kit::{
    App, Context, Entity, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder as _, px,
};
use rust_i18n::t;

use crate::desktop::model::{AppModel, SourceStatus};

use super::source_select::SourceSelectItem;

/// 输入行：关键词 Input + "书源" label + Select + 搜索 Button。
pub(super) fn toolbar_row(
    keyword: &Entity<InputState>,
    source_state: &Entity<SelectState<SearchableVec<SourceSelectItem>>>,
    running: bool,
    keyword_empty: bool,
    cx: &Context<'_, super::SearchPage>,
) -> impl IntoElement {
    h_flex()
        .gap_3()
        .items_center()
        .child(
            Input::new(keyword).w(px(320.0)).prefix(
                Icon::new(IconName::Search)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            ),
        )
        .child(
            // 书源下拉："书源" label + Select。
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("Search.source.label")),
                )
                .child(Select::new(source_state).w(px(200.0))),
        )
        .child(
            Button::new("search-go")
                .icon(Icon::new(IconName::Search))
                .label(t!("Search.action.search"))
                .loading(running)
                // 关键词空或正在跑时禁用。
                .disabled(keyword_empty || running)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.run_search(window, cx);
                })),
        )
}

/// 源状态行：每个源的 status badge（搜索运行时显示）+ 进度 spinner。
#[allow(clippy::too_many_arguments)]
pub(super) fn source_status_row(
    _model: &Entity<AppModel>,
    source_status: &[(i32, String, SourceStatus)],
    running: bool,
    received: usize,
    expected: usize,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .gap_2()
        .items_center()
        .flex_wrap()
        .children(source_status.iter().map(|(_, name, status)| {
            // 源状态塞进一个 Tag，颜色跟状态语义走（Pending→secondary / Ok→success /
            // Err→danger）。
            match status {
                SourceStatus::Pending => Tag::secondary()
                    .outline()
                    .child(format!("{name} {}", t!("Search.source_status.pending"))),
                SourceStatus::Ok(n) => Tag::success().outline().child(format!(
                    "{name} {} {}",
                    n,
                    t!("Search.source_status.format")
                )),
                SourceStatus::Err(_) => Tag::danger().outline().child(name.clone()),
            }
        }))
        .when(running, |this| {
            this.child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(Spinner::new().small())
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(" {received}/{expected}")),
                    ),
            )
        })
}
