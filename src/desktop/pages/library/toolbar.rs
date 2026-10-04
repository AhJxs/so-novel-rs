//! Library 页工具栏：文件名过滤 Input + 7 个文件类型过滤 Button。
//!
//! 不用 SelectState：它把选项的翻译字段存在 State 里，切语言后失效；Button 的 label
//! 在 render 里现取 `t!(...)`，切语言自动同步。扩展名不译（技术名词）。

use gpui_kit::Context;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Selectable, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
};
use gpui_kit::{Entity, IntoElement, ParentElement, Styled, px};
use rust_i18n::t;

/// 输入行：文件名过滤 Input + ext 过滤按钮组。
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

/// 7 个 ext 过滤 Button（全部 / epub / txt / zip / html / pdf / md）。
fn ext_filter_buttons(
    current_ext: Option<&str>,
    cx: &Context<'_, super::LibraryPage>,
) -> impl IntoElement {
    h_flex().gap_1().items_center().children(vec![
        ext_button(
            "ext-all",
            t!("Library.filter_option_all").into(),
            None,
            current_ext,
            cx,
        ),
        ext_button("ext-epub", "epub".into(), Some("epub"), current_ext, cx),
        ext_button("ext-txt", "txt".into(), Some("txt"), current_ext, cx),
        ext_button("ext-zip", "zip".into(), Some("zip"), current_ext, cx),
        ext_button("ext-html", "html".into(), Some("html"), current_ext, cx),
        ext_button("ext-pdf", "pdf".into(), Some("pdf"), current_ext, cx),
        ext_button("ext-md", "md".into(), Some("md"), current_ext, cx),
    ])
}

/// 单个 ext 过滤 Button：点击 → `set_ext_filter(value)`。
fn ext_button(
    id: &'static str,
    label: gpui_kit::SharedString,
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
