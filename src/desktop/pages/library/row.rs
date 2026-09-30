//! Library 行渲染（序号 / 书名 + tag / 时间 / 操作 列）。
//!
//! 删除按钮经 `page.update(|p| p.prompt_delete(...))` 转发到 `mod.rs` 的
//! `LibraryPage::prompt_delete`。

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    tag::Tag,
};
use gpui_kit::{App, Entity, IntoElement, ParentElement, Styled, div, px};

use crate::desktop::components::truncate;
use crate::desktop::model::LibraryEntry;
use crate::i18n::ts_cached;
use crate::utils::system::{open_path, reveal_in_folder};

use super::LibraryPage;

/// 渲染一行 entry。
pub(super) fn render_row(
    index: usize,
    entry: &LibraryEntry,
    page: &Entity<LibraryPage>,
    cx: &App,
) -> impl IntoElement {
    let path_open = entry.path.clone();
    let path_reveal = entry.path.clone();
    let path_del = entry.path.clone();
    let page_for_del = page.clone();

    // 书名去掉扩展名（类型已由后面的 tag 显示，避免 "三体.epub" 冗余）；拿不到就回退
    // 原 file_name。
    let stem = entry
        .file_name
        .strip_suffix(&format!(".{}", entry.ext))
        .unwrap_or(&entry.file_name)
        .to_string();
    let stem_display = truncate(&stem, 30);
    let ext_upper = entry.ext.to_uppercase();
    let mod_time = crate::utils::formatting::format_local_unix_secs(
        i64::try_from(entry.modified_unix_secs).unwrap_or(0),
        "Library.time.unknown",
        "Library.time.invalid",
        "Library.time.format_failed",
    );

    h_flex()
        // 不要 `.id(...)` / `.hover(...)` / `.border_b_1()`：外层 `ListItem::new(ix)`
        // 已给 id（再加会和虚拟滚动 hit-test 冲突），hover / 选中样式也由 ListItem paint 画。
        .px_2()
        .py_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .items_center()
        // 序号列：跨分页连续的 0-based 全局序号 +1。48px 装 "#100"。
        .child(
            div()
                .w(px(48.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("#{}", index + 1)),
        )
        // 书名 + 类型 tag：tag 紧贴书名右侧，显示大写扩展名。
        //
        // **布局**：外层 h_flex `flex_1()` 占满 row 减去固定列（序号 48px / 大小 80px /
        // 时间 140px / 操作 200px）后的剩余宽度，短书名时也不会让 tag 飘到中间。
        // 内层 book div 也 `flex_1()`；配合 `min_w(0)`（默认 min-width:auto 会阻止收缩）、
        // `overflow_x_hidden` + `text_ellipsis` + `whitespace_nowrap` 才能正确省略。
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
                        .child(
                            div()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(stem_display),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .child(Tag::secondary().small().child(ext_upper))
                                .child(
                                    Tag::secondary()
                                        .small()
                                        .child(crate::utils::formatting::format_size(
                                            entry.size_bytes,
                                        )),
                                ),
                        ),
                ),
        )
        .child(
            // 时间列用固定 140px 而非 flex_1 —— 否则会和书名列平分剩余宽度。
            div()
                .w(px(140.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(mod_time),
        )
        .child(
            // 操作列：固定 240px 装 3 个 xsmall 按钮 + gap。
            h_flex()
                .w(px(240.0))
                .gap_1()
                .justify_end()
                .child(
                    Button::new(("lib-open", index as u64))
                        .small()
                        .outline()
                        .icon(Icon::new(IconName::ExternalLink))
                        .label(ts_cached("Library.action_open"))
                        // 「打开」：系统默认程序打开文件（util/system.rs::open_path）。
                        .on_click(move |_, _window, _cx| {
                            if let Err(e) = open_path(&path_open) {
                                tracing::warn!("open_path failed: {e:#}");
                            }
                        }),
                )
                .child(
                    Button::new(("lib-reveal", index as u64))
                        .small()
                        .outline()
                        .icon(Icon::new(IconName::Folder))
                        .label(ts_cached("Library.action_reveal"))
                        // 「位置」：文件管理器打开所在目录并选中该文件（util/system.rs）。
                        .on_click(move |_, _window, _cx| {
                            if let Err(e) = reveal_in_folder(&path_reveal) {
                                tracing::warn!("reveal_in_folder failed: {e:#}");
                            }
                        }),
                )
                .child(
                    Button::new(("lib-del", index as u64))
                        .small()
                        .danger()
                        .icon(Icon::new(IconName::Delete))
                        .label(ts_cached("Library.action_delete"))
                        .on_click(move |_, window, cx| {
                            // on_click 是 Fn → 每次点击 clone 一份 owned path 给内层闭包。
                            let path_for_click = path_del.clone();
                            page_for_del.update(cx, move |p, cx| {
                                p.prompt_delete(path_for_click, window, cx);
                            });
                        }),
                ),
        )
}
