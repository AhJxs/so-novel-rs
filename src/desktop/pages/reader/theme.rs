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
    match theme {
        ReaderTheme::Paper => palette(0x00f7_f4ec, 0x002b_2a26, 0x00f3_d98b, 0x00e8_b93f),
        ReaderTheme::Sepia => palette(0x00f0_e2c8, 0x004a_3b24, 0x00e9_cf95, 0x00d9_a94a),
        ReaderTheme::Night => palette(0x001c_1d21, 0x00c9_c9c4, 0x004a_4a20, 0x00b8_a54a),
        ReaderTheme::Green => palette(0x00dc_e8d5, 0x0024_331f, 0x00bc_d98f, 0x008f_bf5a),
        ReaderTheme::App => ReaderColors {
            background: cx.theme().background,
            foreground: cx.theme().foreground,
            highlight: cx.theme().selection,
            active_highlight: cx.theme().accent,
        },
    }
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

    /// 每个内置主题的背景 / 前景都必须与调色板表一致：改一个十六进制值就会红。
    #[test]
    fn palettes_match_the_table() {
        let paper = palette(0xf7_f4ec, 0x2b_2a_26, 0xf3_d98b, 0xe8_b93f);
        assert_eq!(paper.background, rgb(0xf7_f4ec));
        assert_eq!(paper.foreground, rgb(0x2b_2a_26));
        assert_eq!(paper.highlight, rgb(0xf3_d98b));
        assert_eq!(paper.active_highlight, rgb(0xe8_b93f));

        let night = palette(0x1c_1d21, 0xc9_c9c4, 0x4a_4a20, 0xb8_a54a);
        assert_eq!(night.background, rgb(0x1c_1d21));
        assert_ne!(night.background, paper.background);
    }
}
