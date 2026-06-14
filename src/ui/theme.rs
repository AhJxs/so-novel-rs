//! 主题与字体初始化。
//!
//! egui 默认字体不包含 CJK，运行后中文会显示为 `▯`（豆腐块）。
//! 本模块尝试从系统字体目录加载一个常见 CJK 字体，找不到时给出 warn 日志，
//! 但不阻塞应用启动（用户至少能看到非 CJK 文本）。

use std::path::PathBuf;
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// 安装 CJK 字体。按平台尝试一组常见字体路径。
pub fn install_cjk_fonts(ctx: &egui::Context) {
    let Some((name, bytes)) = load_first_available_cjk_font() else {
        tracing::warn!("未找到系统 CJK 字体，中文可能显示为豆腐块。");
        return;
    };

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, name.clone());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .push(name);

    ctx.set_fonts(fonts);
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

/// 顶部导航条 frame：底边 1px 分隔线 + 上下 padding。
pub fn nav_frame(style: &egui::Style) -> egui::Frame {
    let visuals = &style.visuals;
    egui::Frame::new()
        .fill(visuals.panel_fill)
        .inner_margin(egui::Margin::symmetric(0, 4))
        .stroke(egui::Stroke::new(
            1.0,
            visuals.widgets.noninteractive.bg_stroke.color,
        ))
}

/// 顶部标题栏（窗口控制按钮所在）frame：无内外边距，紧贴顶部。
/// 高度由 panel 自己根据内容（28px 按钮 + 一点上下空隙）决定。
pub fn title_bar_frame(style: &egui::Style) -> egui::Frame {
    let visuals = &style.visuals;
    egui::Frame::new()
        .fill(visuals.panel_fill)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .outer_margin(egui::Margin::ZERO)
}

/// 主面板通用 padding frame：8px 内边距 + `visuals.panel_fill` 背景。
///
/// **没有 outer_margin** — outer_margin 是透明的，OS 窗口背景（深色模式
/// 默认黑色）会透过 panel 边缘显示成"一圈黑边"。
/// panel 直接贴到 OS 窗口边框（OS 圆角本身提供视觉边界），
/// 用 inner_margin(8) 给内容与面板边缘一点呼吸感。
pub fn content_frame(visuals: &egui::Visuals) -> egui::Frame {
    egui::Frame::new()
        .fill(visuals.panel_fill)
        .inner_margin(egui::Margin::same(8))
}

/// 强调色（导航选中按钮 / 卡片选中边框 / 主题切换选中态等）。
/// 在 dark / light 主题下都用同一组 RGB，与原生 macOS / Win 11 的"亮蓝"系一致。
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(58, 134, 230);

/// 主题感知的语义色（status / banner / 错误提示等）。
///
/// 暗色主题用稍亮、稍偏冷的色调，避免在深色背景上看着发"闷"；
/// 浅色主题用稍重、饱和度更高的色调，避免在白底上看着发"虚"。
///
/// 用 `ui.style().visuals.dark_mode` 决定取哪一组：
/// ```ignore
/// let danger = theme::semantic_danger(ui.style().visuals.dark_mode);
/// ```
pub fn semantic_danger(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(240, 110, 110)
    } else {
        egui::Color32::from_rgb(220, 80, 80)
    }
}

pub fn semantic_warn(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(240, 160, 90)
    } else {
        egui::Color32::from_rgb(220, 130, 60)
    }
}

pub fn semantic_success(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(110, 200, 140)
    } else {
        egui::Color32::from_rgb(80, 170, 110)
    }
}

pub fn semantic_info(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(140, 180, 240)
    } else {
        egui::Color32::from_rgb(120, 160, 220)
    }
}

/// 副标题 / 弱提示色 — 比 `weak_text_color` 更暗，但仍可读。
pub fn semantic_muted(dark: bool) -> egui::Color32 {
    if dark {
        egui::Color32::from_rgb(160, 160, 160)
    } else {
        egui::Color32::from_rgb(140, 140, 140)
    }
}

/// 统一按钮工厂：固定字号 14pt，跨主题按钮高度一致。
///
/// **为什么不用 `ui.button("text")` 直接调用**：egui 的 `Button` 默认用
/// `TextStyle::Button` 的字号 — 这个字号会随主题变化（浅色模式默认 14，
/// 深色模式可能不同），导致同一按钮在主题切换时高度跳变。
/// 这里显式 `RichText::new(text).size(14.0)` 锁死视觉字号，按钮高度稳定。
///
/// 用法：
/// ```ignore
/// if theme::button(ui, "查看任务").clicked() { ... }
/// if theme::small_button(ui, "删除").clicked() { ... }
/// if theme::action_button(ui, "下载").clicked() { ... }
/// ```
pub fn button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(14.0))
            .corner_radius(egui::CornerRadius::same(6)),
    )
}

pub fn small_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(13.0))
            .corner_radius(egui::CornerRadius::same(4))
            .small(),
    )
}

/// 次要操作按钮：比 `button` 更"按钮感"——更大的内边距和最小高度、圆角 8px。
///
/// 与 `button` 区别：
/// - `button_padding` 14/8（默认 10/6）→ 横向更舒展、整体更高
/// - 圆角 8 跟导航栏 / 输入框 / 下拉保持一致
/// - 强制 `min_size` 高度 30，避免"查看任务""下载"等一两字短文本在低密度行里看着太单薄
///
/// 用途：搜索结果卡的"下载"、底部横幅的"查看任务"这类需要稍显眼但不该是 ACCENT 实心
/// 填充的次要动作。视觉重量介于 `button` 和顶部"🔍 搜索"主按钮之间。
///
/// 实现：egui 0.34 的 `Button` 没有 `padding()`，只能改 style。这里用 `ui.scope`
/// 隔离样式修改，不污染外层（按钮绘制完出 scope 后样式自动还原）。
pub fn action_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let mut style = (**ui.style()).clone();
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    ui.scope(|ui| {
        ui.set_style(style);
        ui.add(
            egui::Button::new(egui::RichText::new(text).size(14.0))
                .corner_radius(egui::CornerRadius::same(8))
                .min_size(egui::vec2(0.0, 30.0)),
        )
    })
    .inner
}

fn load_first_available_cjk_font() -> Option<(String, Vec<u8>)> {
    for (name, path) in candidate_paths() {
        if let Ok(bytes) = std::fs::read(&path) {
            tracing::info!("CJK 字体已加载: {} ({})", name, path.display());
            return Some((name.to_string(), bytes));
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn candidate_paths() -> Vec<(&'static str, PathBuf)> {
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
fn candidate_paths() -> Vec<(&'static str, PathBuf)> {
    vec![
        (
            "PingFangSC",
            PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
        ),
        (
            "STHeiti",
            PathBuf::from("/System/Library/Fonts/STHeiti Light.ttc"),
        ),
        (
            "HiraginoSansGB",
            PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
        ),
    ]
}

#[cfg(target_os = "linux")]
fn candidate_paths() -> Vec<(&'static str, PathBuf)> {
    vec![
        (
            "NotoSansCJK",
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
        ),
        (
            "NotoSansCJK",
            PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
        ),
        (
            "WenQuanYiMicroHei",
            PathBuf::from("/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc"),
        ),
        (
            "WenQuanYiMicroHei",
            PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc"),
        ),
        (
            "SourceHanSans",
            PathBuf::from("/usr/share/fonts/adobe-source-han-sans/SourceHanSansSC-Regular.otf"),
        ),
    ]
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn candidate_paths() -> Vec<(&'static str, PathBuf)> {
    Vec::new()
}
