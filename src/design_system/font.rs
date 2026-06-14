//! CJK 字体安装 + 视觉风格初始化。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// 项目内打包的 Noto Sans SC 字体目录。运行时按 cwd 解析，
/// `cargo run` 时 cwd 是仓库根，`bundle/fonts/...` 直接可读。
const BUNDLE_FONTS_DIR: &str = "bundle/fonts";

/// Noto Sans SC 全 weight 列表 + 在 egui 里的 font_data 注册名。
const BUNDLED_NOTO_SANS_SC: &[(&str, &str)] = &[
    ("noto_sans_sc_regular", "NotoSansSC-Regular.ttf"),
    ("noto_sans_sc_bold", "NotoSansSC-Bold.ttf"),
    ("noto_sans_sc_medium", "NotoSansSC-Medium.ttf"),
    ("noto_sans_sc_semibold", "NotoSansSC-SemiBold.ttf"),
    ("noto_sans_sc_extrabold", "NotoSansSC-ExtraBold.ttf"),
    ("noto_sans_sc_black", "NotoSansSC-Black.ttf"),
    ("noto_sans_sc_light", "NotoSansSC-Light.ttf"),
    ("noto_sans_sc_extralight", "NotoSansSC-ExtraLight.ttf"),
    ("noto_sans_sc_thin", "NotoSansSC-Thin.ttf"),
];

/// 安装 CJK 字体。
///
/// 优先级：
/// 1. `bundle/fonts/NotoSansSC-*.ttf`（项目内打包，全 9 个 weight）— 命中任一即用，
///    缺失的 weight warn 但不阻塞，剩下的 weight 继续注册。
/// 2. 全部 9 个都找不到 → 回落到系统 CJK 字体（仅取第一个可用的）。
/// 3. 都没有 → warn 后 return，不调 `set_fonts`（用 egui 默认字体启动，非 CJK 仍可读）。
pub fn install_cjk_fonts(ctx: &egui::Context) {
    let loaded = load_bundled_noto_sans_sc().unwrap_or_default();
    let loaded = if loaded.is_empty() {
        tracing::warn!("bundle/fonts/ 下找不到任何 Noto Sans SC weight，回落到系统 CJK 字体");
        load_first_available_system_cjk_font()
            .map(|one| vec![one])
            .unwrap_or_default()
    } else {
        tracing::info!("已加载 {} 个 Noto Sans SC weight", loaded.len());
        loaded
    };

    if loaded.is_empty() {
        tracing::warn!("未找到任何 CJK 字体，中文可能显示为豆腐块。");
        return;
    }

    let mut fonts = FontDefinitions::default();
    for (name, bytes) in &loaded {
        fonts
            .font_data
            .insert(name.clone(), Arc::new(FontData::from_owned(bytes.clone())));
    }

    {
        let prop = fonts.families.entry(FontFamily::Proportional).or_default();
        for (name, _) in loaded.iter().rev() {
            prop.insert(0, name.clone());
        }
    }
    {
        let mono = fonts.families.entry(FontFamily::Monospace).or_default();
        for (name, _) in &loaded {
            mono.push(name.clone());
        }
    }

    ctx.set_fonts(fonts);
}

fn load_bundled_noto_sans_sc() -> Option<Vec<(String, Vec<u8>)>> {
    let dir = Path::new(BUNDLE_FONTS_DIR);
    if !dir.is_dir() {
        return None;
    }
    let mut loaded = Vec::new();
    for (name, filename) in BUNDLED_NOTO_SANS_SC {
        let path = dir.join(filename);
        match std::fs::read(&path) {
            Ok(bytes) => {
                tracing::debug!("Noto Sans SC weight 已加载: {} ({} bytes)", name, bytes.len());
                loaded.push((name.to_string(), bytes));
            }
            Err(e) => {
                tracing::warn!("Noto Sans SC weight 缺失: {} ({e})", path.display());
            }
        }
    }
    if loaded.is_empty() { None } else { Some(loaded) }
}

fn load_first_available_system_cjk_font() -> Option<(String, Vec<u8>)> {
    for (name, path) in system_cjk_candidate_paths() {
        if let Ok(bytes) = std::fs::read(&path) {
            tracing::info!("系统 CJK 字体已加载: {} ({})", name, path.display());
            return Some((name.to_string(), bytes));
        }
    }
    None
}

/// 现代化外观：稍紧凑的 spacing、跟随系统的浅/深色。
pub fn install_visuals(ctx: &egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.window_margin = egui::Margin::same(8);
    style.visuals.window_corner_radius = egui::CornerRadius::same(6);
    ctx.set_global_style(style);
}

#[cfg(target_os = "windows")]
fn system_cjk_candidate_paths() -> Vec<(&'static str, PathBuf)> {
    let win_fonts = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts");
    vec![
        ("MicrosoftYaHei", win_fonts.join("msyh.ttc")),
        ("MicrosoftYaHei", win_fonts.join("msyh.ttf")),
        ("SimHei", win_fonts.join("simhei.ttf")),
        ("SimSun", win_fonts.join("simsun.ttc")),
        ("MicrosoftJhengHei", win_fonts.join("msjh.ttc")),
    ]
}

#[cfg(target_os = "macos")]
fn system_cjk_candidate_paths() -> Vec<(&'static str, PathBuf)> {
    vec![
        ("PingFangSC", PathBuf::from("/System/Library/Fonts/PingFang.ttc")),
        ("STHeiti", PathBuf::from("/System/Library/Fonts/STHeiti Light.ttc")),
        ("HiraginoSansGB", PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc")),
    ]
}

#[cfg(target_os = "linux")]
fn system_cjk_candidate_paths() -> Vec<(&'static str, PathBuf)> {
    vec![
        ("NotoSansCJK", PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc")),
        ("NotoSansCJK", PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc")),
        ("WenQuanYiMicroHei", PathBuf::from("/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc")),
        ("WenQuanYiMicroHei", PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc")),
        ("SourceHanSans", PathBuf::from("/usr/share/fonts/adobe-source-han-sans/SourceHanSansSC-Regular.otf")),
    ]
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn system_cjk_candidate_paths() -> Vec<(&'static str, PathBuf)> {
    Vec::new()
}
