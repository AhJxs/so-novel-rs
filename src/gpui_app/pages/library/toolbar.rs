//! Library 页工具栏：文件名过滤 Input + 6-Button 文件类型过滤组。
//!
//! 文件名过滤：placeholder 在 `InputState` 上（gpui-component 0.5.1 API 限制），
//! 切语言靠重启生效。
//!
//! 文件类型过滤：不用 SelectState（持有 options 翻译字段，切语言失效）。
//! 改用 6 个 Button，label 在 render 里现取 `ts(...)`，切语言自动同步。
//! 6 个值 = "全部" + epub/txt/zip/html/pdf。扩展名不译（技术名词）。

use gpui::Context;
use gpui::{Entity, IntoElement, ParentElement, Styled, px};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
};

use crate::i18n::ts;

/// 输入行：文件名前缀 Search 图标 + Input + 6-Button ext 过滤组。
pub(super) fn render(
    filter_input: &Entity<InputState>,
    current_ext: Option<&str>,
    cx: &Context<'_, super::LibraryPage>,
) -> impl IntoElement {
    h_flex()
        .gap_3()
        .items_center()
        .child(
            Input::new(filter_input).w(px(280.0)).prefix(
                Icon::new(IconName::Search)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            ),
        )
        .child(ext_filter_buttons(current_ext, cx))
}

/// 6 个 ext 过滤 Button（全部 / epub / txt / zip / html / pdf）。
fn ext_filter_buttons(
    current_ext: Option<&str>,
    cx: &Context<'_, super::LibraryPage>,
) -> impl IntoElement {
    h_flex().gap_1().items_center().children(vec![
        ext_button(
            "ext-all",
            ts("Library.filter_option_all"),
            None,
            current_ext,
            cx,
        ),
        ext_button("ext-epub", "epub".into(), Some("epub"), current_ext, cx),
        ext_button("ext-txt", "txt".into(), Some("txt"), current_ext, cx),
        ext_button("ext-zip", "zip".into(), Some("zip"), current_ext, cx),
        ext_button("ext-html", "html".into(), Some("html"), current_ext, cx),
        ext_button("ext-pdf", "pdf".into(), Some("pdf"), current_ext, cx),
    ])
}

/// 单个 ext 过滤 Button：点击 → `set_ext_filter(value)`。
fn ext_button(
    id: &'static str,
    label: gpui::SharedString,
    value: Option<&'static str>,
    current_ext: Option<&str>,
    cx: &Context<'_, super::LibraryPage>,
) -> impl IntoElement {
    let selected = current_ext == value;
    Button::new(id)
        .small()
        .ghost()
        .selected(selected)
        .label(label)
        .on_click(cx.listener(move |this, _, _window, cx| {
            this.set_ext_filter(value.map(str::to_string), cx);
        }))
}
