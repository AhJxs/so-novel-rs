//! Library 行渲染（5 列：序号 / 书名 + tag / 时间 / 操作）。
//!
//! 纯 row 内容 —— `LibraryDelegate`（含 `ListDelegate` impl）抽到 `delegate.rs`。
//! 删除按钮 → `page.update(|p| p.prompt_delete(...))` 转发给 `LibraryPage::prompt_delete`
//! 方法（定义在 `mod.rs`）。

use gpui::{App, Entity, IntoElement, ParentElement, Styled, div, px};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    tag::Tag,
};

use crate::app::LibraryEntry;
use crate::gpui_app::components::truncate;
use crate::i18n::ts_cached;
use crate::utils::system::{open_path, reveal_in_folder};

use super::LibraryPage;

/// 渲染一行 entry（5 列：序号 / 书名 / 时间 / 操作 3 按钮）。
///
/// h_flex 5 个固定宽度的子节点，第 4 列 `flex_1` 占满剩余。
/// 删除按钮走 `LibraryPage::prompt_delete`（page 通过 Entity handle 转发）。
pub(super) fn render_row(
    index: usize,
    entry: &LibraryEntry,
    page: &Entity<LibraryPage>,
    cx: &mut App,
) -> impl IntoElement {
    let path_open = entry.path.clone();
    let path_reveal = entry.path.clone();
    let path_del = entry.path.clone();
    let page_for_del = page.clone();

    // 书名去掉扩展名 —— 扩展名在后面用 tag 显示，避免 "三体.epub" 这种冗余。
    // `strip_suffix(".epub")` 拿不到时（理论上 scan_library_dir 一定 set ext，但兜底）
    // 回退原 file_name，不显示 tag。
    let stem = entry
        .file_name
        .strip_suffix(&format!(".{}", entry.ext))
        .unwrap_or(&entry.file_name)
        .to_string();
    let stem_display = truncate(&stem, 30);
    let ext_upper = entry.ext.to_uppercase();
    let mod_time = crate::utils::formatting::format_local_unix_secs(
        entry.modified_unix_secs as i64,
        "Library.time.unknown",
        "Library.time.invalid",
        "Library.time.format_failed",
    );

    h_flex()
        // 不要 `.id(...)`：外层 `ListItem::new(ix)` 已经给了 id，自己再加会和 List 的
        // 虚拟滚动 hit-test 冲突。
        // 不要 `.hover(|this| this.bg(list_hover))` / `.border_b_1()`：ListItem 的 paint
        // 逻辑已经根据 `selected` / hover 状态画 `list_hover` / `list_active` /
        // `list_active_border` 三套样式（见 `list_item.rs` paint body）。
        .px_2()
        .py_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .items_center()
        // ---- 序号列：跨分页连续（global_index 是 0-based 全局位置，+1 给用户看）----
        // 宽度 48px 装 "#100" / "#999" 这种 4 字符号（"#xxx"）。
        // 右对齐 + muted 颜色，跟"大小 / 时间"列的视觉权重一致。
        .child(
            div()
                .w(px(48.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("#{}", index + 1)),
        )
        // ---- 书名 + 类型 tag ----
        // 书名去掉 `.epub`/`.txt` 等扩展名（用户已经在 tag 看到类型了）；tag 紧贴
        // 书名右侧，显示大写扩展名（EPUB / TXT / ZIP / HTML / PDF）。
        //
        // 用 gpui-component `Tag::secondary().small()` —— 主题色 secondary bg + 圆角 +
        // border，跟周围元素视觉权重区分；小号 (px_1p5 py_0p5) 不抢书名焦点。
        //
        // **布局**：整个 h_flex 列 `flex_1()` 占满 row 减去其他固定列（序号 48px /
        // 大小 80px / 时间 140px / 操作 200px）后的**全部剩余宽度**。短书名时列本身
        // 撑到右边，tag 不会飘到中间或靠左错位。
        //   - 内层 book div `flex_1()` 占满父列（380→可变）**减去 tag 后**的剩余宽度，
        //     不管书名多短，book 部分本身都撑到右边。
        //   - `min_w(0)` 让 flex 子项可以收缩到内容以下（默认 min-width = auto → 子项
        //     内容比容器宽时 flex 不收缩），配合 `overflow_x_hidden` + `text_ellipsis`
        //     才能正确触发文本省略号截断。
        //   - `whitespace_nowrap()` 强制单行（默认 flex 容器会让文本 wrap 撑高）。
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
            // 时间列：固定 140px 装 "2026-01-15 12:34" 这种 16 字符。
            // 不再用 flex_1 —— 否则会跟书名列抢剩余宽度（两个 flex_1 会平分）。
            div()
                .w(px(140.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(mod_time),
        )
        .child(
            // 操作列：固定 200px 装 3 个 xsmall 按钮 + gap，跟其他列一致。
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
                        // 「打开」按钮：用系统默认程序打开文件本身（如 .epub → ebook reader）。
                        // 对应 `util/system.rs::open_path(path)`。
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
                        // 「位置」按钮：OS 文件管理器打开文件**所在目录**，
                        // 高亮显示该文件（Windows `explorer /select,<file>` /
                        // macOS Finder reveal / Linux fallback 到打开父目录 —— 见 util/system.rs）。
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
                            // `Button::on_click` 的 handler 是 `Fn`（点击可多次触发），所以
                            // 外层 closure 不能 move `path_del` 进内层 closure。每次点击
                            // 重新 clone 一份给内层 closure（prompt_delete 内部需要 owned）。
                            let path_for_click = path_del.clone();
                            page_for_del.update(cx, move |p, cx| {
                                p.prompt_delete(path_for_click, window, cx);
                            });
                        }),
                ),
        )
}
