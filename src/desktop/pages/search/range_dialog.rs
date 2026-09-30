//! 选章下载 Dialog body 渲染 + 起止输入框 clamp 工具。
//!
//! 反应式读 `toc_cache[(source_id, url)]`：`Pending` 显示 loading、`Loaded` 显示
//! 「共 N 章」+ 起止 `NumberInput` + 首尾章名预览、`Failed` 显示错误占位，
//! 都由 drain loop 每 100ms 重 render 自动推进。就绪判定在 `confirm_range_download`
//! 那边，这里只渲染。

use gpui_kit::component::{
    ActiveTheme as _, Sizable, h_flex, input::NumberInput, spinner::Spinner, v_flex,
};
use gpui_kit::{App, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div, px};

use crate::desktop::components::truncate;
use crate::desktop::model::TocState;
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
            // 首次进来把输入框初始化成 1 / N；`range_initialized` 防止每帧覆盖用户输入。
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
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(ts_fmt("Search.range.total", &[("n", &n.to_string())])),
                )
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
                                // 宽 160px：minus/plus 各 ~28px，留 ~100px 给数字。
                                // 数字左对齐是组件限制（Input/NumberInput 无水平居中 API，
                                // 外层 text_align 也不会被内部 Input 继承），不要再试着居中。
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
                // 首尾章名各占一行，避免长章名被挤成两段。
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
                        .child(
                            div().text_sm().text_color(cx.theme().foreground).child(
                                div()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .overflow_x_hidden()
                                    .child(format!("{start_title}")),
                            ),
                        )
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

/// 把输入框原始值规整到 `[1, N]`；N = 当前选章 target 在 `toc_cache` 的 Loaded 章节数，
/// 取不到按 `[1, u32::MAX]`。非数字 / 越界 → 1。
///
/// **空字符串返回 0**：sentinel，Change handler 据此识别"用户正在清空输入框，不要重置"。
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
