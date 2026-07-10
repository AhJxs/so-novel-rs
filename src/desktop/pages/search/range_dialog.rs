//! 选章下载 Dialog body 渲染 + 起止输入框 clamp 工具。
//!
//! 反应式读 `toc_cache[(source_id, url)]`：
//! - `Pending` / 未拉到 → loading 占位（drain loop 100ms 后刷新）
//! - `Loaded(book, chapters)` → 首次 `set_value` 初始化 1 / N，显示「共 N 章」+ 起止
//!   `NumberInput` + 选中范围首尾章节名预览
//! - `Failed` → 错误占位
//!
//! 就绪与否由 `on_ok` 通过 `confirm_range_download` 返回的 `RangeOutcome::Pending` 判定，
//! 这里只负责渲染。

use gpui::{App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div, px};
use gpui_component::{
    ActiveTheme as _, Sizable, h_flex, input::NumberInput, spinner::Spinner, v_flex,
};

use crate::desktop::model::TocState;
use crate::desktop::components::truncate;
use crate::i18n::{ts, ts_fmt};
use crate::models::Chapter;

use super::SearchPage;

/// 渲染选章 Dialog 的 body。
pub(super) fn content(
    page: &Entity<SearchPage>,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    // 先取 toc 状态（读 model，只读借用）。
    let target = page.read(cx).range_target.clone();
    let toc = target.as_ref().and_then(|t| {
        page.read(cx)
            .model
            .read(cx)
            .search
            .toc_cache
            .get(&(t.source_id, t.url.clone()))
            .cloned()
    });

    match toc {
        None | Some(TocState::Pending) => h_flex()
            .gap_2()
            .items_center()
            .py_4()
            .child(Spinner::new().small())
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(ts("Search.range.loading")),
            )
            .into_any_element(),
        Some(TocState::Failed(msg)) => div()
            .py_4()
            .text_sm()
            .text_color(cx.theme().danger_foreground)
            .child(format!("{}: {msg}", ts("Search.range.failed")))
            .into_any_element(),
        Some(TocState::Loaded(_book, chapters)) => {
            let n = chapters.len();
            // 首次进来初始化起止输入框：1 / N。range_initialized 防止每帧覆盖用户输入。
            // set_value 要 `&mut Window`，update 只借 cx，window 独立借用不冲突。
            page.update(cx, |p, cx| {
                if !p.range_initialized {
                    p.range_start_input
                        .update(cx, |s, cx| s.set_value("1".to_string(), window, cx));
                    p.range_end_input
                        .update(cx, |s, cx| s.set_value(n.to_string(), window, cx));
                    p.range_initialized = true;
                }
            });

            // 读当前输入框值，算预览的首尾章节名。
            let start_v = page
                .read(cx)
                .range_start_input
                .read(cx)
                .value()
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|&v| v >= 1 && v <= n)
                .unwrap_or(1);
            let end_v = page
                .read(cx)
                .range_end_input
                .read(cx)
                .value()
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|&v| v >= 1 && v <= n)
                .unwrap_or(n);
            let (lo, hi) = if start_v <= end_v {
                (start_v, end_v)
            } else {
                (end_v, start_v)
            };
            let start_title = chapter_title_display(&chapters, lo);
            let end_title = chapter_title_display(&chapters, hi);
            let count = hi.saturating_sub(lo) + 1;

            // 布局：共 N 章 → 起止 NumberInput（label + 输入框）→ 选中预览（首尾章名各一行）。
            v_flex()
                .gap_3()
                // 共 N 章
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(ts_fmt("Search.range.total", &[("n", &n.to_string())])),
                )
                // 起止输入行
                .child(
                    h_flex()
                        .gap_4()
                        .items_center()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(ts("Search.range.start")),
                                )
                                // 加宽到 160px（minus/plus 按钮各占 ~28px，留 ~100px 给数字）。
                                // 注：gpui-component 0.5.1 的 Input/NumberInput 不支持文本水平
                                // 居中——数字由自定义 element 固定左对齐绘制，无对齐 API，
                                // 外层 styled 的 text_align 也不会被内部 Input 继承。接受左对齐。
                                .child(NumberInput::new(&page.read(cx).range_start_input).w(px(160.0))),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(ts("Search.range.end")),
                                )
                                .child(NumberInput::new(&page.read(cx).range_end_input).w(px(160.0))),
                        ),
                )
                // 选中预览：首尾章名各一行（避免长章名挤一行被换行成两段）。
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} ({} {})",
                                    ts("Search.range.preview"),
                                    count,
                                    ts("Search.source_status.format")
                                )),
                        )
                        // 起始章名：truncate 防超长。
                        .child(
                            div().text_sm().text_color(cx.theme().foreground).child(
                                div()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .overflow_x_hidden()
                                    .child(format!("{start_title}")),
                            ),
                        )
                        // 结束章名：truncate 防超长。
                        .child(
                            div().text_sm().text_color(cx.theme().foreground).child(
                                div()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .overflow_x_hidden()
                                    .child(format!("{end_title}")),
                            ),
                        ),
                )
                .into_any_element()
        }
    }
}

/// 取第 `n` 章（1-based）的显示标题；越界 / 空标题走 fallback。
fn chapter_title_display(chapters: &[Chapter], n: usize) -> SharedString {
    match chapters.get(n.saturating_sub(1)) {
        Some(c) if !c.title.trim().is_empty() => {
            SharedString::from(format!("{}. {}", n, truncate(&c.title, 40)))
        }
        _ => SharedString::from(format!("{}. {}", n, ts("Search.range.no_title"))),
    }
}

/// 把输入框原始值规整到 `[1, N]`：N 取当前选章 Dialog 的 `toc_cache` Loaded 章节数。
/// 非数字 / 越界 → 1。N 取不到（TOC 没回来）时按 `[1, u32::MAX]`（任意正整数）。
///
/// **空字符串返回 0**（sentinel，Change handler 据此识别"用户正在清空输入框，不要重置"）。
/// free fn —— 4 个订阅共用它无需 clone。
pub(super) fn clamp_range_value(this: &SearchPage, raw: &SharedString, cx: &App) -> u32 {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return 0;
    }
    let n = this
        .current_range_chapters_len(cx)
        .unwrap_or(u32::MAX)
        .max(1);
    let v = trimmed.parse::<u32>().unwrap_or(1);
    v.clamp(1, n)
}
