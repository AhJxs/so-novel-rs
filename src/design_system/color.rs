//! 配色常量 + 语义色函数。

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
/// let danger = color::semantic_danger(ui.style().visuals.dark_mode);
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
