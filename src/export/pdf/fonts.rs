//! CJK 字体发现 + 字宽量宽器
//!
//! 跨系统找一份 CJK 字体 (TTC/TTF/OTF), 找不到时降级到启发式量宽 (CJK=1em, ASCII=0.55em)。

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use pdf_oxide::writer::EmbeddedFont;

/// 正文与标题用的 CJK 字体注册名 (找不到系统字体时退化为 Base-14 Helvetica)。
pub const CJK_FONT: &str = "CJK";

/// 字符宽度量宽器。
///
/// - `Embedded`: 用真实 CJK 字体字形度量 (精确, CJK 路径)。
/// - `Heuristic`: CJK=1em、ASCII=0.55em、空格=0.3em 的近似 (无字体降级路径)。
pub enum Measurer {
    Embedded {
        font: Box<EmbeddedFont>,
        width_cache: RefCell<HashMap<u32, f32>>,
    },
    Heuristic,
}

impl Measurer {
    /// 单字符宽度 (pt)。
    pub fn char_w(&self, ch: char, size: f32) -> f32 {
        match self {
            Self::Embedded { font, width_cache } => {
                let cp = ch as u32;
                // 字形原始宽度 (0–1000 units), 与 size 无关, 缓存 u32 键。
                let raw = *width_cache
                    .borrow_mut()
                    .entry(cp)
                    .or_insert_with(|| font.char_width(cp) as f32);
                raw * size / 1000.0
            }
            Self::Heuristic => {
                if ch == ' ' {
                    0.3 * size
                } else if ch.is_ascii() {
                    0.55 * size
                } else {
                    size // CJK 全角
                }
            }
        }
    }

    /// 字符串宽度 (pt)。
    pub fn text_w(&self, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.char_w(c, size)).sum()
    }
}

/// 跑遍常见系统路径找一份 CJK 字体 (TTC/TTF/OTF)。
///
/// # Examples
///
/// ```ignore
/// if let Some(bytes) = find_cjk_font() {
///     let font = EmbeddedFont::from_data(Some(CJK_FONT.into()), bytes)?;
///     // ...
/// }
/// ```
///
/// # Errors
///
/// 不返错误, 找不到任何 CANDIDATES 路径时返 `None` (由 caller 走降级路径)。
pub fn find_cjk_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        // Windows
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\simfang.ttf",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/Library/Fonts/Songti.ttc",
        // Linux — 主流发行版
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        // 用户目录 (手动放的)
        "fonts/NotoSansCJK-Regular.ttc",
        "assets/NotoSansCJK-Regular.ttc",
    ];
    for path in CANDIDATES {
        let p = Path::new(path);
        if !p.exists() {
            continue;
        }
        match fs::read(p) {
            Ok(bytes) if !bytes.is_empty() => {
                tracing::debug!("使用 CJK 字体: {}", p.display());
                return Some(bytes);
            }
            Ok(_) => {}
            Err(e) => {
                tracing::debug!("读 CJK 字体 {} 失败: {}", p.display(), e);
            }
        }
    }
    None
}
