//! 阅读主题 → 具体颜色。`src/reader` 保持 UI 中立，颜色只在桌面层出现。

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{Hsla, Rgba};

use crate::config::ReaderTheme;

/// 正文与阅读器外壳的前景 / 背景 / 高亮色。
#[derive(Debug, Clone, Copy)]
pub struct ReaderColors {
    pub background: Hsla,
    pub foreground: Hsla,
    // Task 20（书内搜索命中高亮）会读这两个字段；在那之前它们是唯一的读点缺口。
    #[allow(dead_code)]
    pub highlight: Hsla,
    #[allow(dead_code)]
    pub active_highlight: Hsla,
}

/// 按主题给出配色；`ReaderTheme::App` 跟随应用主题。
#[must_use]
pub fn reader_colors(theme: ReaderTheme, cx: &gpui_kit::App) -> ReaderColors {
    builtin_colors(theme).unwrap_or_else(|| ReaderColors {
        background: cx.theme().background,
        foreground: cx.theme().foreground,
        highlight: cx.theme().selection,
        active_highlight: cx.theme().accent,
    })
}

/// 内置主题的配色；`ReaderTheme::App` 跟随应用主题，没有内置表。
fn builtin_colors(theme: ReaderTheme) -> Option<ReaderColors> {
    let (background, foreground, highlight, active_highlight) = match theme {
        ReaderTheme::Paper => (0xf7_f4ec, 0x2b_2a_26, 0xf3_d98b, 0xe8_b93f),
        ReaderTheme::Sepia => (0xf0_e2c8, 0x4a_3b24, 0xe9_cf95, 0xd9_a94a),
        ReaderTheme::Night => (0x1c_1d21, 0xc9_c9c4, 0x4a_4a20, 0xb8_a54a),
        ReaderTheme::Green => (0xdc_e8d5, 0x24_331f, 0xbc_d98f, 0x8f_bf5a),
        ReaderTheme::App => return None,
    };
    Some(palette(background, foreground, highlight, active_highlight))
}

fn palette(
    background: u32,
    foreground: u32,
    highlight: u32,
    active_highlight: u32,
) -> ReaderColors {
    ReaderColors {
        background: rgb(background),
        foreground: rgb(foreground),
        highlight: rgb(highlight),
        active_highlight: rgb(active_highlight),
    }
}

fn rgb(value: u32) -> Hsla {
    // 先落 u8 再 `f32::from`：u32 直接转 f32 会触发 `cast_precision_loss`（不在全局 allow 名单），
    // 而 u8 → f32 是无损的。
    let red = f32::from(((value >> 16) & 0xff) as u8) / 255.0;
    let green = f32::from(((value >> 8) & 0xff) as u8) / 255.0;
    let blue = f32::from((value & 0xff) as u8) / 255.0;
    Rgba {
        r: red,
        g: green,
        b: blue,
        a: 1.0,
    }
    .into()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    /// 每个内置主题都必须可读：前景与背景对比度够，高亮与底色、当前命中与普通命中可区分。
    ///
    /// 故意不比对具体十六进制——换配色是设计决定，不该让测试红。这里钉的是真会写错的
    /// 性质：夜间主题写成浅色、前景与背景调反、高亮色撞上底色。
    #[test]
    fn builtin_palettes_stay_readable() {
        let themes = [
            ReaderTheme::Paper,
            ReaderTheme::Sepia,
            ReaderTheme::Night,
            ReaderTheme::Green,
        ];
        for theme in themes {
            let colors = builtin_colors(theme).expect("内置主题必须有调色板");
            let contrast = contrast_ratio(colors.background, colors.foreground);
            assert!(
                contrast >= 4.5,
                "{theme:?} 的前景与背景对比度只有 {contrast:.2}，读不下去"
            );
            assert_ne!(
                colors.highlight, colors.background,
                "{theme:?} 高亮与底色相同"
            );
            assert_ne!(
                colors.active_highlight, colors.highlight,
                "{theme:?} 当前命中与普通命中相同"
            );
        }
        assert!(
            builtin_colors(ReaderTheme::App).is_none(),
            "App 主题跟随应用主题，不能有内置表"
        );
    }

    fn contrast_ratio(first: Hsla, second: Hsla) -> f32 {
        let first = luminance(first);
        let second = luminance(second);
        let (lighter, darker) = if first >= second {
            (first, second)
        } else {
            (second, first)
        };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// WCAG 相对亮度。
    fn luminance(color: Hsla) -> f32 {
        let rgba = Rgba::from(color);
        let channel = |value: f32| {
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.0722f32.mul_add(
            channel(rgba.b),
            0.2126f32.mul_add(channel(rgba.r), 0.7152 * channel(rgba.g)),
        )
    }
}
