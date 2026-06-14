//! 主题与字体初始化。
//!
//! egui 默认字体不包含 CJK，运行后中文会显示为 `▯`（豆腐块）。
//! 本模块优先加载 `bundle/fonts/` 下打包的 Noto Sans SC 全 9 个 weight（Regular /
//! Medium / SemiBold / Bold / ExtraBold / Black + Light / ExtraLight / Thin），
//! 全部缺失时回落到系统 CJK 字体，找不到任何 CJK 字体时 warn 不阻塞启动。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// 项目内打包的 Noto Sans SC 字体目录。运行时按 cwd 解析，
/// `cargo run` 时 cwd 是仓库根，`bundle/fonts/...` 直接可读。
const BUNDLE_FONTS_DIR: &str = "bundle/fonts";

/// Noto Sans SC 全 weight 列表 + 在 egui 里的 font_data 注册名。
///
/// 顺序就是 Proportional 家族里的优先级（最前 = 最先尝试），也让
/// `RichText::strong()` 之类带"bold"暗示的渲染能找到对应的 Bold 字重。
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

    // Proportional 家族：反序 insert(0, ...) 让 `loaded[0]` = Regular 排在最前，
    // Bold / Medium 紧随其后（egui 的 "strong" 渲染会找名字含 "bold" 的字重）。
    // Monospace 家族：用 push，把 Noto SC 放到系统默认等宽字体之后，作为 CJK
    // 字符的兜底 — 这样代码里的中文不会回退到丑陋的方块字。
    // 注意：两个 `entry().or_default()` 不能同时持有 &mut 借用，所以各自套一个 scope
    // 让前一个借用出 scope 再借下一次。
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

/// 尝试从 `bundle/fonts/` 加载所有能读到的 Noto Sans SC weight。
/// 单个 weight 缺失不中断；返回加载成功的列表。
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

/// 加载系统中第一个可用的 CJK 字体（仅一个，作为打包字体缺失时的兜底）。
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
fn system_cjk_candidate_paths() -> Vec<(&'static str, PathBuf)> {
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
fn system_cjk_candidate_paths() -> Vec<(&'static str, PathBuf)> {
    Vec::new()
}
