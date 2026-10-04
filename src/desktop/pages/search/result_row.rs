//! 搜索结果列表行渲染（序号 / 书名 / 元信息 / 源名 / 详情 / 选章 / 全本 列）。
//!
//! 固定宽度列 + `flex_1` 撑满 + `ListItem` 内部选中样式；`page` 只用于按钮 `on_click` 转发。

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable, WindowExt,
    button::Button,
    dialog::Dialog,
    h_flex,
    notification::{Notification, NotificationType},
    v_flex,
};
use gpui_kit::{App, Entity, IntoElement, ParentElement, Styled, div, px};
use rust_i18n::t;

use crate::desktop::components::truncate;
use crate::models::SearchResult;

use super::SearchPage;
use super::detail_dialog;

/// 渲染一条搜索结果行（6 列：序号 / 书名 / 元信息 / 源 id / 选章 / 全本）。
pub(super) fn render(
    idx: usize,
    r: &SearchResult,
    page: Entity<SearchPage>,
    cx: &App,
) -> impl IntoElement {
    let name = truncate(&r.book_name, 50);
    let author_display = r
        .author
        .clone()
        .unwrap_or_else(|| t!("Search.result.unknown_author").to_string());
    let latest_display = r
        .latest_chapter
        .clone()
        .unwrap_or_else(|| t!("Search.result.no_latest").to_string());
    // 书源名称直接用结果自带的 source_name。
    let source_name_display = if r.source_name.is_empty() {
        t!("Search.result.unknown_source").to_string()
    } else {
        truncate(&r.source_name, 20)
    };

    // 三个 on_click 都是 Fn（可能触发多次），所以 result / page 只 clone 到闭包外，
    // 每次点击时再 clone 一份。
    let result_for_whole = r.clone();
    let page_for_whole = page.clone();
    // 详情 Dialog 的 builder 每帧重调以重读 live cover_cache（封面到达后自动刷新），
    // 所以 result 留在闭包外只 clone。
    let result_for_detail = r.clone();
    let page_for_detail = page.clone();
    let source_id_for_detail = r.source_id;
    let url_for_detail = r.url.clone();
    let result_for_range = r.clone();
    let page_for_range = page;

    h_flex()
        // 不要 .id(...) / .hover：外层 `ListItem::new(ix)` 已给 id，再加会和 List 的
        // 虚拟滚动 hit-test 冲突；hover / 选中样式也由 ListItem paint 负责。
        .px_2()
        .py_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .items_center()
        // 序号列：跨分页连续（idx 是全局 0-based）。48px 装 "#100"。
        .child(
            div()
                .w(px(48.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("#{}", idx + 1)),
        )
        // 书名列（flex_1）：书名在上，最新章节在下。
        .child(
            v_flex()
                .flex_1()
                .min_w(px(160.))
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .text_color(cx.theme().foreground)
                        .child(div().whitespace_nowrap().text_ellipsis().child(name)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            div()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(latest_display),
                        ),
                ),
        )
        // 作者列
        .child(
            div()
                .w(px(140.0))
                .overflow_x_hidden()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(author_display),
                ),
        )
        // 书源名称列
        .child(
            div()
                .w(px(120.0))
                .overflow_x_hidden()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(source_name_display),
                ),
        )
        // 详情按钮（弹只读 Dialog 展示全字段）
        .child(
            Button::new(("search-detail", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::Info))
                .label(t!("Search.detail.action"))
                .on_click(move |_, window, cx| {
                    // 1) 拉详情（幂等：detail_cache 命中即返回）；拿到 cover_url 后 drain
                    //    loop 会自动派发封面下载。
                    page_for_detail.update(cx, |p, cx| {
                        p.model.update(cx, |m, _cx| m.select_search_result(idx));
                    });

                    // 2) 弹反应式 Dialog：builder 是 Fn（每帧重调），每帧重读 live
                    //    cover_cache，封面到达后自动刷新。result 是 Clone 的，留在闭包外
                    //    每次 clone 一份进当帧，所以不用 move 进去。
                    let page = page_for_detail.clone();
                    let r = result_for_detail.clone();
                    let source_id = source_id_for_detail;
                    let url = url_for_detail.clone();
                    window.open_dialog(cx, move |dialog: Dialog, _window, cx| {
                        // 每帧重新 clone（builder 可多次调）。
                        let r = r.clone();
                        let page = page.clone();
                        let url = url.clone();
                        dialog
                            .title(t!("Search.detail.title"))
                            .w(px(640.))
                            .child(detail_dialog::content(
                                &r,
                                &page,
                                source_id,
                                &url,
                                cx,
                            ))
                    });
                }),
        )
        // 选章按钮（拉 TOC + 弹选起止章节的 confirm Dialog）
        .child(
            Button::new(("search-chapters", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::ChevronRight))
                .label(t!("Search.action.select_chapters"))
                .on_click(move |_, window, cx| {
                    // on_click 是 Fn → 每次点击重新 clone 一份 result。
                    let r = result_for_range.clone();
                    page_for_range.update(cx, |p, cx| p.open_range_dialog(r, window, cx));
                }),
        )
        // 全本按钮（spawn download + success toast）
        .child(
            Button::new(("search-whole", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::BookOpen))
                .label(t!("Search.action.download_whole"))
                .on_click(move |_, window, cx| {
                    // on_click 是 Fn（可多次触发），每次点击重新 clone 一份。
                    let result_for_click = result_for_whole.clone();
                    // spawn_download 在 AppModel 上，经 page.update 转发。
                    let _ = page_for_whole.update(cx, |p, cx| {
                        p.model
                            .update(cx, |m, _cx| m.spawn_download(result_for_click))
                    });
                    // 提示带书名（任务 id 对用户无意义）；truncate 防超长书名撑爆 toast。
                    window.push_notification(
                        Notification::new()
                            .title(t!("Search.action.download_started"))
                            .message(truncate(&result_for_whole.book_name, 50))
                            .with_type(NotificationType::Success)
                            .autohide(true),
                        cx,
                    );
                }),
        )
}
