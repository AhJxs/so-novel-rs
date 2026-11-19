//! 「阅读设置」弹窗：实时预览、主题、字号 / 行高 / 段距 / 页宽、首行缩进、编码。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, IconName};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement, StatefulInteractiveElement as _, Styled, div, px,
    relative,
};
use rust_i18n::t;

use super::reader_colors;
use super::reader_page::{ENCODING_CHOICES, ReaderPage};
use crate::config::{ReaderConfig, ReaderTheme};
use crate::library::BookFormat;

/// 面板里五个主题按钮的顺序。
const THEMES: [ReaderTheme; 5] = [
    ReaderTheme::Paper,
    ReaderTheme::Sepia,
    ReaderTheme::Night,
    ReaderTheme::Green,
    ReaderTheme::App,
];

/// 可以用「− 值 +」调整的四项数值。
#[derive(Debug, Clone, Copy)]
enum Field {
    FontSize,
    LineHeight,
    ParagraphSpacing,
    PageWidth,
}

impl Field {
    const ALL: [Self; 4] = [
        Self::FontSize,
        Self::LineHeight,
        Self::ParagraphSpacing,
        Self::PageWidth,
    ];

    const fn slug(self) -> &'static str {
        match self {
            Self::FontSize => "font-size",
            Self::LineHeight => "line-height",
            Self::ParagraphSpacing => "paragraph-spacing",
            Self::PageWidth => "page-width",
        }
    }

    const fn label_key(self) -> &'static str {
        match self {
            Self::FontSize => "Reader.typography.font_size",
            Self::LineHeight => "Reader.typography.line_height",
            Self::ParagraphSpacing => "Reader.typography.paragraph_spacing",
            Self::PageWidth => "Reader.typography.page_width",
        }
    }

    const fn step(self) -> f32 {
        match self {
            Self::FontSize => 1.0,
            Self::LineHeight => 0.1,
            Self::ParagraphSpacing => 2.0,
            Self::PageWidth => 40.0,
        }
    }

    const fn value(self, reader: &ReaderConfig) -> f32 {
        match self {
            Self::FontSize => reader.font_size,
            Self::LineHeight => reader.line_height,
            Self::ParagraphSpacing => reader.paragraph_spacing,
            Self::PageWidth => reader.page_width,
        }
    }

    const fn set(self, reader: &mut ReaderConfig, value: f32) {
        match self {
            Self::FontSize => reader.font_size = value,
            Self::LineHeight => reader.line_height = value,
            Self::ParagraphSpacing => reader.paragraph_spacing = value,
            Self::PageWidth => reader.page_width = value,
        }
    }

    /// 步进值的小数位跟着 `step()` 走，避免 1.7000000000000002 这种尾巴。
    fn format(self, value: f32) -> String {
        match self {
            Self::LineHeight => format!("{value:.1}"),
            Self::FontSize | Self::ParagraphSpacing | Self::PageWidth => format!("{value:.0}"),
        }
    }
}

impl ReaderPage {
    /// 居中的「阅读设置」弹窗：每次改动即时生效，关闭时才落盘。
    pub(super) fn render_typography(&self, cx: &Context<Self>) -> AnyElement {
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, cx);
        // 预览按「当前设置排一行字」，改字号 / 行距 / 缩进都立刻看得见。
        let preview_text = if reader.indent {
            format!("　　{}", t!("Reader.typography.preview"))
        } else {
            t!("Reader.typography.preview").to_string()
        };
        let preview = div()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(colors.background)
            .p_4()
            .overflow_hidden()
            .child(
                div()
                    .text_size(px(reader.font_size))
                    .line_height(px(reader.font_size * reader.line_height))
                    .text_color(colors.foreground)
                    .child(preview_text),
            );

        let mut body = div()
            .id("reader-settings-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(preview)
            .child(Self::render_theme_section(reader.theme, cx));

        let mut fields = div().flex().flex_col().gap_3();
        for field in Field::ALL {
            fields = fields.child(Self::render_field_row(field, &reader, cx));
        }
        body = body
            .child(fields)
            .child(Self::render_indent_row(reader.indent, cx));
        // 编码只对 TXT 有意义；放在这里，用户随时能改回自动或换一个标签。
        if matches!(self.book.format, BookFormat::Txt) {
            body = body.child(self.render_encoding_section(cx));
        }

        let card = div()
            .id("reader-settings-card")
            .w(px(520.0))
            .h(px(600.0))
            .max_h(relative(0.9))
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            // 卡片内部的点击不要冒泡到遮罩层，否则改设置会顺手关掉弹窗。
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation();
            })
            .on_click(|_event: &ClickEvent, _window, cx| {
                cx.stop_propagation();
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_4()
                    .h(px(48.0))
                    .flex_none()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Reader.settings.title").to_string()),
                    )
                    .child(
                        Button::new("reader-settings-close")
                            .ghost()
                            .compact()
                            .icon(IconName::Close)
                            .tooltip(t!("Reader.action.close").to_string())
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.close_settings(cx);
                            })),
                    ),
            )
            .child(body);

        div()
            .id("reader-settings-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().overlay)
            // 只由右上角 ✕ 或 Esc 关闭：遮罩不接管点击，免得卡片里点一下就把弹窗关掉。
            .child(card)
            .into_any_element()
    }

    /// 主题色卡：每张卡直接用该主题的纸面 / 文字上色，选中的描主色边。
    fn render_theme_section(current: ReaderTheme, cx: &Context<Self>) -> Div {
        let mut row = div().flex().flex_row().gap_2();
        for theme in THEMES {
            let colors = reader_colors(theme, cx);
            let selected = theme == current;
            let swatch = div()
                .id(ElementId::Name(
                    format!("reader-theme-{}", theme.as_str()).into(),
                ))
                .h(px(44.0))
                .w_full()
                .rounded_md()
                .border_2()
                .border_color(if selected {
                    cx.theme().primary
                } else {
                    cx.theme().border
                })
                .bg(colors.background)
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(colors.foreground)
                .child("Aa")
                .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                    this.update_reader_config(move |reader| reader.theme = theme, cx);
                }));
            row = row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .child(swatch)
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!(theme.label_key()).to_string()),
                    ),
            );
        }
        section("Reader.typography.title", row, cx)
    }

    /// TXT 编码行：显示当前生效编码 + 「自动 / 各候选编码」按钮。
    ///
    /// 高亮看 `book.encoding`（用户的显式选择）；文字显示 `content.encoding`
    /// （真正解码用的标签，自动探测时两者可能不同）。
    fn render_encoding_section(&self, cx: &Context<Self>) -> Div {
        // 加载失败时 `content` 为空：退回显示用户选的标签，别谎报「自动」。
        let effective = self
            .content
            .as_ref()
            .and_then(|content| content.encoding.clone())
            .or_else(|| self.book.encoding.clone());
        let chosen = self.book.encoding.as_deref();
        let mut buttons = div().flex().flex_row().flex_wrap().gap_2();
        let mut auto = Button::new("reader-encoding-auto")
            .compact()
            .label(t!("Reader.typography.encoding_auto"));
        auto = if chosen.is_none() {
            auto.primary()
        } else {
            auto.ghost()
        };
        buttons = buttons.child(auto.on_click(cx.listener(
            |this, _event: &ClickEvent, _window, cx| {
                this.retry_with_encoding(None, cx);
            },
        )));
        for &label in ENCODING_CHOICES {
            let mut button = Button::new(format!("reader-encoding-panel-{label}"))
                .compact()
                .label(label);
            button = if chosen == Some(label) {
                button.primary()
            } else {
                button.ghost()
            };
            buttons = buttons.child(button.on_click(cx.listener(
                move |this, _event: &ClickEvent, _window, cx| {
                    this.retry_with_encoding(Some(label), cx);
                },
            )));
        }

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("Reader.typography.encoding").to_string()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(effective.unwrap_or_else(|| {
                                t!("Reader.typography.encoding_auto").to_string()
                            })),
                    ),
            )
            .child(buttons)
    }

    fn render_field_row(field: Field, reader: &ReaderConfig, cx: &Context<Self>) -> Div {
        let value = field.value(reader);
        let minus = Button::new(ElementId::Name(
            format!("reader-{}-minus", field.slug()).into(),
        ))
        .ghost()
        .compact()
        .icon(IconName::Minus)
        .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
            this.update_reader_config(
                move |reader| {
                    let next = field.value(reader) - field.step();
                    field.set(reader, next);
                },
                cx,
            );
        }));
        let plus = Button::new(ElementId::Name(
            format!("reader-{}-plus", field.slug()).into(),
        ))
        .ghost()
        .compact()
        .icon(IconName::Plus)
        .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
            this.update_reader_config(
                move |reader| {
                    let next = field.value(reader) + field.step();
                    field.set(reader, next);
                },
                cx,
            );
        }));
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!(field.label_key()).to_string()),
            )
            .child(minus)
            .child(
                div()
                    .w(px(48.0))
                    .text_center()
                    .text_sm()
                    .child(field.format(value)),
            )
            .child(plus)
    }

    fn render_indent_row(indent: bool, cx: &Context<Self>) -> Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("Reader.typography.indent").to_string()),
            )
            .child(
                Switch::new("reader-indent")
                    .checked(indent)
                    .on_click(cx.listener(|this, checked: &bool, _window, cx| {
                        let checked = *checked;
                        this.update_reader_config(move |reader| reader.indent = checked, cx);
                    })),
            )
    }
}

/// 带小标题的一组设置。
fn section(title_key: &str, content: impl IntoElement, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t!(title_key).to_string()),
        )
        .child(content)
}
