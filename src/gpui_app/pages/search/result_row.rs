//! 搜索结果列表行渲染（6 列：序号 / 书名 / 元信息 / 源 id / 详情 / 选章 / 全本）。
//!
//! 跟 `library.rs::render_row` / `sources.rs::render_source_row` 同模式：固定宽度列 +
//! `flex_1` 撑满剩余 + `ListItem` 内部选中样式。
//!
//! `page: Entity<SearchPage>` 转发用：全本 / 详情 / 选章 按钮 `on_click` 调
//! `page.update` 拿 `&mut AppModel`。

use gpui::{App, Entity, IntoElement, ParentElement, Styled, div, px};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable, WindowExt,
    button::Button,
    dialog::Dialog,
    h_flex,
    notification::{Notification, NotificationType},
    v_flex,
};

use crate::gpui_app::components::truncate;
use crate::i18n::ts_cached;
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
        .unwrap_or_else(|| ts_cached("Search.result.unknown_author").to_string());
    let latest_display = r
        .latest_chapter
        .clone()
        .unwrap_or_else(|| ts_cached("Search.result.no_latest").to_string());
    // 书源名称：直接用结果自带的 source_name（不再显示 #id）。
    let source_name_display = if r.source_name.is_empty() {
        ts_cached("Search.result.unknown_source").to_string()
    } else {
        truncate(&r.source_name, 20)
    };

    // 全本按钮要 clone result 进闭包（on_click 是 Fn，可多次触发）—— 在闭包
    // 里每次重新 clone。
    let result_for_whole = r.clone();
    let page_for_whole = page.clone();
    // 详情按钮：page clone 进 on_click → open_dialog builder。封面是反应式的
    // （每帧读 live cover_cache），所以 result 的文本字段也要进 builder 闭包 ——
    // 用 Box 改 FnOnce 语义，让 builder 只能调一次（builder 本来也只每帧调一次）。
    let result_for_detail = r.clone();
    let page_for_detail = page.clone();
    let source_id_for_detail = r.source_id;
    let url_for_detail = r.url.clone();
    // 选章按钮：page clone 进 on_click → open_range_dialog。
    let result_for_range = r.clone();
    let page_for_range = page;

    h_flex()
        // 不要 .id(...)：外层 ListItem::new(ix) 已经给了 id，自己再加会和 List 的
        // 虚拟滚动 hit-test 冲突。
        // 不要 .hover / .border_b_1：ListItem paint 逻辑已经根据 selected / hover
        // 画 list_hover / list_active / list_active_border 三套样式。
        .px_2()
        .py_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .items_center()
        // ---- 序号列（跨分页连续：global_index 是 0-based 全局位置，+1 给用户看）----
        // 48px 装 "#100" 这种 4 字符号。右对齐 + muted 颜色，跟 library / sources 一致。
        .child(
            div()
                .w(px(48.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("#{}", idx + 1)),
        )
        // ---- 书名列（flex_1）：书名在上，最新章节在下 ----
        .child(
            v_flex()
                .flex_1()
                .min_w(px(160.))
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
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
        // ---- 作者列（固定宽度，超出省略）----
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
        // ---- 书源名称列（固定宽度，超出省略）----
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
        // ---- 详情按钮（弹只读 Dialog 展示 SearchResult 全字段）----
        .child(
            Button::new(("search-detail", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::Info))
                .label(ts_cached("Search.detail.action"))
                .on_click(move |_, window, cx| {
                    // 1) 拉详情（幂等：detail_cache 命中直接返回）。详情后端返回 cover_url
                    //    后，drain loop 自动派发封面下载（见 app/events.rs L43-50）。
                    page_for_detail.update(cx, |p, cx| {
                        p.model.update(cx, |m, _cx| m.select_search_result(idx));
                    });

                    // 2) 弹反应式 Dialog：builder 每帧被 RootView render 重调，每帧重新
                    //    clone result 文本 + 重新读 live cover_cache，所以封面到达后会自动
                    //    刷新（drain loop 100ms notify → RootView 重 render → builder 重调）。
                    //
                    //    builder 是 `Fn`（每帧重调），但我们要 move result 进去给它读文本字段 ——
                    //    result 是 Clone 的，每次 builder 被调前重新 clone 一份 move 进当帧
                    //    闭包即可，所以 on_click 外层把 result 留在闭包外只 clone 不 move。
                    let page = page_for_detail.clone();
                    let r = result_for_detail.clone();
                    let source_id = source_id_for_detail;
                    let url = url_for_detail.clone();
                    window.open_dialog(cx, move |dialog: Dialog, _window, cx| {
                        // 每帧重新 clone 文本字段（builder 是 Fn，可多次调）。
                        let r = r.clone();
                        let page = page.clone();
                        let url = url.clone();
                        dialog
                            .title(ts_cached("Search.detail.title"))
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
        // ---- 选章按钮（拉 TOC + 弹 confirm Dialog 选起止章节下载）----
        .child(
            Button::new(("search-chapters", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::ChevronRight))
                .label(ts_cached("Search.action.select_chapters"))
                .on_click(move |_, window, cx| {
                    // on_click 是 Fn → 每次重新 clone 一份 result 喂给 open_range_dialog。
                    let r = result_for_range.clone();
                    page_for_range.update(cx, |p, cx| p.open_range_dialog(r, window, cx));
                }),
        )
        // ---- 全本按钮（spawn download + success toast）----
        .child(
            Button::new(("search-whole", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::BookOpen))
                .label(ts_cached("Search.action.download_whole"))
                .on_click(move |_, window, cx| {
                    // on_click 是 Fn（点击可多次触发），所以 on_click 内不能 move
                    // result_for_whole 多次 — 每次重新 clone 一份。
                    let result_for_click = result_for_whole.clone();
                    // spawn_download 走 AppModel — 通过 page.update 转发。
                    // page 是 Entity<SearchPage>，update 后拿到 &mut SearchPage，再
                    // update model 拿 &mut AppModel。
                    let _ = page_for_whole.update(cx, |p, cx| {
                        p.model
                            .update(cx, |m, _cx| m.spawn_download(result_for_click))
                    });
                    // 提示带书名（不用任务 id —— id 对用户无意义）。truncate 防超长书名撑爆 toast。
                    window.push_notification(
                        Notification::new()
                            .title(ts_cached("Search.action.download_started"))
                            .message(truncate(&result_for_whole.book_name, 50))
                            .with_type(NotificationType::Success)
                            .autohide(true),
                        cx,
                    );
                }),
        )
}
