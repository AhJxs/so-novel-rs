//! 阅读主题 → 具体颜色。`src/reader` 保持 UI 中立，颜色只在桌面层出现。

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{Hsla, Rgba, hsla};

use crate::config::ReaderTheme;

/// 正文与阅读器外壳的前景 / 背景 / 高亮色。
///
/// `muted` / `highlight` / `active_highlight` 供 Task 18/19 的搜索高亮与弱化文字使用，
/// 本任务只渲染正文与外壳，暂无读点。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct ReaderColors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
    pub highlight: Hsla,
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
            muted: cx.theme().muted_foreground,
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
        muted: hsla(0.0, 0.0, 0.55, 1.0),
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
