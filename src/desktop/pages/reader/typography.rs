//! 「阅读设置」弹窗：实时预览、主题、字号 / 行高 / 段距 / 页宽、首行缩进、编码。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::select::Select;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, IconName};
use gpui_kit::prelude::FluentBuilder as _;
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

/// 面板里的五档字重（值，文案 key）。gpui 的 `FontWeight` 只认具体数值，
/// 阅读场景五档足够，不给任意输入。
const WEIGHTS: [(u16, &str); 5] = [
    (300, "Reader.typography.weight.light"),
    (400, "Reader.typography.weight.regular"),
    (500, "Reader.typography.weight.medium"),
    (600, "Reader.typography.weight.semibold"),
    (700, "Reader.typography.weight.bold"),
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
                    .font_weight(FontWeight(f32::from(reader.font_weight)))
                    .when(!reader.font_family.is_empty(), |this| {
                        this.font_family(reader.font_family.clone())
                    })
                    .text_color(colors.foreground)
                    .child(preview_text),
            );

        let mut body = div()
            .id("reader-settings-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_5()
            .py_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(section("Reader.typography.preview_title", preview, cx))
            .child(Self::render_theme_section(reader.theme, cx))
            .child(divider(cx))
            .child(self.render_font_section(&reader, cx))
            .child(divider(cx))
            .child(Self::render_layout_section(&reader, cx));
        // 编码只对 TXT 有意义；放在这里，用户随时能改回自动或换一个标签。
        if matches!(self.book.format, BookFormat::Txt) {
            body = body
                .child(divider(cx))
                .child(self.render_encoding_section(cx));
        }

        let card = div()
            .id("reader-settings-card")
            .w(px(560.0))
            .h(px(640.0))
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
            .occlude()
            // 卡片内部的点击不要冒泡到遮罩层，否则改设置会顺手关掉弹窗。
            .on_mouse_down(MouseButton::Left, |_event, _window, cx| {
                cx.stop_propagation();
            })
            .on_click(|_event: &ClickEvent, _window, cx| {
                cx.stop_propagation();
            })
            // 滚轮只作用于弹窗：遮罩下的正文滚轮监听不能同时被触发。
            .on_scroll(|_event, _window, cx| {
                cx.stop_propagation();
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_5()
                    .h(px(56.0))
                    .flex_none()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_base()
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
            // 遮罩挡住下面阅读器的 hitbox，滚轮 / 悬停不再落到正文上。
            .occlude()
            .on_scroll(|_event, _window, cx| {
                cx.stop_propagation();
            })
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
        section("Reader.typography.theme_title", row, cx)
    }

    /// 字体：可搜索的系统字体下拉（首项「系统默认」）+ 五档字重按钮。
    ///
    /// 下拉在打开面板时由 `ensure_font_select` 建好；理论上这里拿不到 `None`，
    /// 留空占位只是防布局塌一行。
    fn render_font_section(&self, reader: &ReaderConfig, cx: &Context<Self>) -> Div {
        let select = self.font_select.as_ref().map_or_else(
            || div().w(px(220.0)).into_any_element(),
            |state| {
                div()
                    .w(px(220.0))
                    .min_w_0()
                    .child(Select::new(state).w_full())
                    .into_any_element()
            },
        );
        let font_row = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .child(label_text("Reader.typography.font_family", cx))
            .child(select);

        let mut weight_buttons = div().flex().flex_row().items_center().gap_2();
        for (value, key) in WEIGHTS {
            let mut button = Button::new(ElementId::Name(format!("reader-weight-{value}").into()))
                .compact()
                .label(t!(key));
            button = if reader.font_weight == value {
                button.primary()
            } else {
                button.ghost()
            };
            weight_buttons = weight_buttons.child(button.on_click(cx.listener(
                move |this, _event: &ClickEvent, _window, cx| {
                    this.update_reader_config(move |reader| reader.font_weight = value, cx);
                },
            )));
        }
        let weights = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .child(label_text("Reader.typography.font_weight", cx))
            .child(weight_buttons);

        section(
            "Reader.typography.font",
            div()
                .flex()
                .flex_col()
                .gap_3()
                .child(font_row)
                .child(weights),
            cx,
        )
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

        let effective_label =
            effective.unwrap_or_else(|| t!("Reader.typography.encoding_auto").to_string());
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(section_title("Reader.typography.encoding", cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                t!(
                                    "Reader.typography.encoding_current",
                                    encoding = &effective_label
                                )
                                .to_string(),
                            ),
                    ),
            )
            .child(buttons)
    }

    /// 「排版」组：字号 / 行高 / 段距 / 页宽 / 首行缩进收进一个带分隔线的分组框。
    fn render_layout_section(reader: &ReaderConfig, cx: &Context<Self>) -> Div {
        let mut list = div()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .overflow_hidden();
        let last_index = Field::ALL.len().saturating_sub(1);
        for (index, field) in Field::ALL.into_iter().enumerate() {
            list = list.child(Self::render_field_row(
                field,
                reader,
                cx,
                index < last_index,
            ));
        }
        let list = list.child(Self::render_indent_row(reader.indent, cx, false));
        section("Reader.typography.title", list, cx)
    }

    fn render_field_row(
        field: Field,
        reader: &ReaderConfig,
        cx: &Context<Self>,
        bordered: bool,
    ) -> Div {
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
        list_row(bordered, cx)
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .child(t!(field.label_key()).to_string()),
            )
            .child(minus)
            .child(
                div()
                    .w(px(44.0))
                    .text_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(field.format(value)),
            )
            .child(plus)
    }

    fn render_indent_row(indent: bool, cx: &Context<Self>, bordered: bool) -> Div {
        list_row(bordered, cx)
            .child(
                div()
                    .flex_1()
                    .text_sm()
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

/// 组标题：小号、半粗、弱化的分组名。
fn section_title(title_key: &str, cx: &App) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(t!(title_key).to_string())
}

/// 带小标题的一组设置。
fn section(title_key: &str, content: impl IntoElement, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(section_title(title_key, cx))
        .child(content)
}

/// 各组之间的通栏细分隔线。
fn divider(cx: &App) -> Div {
    div().h(px(1.0)).w_full().flex_none().bg(cx.theme().border)
}

/// 表单行左侧的固定宽度标签，保证同一组里控件起点对齐。
fn label_text(label_key: &str, cx: &App) -> Div {
    div()
        .w(px(56.0))
        .flex_none()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(t!(label_key).to_string())
}

/// 分组框里的一行：40 高、左右内边距、除最后一行外底部一条分隔线。
fn list_row(bordered: bool, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .h(px(40.0))
        .when(bordered, |this| {
            this.border_b_1().border_color(cx.theme().border)
        })
}
