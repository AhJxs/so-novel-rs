//! 主题偏好枚举 + 段控选择器。

use serde::{Deserialize, Serialize};

use super::color::ACCENT;

/// 主题偏好：浅色 / 跟随系统 / 深色。
#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ThemePref {
    Light,
    System,
    Dark,
}

impl ThemePref {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemePref::Light => "light",
            ThemePref::System => "system",
            ThemePref::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => ThemePref::Light,
            "dark" => ThemePref::Dark,
            _ => ThemePref::System,
        }
    }

    pub fn to_theme_preference(self) -> egui::ThemePreference {
        match self {
            ThemePref::Light => egui::ThemePreference::Light,
            ThemePref::System => egui::ThemePreference::System,
            ThemePref::Dark => egui::ThemePreference::Dark,
        }
    }
}

/// 主题切换段控（segmented control）：☀ 浅色 / 🌙 深色 / 💻 跟随系统。
///
/// 三个圆角图标按钮拼成一个胶囊；当前选中态用 `ACCENT` 浅底色 + 蓝边。
/// 返回 true 表示主题偏好在这帧被用户改变了。
pub fn theme_segmented_control(
    ui: &mut egui::Ui,
    current: &mut ThemePref,
) -> bool {
    let before = *current;
    let dark_mode = ui.style().visuals.dark_mode;

    if theme_icon_button(ui, ThemeIcon::Light, *current == ThemePref::Light, dark_mode)
        .on_hover_text("浅色")
        .clicked()
    {
        *current = ThemePref::Light;
    }
    if theme_icon_button(ui, ThemeIcon::Dark, *current == ThemePref::Dark, dark_mode)
        .on_hover_text("深色")
        .clicked()
    {
        *current = ThemePref::Dark;
    }
    if theme_icon_button(ui, ThemeIcon::System, *current == ThemePref::System, dark_mode)
        .on_hover_text("跟随系统")
        .clicked()
    {
        *current = ThemePref::System;
    }

    *current != before
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum ThemeIcon {
    Light,
    System,
    Dark,
}

fn theme_icon_button(
    ui: &mut egui::Ui,
    icon: ThemeIcon,
    selected: bool,
    dark_mode: bool,
) -> egui::Response {
    use crate::material_icons::icons as mi;

    let size = egui::vec2(30.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();
    let rounding = egui::CornerRadius::same(6);

    if selected {
        painter.rect_filled(
            rect,
            rounding,
            egui::Color32::from_rgba_unmultiplied(58, 134, 230, if dark_mode { 50 } else { 35 }),
        );
        painter.rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(1.0, ACCENT),
            egui::StrokeKind::Inside,
        );
    } else if response.hovered() {
        painter.rect_filled(
            rect,
            rounding,
            if dark_mode {
                egui::Color32::from_white_alpha(20)
            } else {
                egui::Color32::from_black_alpha(12)
            },
        );
    }

    let icon_color = if selected {
        ACCENT
    } else if dark_mode {
        egui::Color32::from_white_alpha(200)
    } else {
        egui::Color32::from_black_alpha(180)
    };

    let mi_icon = match icon {
        ThemeIcon::Light => mi::ICON_LIGHT_MODE,
        ThemeIcon::System => mi::ICON_COMPUTER,
        ThemeIcon::Dark => mi::ICON_DARK_MODE,
    };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        mi_icon.codepoint,
        egui::FontId::new(18.0, mi_icon.font_family()),
        icon_color,
    );

    response
}
