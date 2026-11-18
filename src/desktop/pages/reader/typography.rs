//! 顶栏 `Aa` 面板：主题、字号、行高、段距、页宽、首行缩进。

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::{ClickEvent, Context, Div, FontWeight, ParentElement, Styled, div, px};
use rust_i18n::t;

use super::reader_page::ReaderPage;
use crate::config::{ReaderConfig, ReaderTheme};

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
    /// 浮在正文右上角的排版面板；每次改动即时生效，关闭时才落盘。
    pub(super) fn render_typography(&self, cx: &Context<Self>) -> Div {
        let reader = self.reader_config(cx);
        let mut panel = div()
            .absolute()
            .top(px(52.0))
            .right(px(12.0))
            .w(px(300.0))
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_lg()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("Reader.typography.title").to_string()),
            )
            .child(Self::render_theme_row(reader.theme, cx));

        for field in Field::ALL {
            panel = panel.child(Self::render_field_row(field, &reader, cx));
        }

        panel.child(Self::render_indent_row(reader.indent, cx))
    }

    fn render_theme_row(current: ReaderTheme, cx: &Context<Self>) -> Div {
        let mut row = div().flex().flex_row().flex_wrap().gap_2();
        for theme in THEMES {
            let mut button = Button::new(format!("reader-theme-{}", theme.as_str()))
                .label(t!(theme.label_key()));
            button = if theme == current {
                button.primary()
            } else {
                button.ghost()
            };
            row = row.child(button.on_click(cx.listener(
                move |this, _event: &ClickEvent, _window, cx| {
                    this.update_reader_config(move |reader| reader.theme = theme, cx);
                },
            )));
        }
        row
    }

    fn render_field_row(field: Field, reader: &ReaderConfig, cx: &Context<Self>) -> Div {
        let value = field.value(reader);
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
            .child(
                Button::new(format!("reader-{}-minus", field.slug()))
                    .ghost()
                    .compact()
                    .label("−")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.update_reader_config(
                            move |reader| {
                                let next = field.value(reader) - field.step();
                                field.set(reader, next);
                            },
                            cx,
                        );
                    })),
            )
            .child(
                div()
                    .w(px(48.0))
                    .text_center()
                    .text_sm()
                    .child(field.format(value)),
            )
            .child(
                Button::new(format!("reader-{}-plus", field.slug()))
                    .ghost()
                    .compact()
                    .label("+")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.update_reader_config(
                            move |reader| {
                                let next = field.value(reader) + field.step();
                                field.set(reader, next);
                            },
                            cx,
                        );
                    })),
            )
    }

    fn render_indent_row(indent: bool, cx: &Context<Self>) -> Div {
        let mut button = Button::new("reader-indent").label(t!("Reader.typography.indent"));
        button = if indent {
            button.primary()
        } else {
            button.ghost()
        };
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
                button.on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                    this.update_reader_config(|reader| reader.indent = !reader.indent, cx);
                })),
            )
    }
}
