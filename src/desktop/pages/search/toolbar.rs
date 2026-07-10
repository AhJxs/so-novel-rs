//! 搜索页工具栏：关键词 Input + 书源 Select + 搜索 Button + 源状态 Tag + Spinner。
//!
//! 拆出来的 2 个子区域：
//! - `toolbar_row`：输入行（Input / Select / Button）
//! - `source_status_row`：源状态行（每个源的 status badge + 进度 spinner）
//!
//! `mod.rs::impl Render` 依次 `.child(toolbar_row(...))` + `.child(source_status_row(...))`。

use gpui::{
    App, Context, Entity, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, IconName, Sizable,
    button::Button,
    h_flex,
    input::{Input, InputState},
    select::{SearchableVec, Select, SelectState},
    spinner::Spinner,
    tag::Tag,
};

use crate::desktop::model::{AppModel, SourceStatus};
use crate::i18n::ts;

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
            // Select 显示当前选中项的 title（聚合搜索 / 书源名称）。
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(ts("Search.source.label")),
                )
                .child(Select::new(source_state).w(px(200.0))),
        )
        .child(
            Button::new("search-go")
                .icon(Icon::new(IconName::Search))
                .label(ts("Search.action.search"))
                .loading(running)
                // 关键词空 OR 正在跑时禁用 —— 跟加载状态绑定
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
            // 源状态：name + 状态文案全部塞进一个 Tag（语义色），
            // 跟 sources.rs 统计行同款。
            // Neutral→secondary、Success→success、Error→danger。
            match status {
                SourceStatus::Pending => Tag::secondary()
                    .outline()
                    .child(format!("{name} {}", ts("Search.source_status.pending"))),
                SourceStatus::Ok(n) => Tag::success().outline().child(format!(
                    "{name} {} {}",
                    n,
                    ts("Search.source_status.format")
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
