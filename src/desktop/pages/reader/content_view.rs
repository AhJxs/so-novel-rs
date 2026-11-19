//! 正文测量与列渲染：滚动模式与翻页模式共用同一份列布局。
//!
//! 测量只依赖 `shape_text` 的折行数，分页纯函数在 `crate::reader::layout` 里，
//! 两边共用「行数」这一约定，所以测量对不上时能用纯函数测出来。

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    Div, FontWeight, HighlightStyle, IntoElement, ParentElement, SharedString, Styled, StyledText,
    TextStyle, WeakEntity, Window, canvas, div, px,
};

use super::reader_page::ReaderPage;
use crate::config::ReaderConfig;

/// 正文列左右留白（每侧 px）。测量与渲染必须用同一个值。
pub(super) const COLUMN_PADDING: f32 = 24.0;

/// 把一章渲染成一整列（含章标题）。翻页模式的外层再套一个裁剪视口。
/// `width` 由 `ReaderPage::content_width` 算好（已扣掉抽屉与页宽上限），
/// 与分页用的是同一个值，折行才对得上。
/// `highlights` 按段落索引给出高亮区间。
pub(super) fn chapter_column(
    title: &str,
    paragraphs: &[String],
    reader: &ReaderConfig,
    width: f32,
    line_height_px: f32,
    highlights: Option<&[Vec<(std::ops::Range<usize>, HighlightStyle)>]>,
) -> Div {
    let font_size = reader.font_size;
    let spacing = reader.paragraph_spacing;
    // `Option<&[...]>` 是 `Copy`：`move` 闭包捕获的只是切片引用，子元素在
    // `children()` 里同步构造完就散掉，不必每帧把整章区间拷成 owned。
    let highlight_rows = highlights;
    let mut column = div()
        .w(px(width))
        .flex_none()
        .flex()
        .flex_col()
        .child(
            div()
                .pb(px(line_height_px * 0.75))
                .text_size(px(font_size))
                .line_height(px(line_height_px))
                .font_weight(FontWeight::BOLD)
                .child(SharedString::from(title.to_string())),
        )
        .children(
            paragraphs
                .iter()
                .cloned()
                .enumerate()
                .map(move |(index, text)| {
                    let mut row = div()
                        .pb(px(spacing))
                        .text_size(px(font_size))
                        .line_height(px(line_height_px));
                    row = match highlight_rows
                        .as_ref()
                        .and_then(|rows| rows.get(index))
                        .filter(|ranges| !ranges.is_empty())
                    {
                        Some(ranges) => {
                            row.child(StyledText::new(text).with_highlights(ranges.clone()))
                        }
                        None => row.child(text),
                    };
                    row
                }),
        );
    if !reader.font_family.is_empty() {
        column = column.font_family(reader.font_family.clone());
    }
    column
}

/// 测量时用的文本样式：字号跟随排版设置，字体族配了就用配的。
pub(super) fn measure_style(window: &Window, reader: &ReaderConfig) -> TextStyle {
    let mut style = window.text_style();
    style.font_size = px(reader.font_size).into();
    if !reader.font_family.is_empty() {
        style.font_family = SharedString::from(reader.font_family.clone());
    }
    style
}

/// 一段文字在 `wrap_width` 下折成几行。`shape_text` 出错时按字符宽度估算。
pub(super) fn measure_lines(
    window: &Window,
    text: &str,
    font_size: f32,
    wrap_width: f32,
    style: &TextStyle,
) -> usize {
    if text.is_empty() {
        return 1;
    }
    let run = style.to_run(text.len());
    let shaped = window.text_system().shape_text(
        SharedString::from(text.to_string()),
        px(font_size),
        std::slice::from_ref(&run),
        Some(px(wrap_width)),
        None,
    );
    match shaped {
        Ok(lines) if !lines.is_empty() => lines
            .iter()
            .map(|line| line.wrap_boundaries.len() + 1)
            .sum::<usize>()
            .max(1),
        _ => {
            // 字符数 → f32 只经由 `layout::count_f32` 这一处转换（那里有唯一一条
            // `cast_precision_loss` 的说明性放行），这里再写一个 `as f32` 会多一条 allow。
            let chars = crate::reader::layout::count_f32(text.chars().count());
            ((chars * font_size / wrap_width.max(1.0)).ceil() as usize).max(1)
        }
    }
}

/// 把正文视口的实测高度回写到页面。
///
/// `canvas` 的 prepaint 拿得到真实 `bounds` 但没有 view `Context`，所以先把值丢给
/// `window.on_next_frame`，由下一帧再更新 view 并触发一次重排。
pub(super) fn viewport_probe(
    page: WeakEntity<ReaderPage>,
    measured: Rc<Cell<Option<f32>>>,
) -> impl IntoElement {
    canvas(
        move |bounds, window, _cx| {
            let height = f32::from(bounds.size.height);
            if height <= 0.0 {
                return;
            }
            let changed = measured
                .get()
                .is_none_or(|previous| (previous - height).abs() > 0.5);
            if !changed {
                return;
            }
            measured.set(Some(height));
            window.on_next_frame(move |_window, cx| {
                let _ = page.update(cx, |page, cx| page.set_viewport_height(height, cx));
            });
        },
        |_bounds, (), _window, _cx| {},
    )
    // `Canvas` 是 `Style::default()` 的 block 叶子，auto 高度恒为 0，prepaint 拿到的
    // bounds 也就恒为 0 高。绝对定位铺满父容器（父容器已有 `.relative()`），
    // 这样量到的就是裁剪盒的 padding box 高度。
    .absolute()
    .inset_0()
}
