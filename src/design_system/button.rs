//! 按钮工厂：各样式按钮组件。

use super::color::{self, ACCENT};

/// 统一按钮工厂：固定字号 14pt，跨主题按钮高度一致。
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

/// 次要操作按钮：更大的内边距和最小高度、圆角 8px。
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

/// 设置页行内按钮：跟 40px 行高视觉对齐。
pub fn settings_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let mut style = (**ui.style()).clone();
    style.spacing.button_padding = egui::vec2(16.0, 8.0);
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

/// 亮蓝填充主按钮。`enabled = false` 时变灰。返回 click 状态。
pub fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    solid_button(ui, text, enabled, ACCENT, super::input::QUERY_HEIGHT)
}

/// 危险色按钮（清除记录 / 删除等破坏性操作）。
pub fn danger_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let dark = ui.style().visuals.dark_mode;
    solid_button(ui, text, enabled, color::semantic_danger(dark), super::input::QUERY_HEIGHT)
}

/// 通用实心按钮工厂：圆角 8 + 阴影 + 按下下沉 1px + hover/pressed 自动派生色阶。
///
/// 返回 true 表示**这一帧被点击**。
pub fn solid_button(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    base_color: egui::Color32,
    height: f32,
) -> bool {
    const BTN_ROUNDING: egui::CornerRadius = egui::CornerRadius::same(8);
    const BTN_PADDING_X: f32 = 18.0;

    let visuals = ui.style().visuals.clone();
    let dark_mode = visuals.dark_mode;
    let font_id = egui::FontId::proportional(
        ui.style()
            .text_styles
            .get(&egui::TextStyle::Button)
            .map(|f| f.size)
            .unwrap_or(14.0),
    );

    let painter_galley =
        ui.painter()
            .layout_no_wrap(text.to_string(), font_id.clone(), egui::Color32::WHITE);
    let text_w = painter_galley.size().x;
    let desired_size = egui::vec2(text_w + BTN_PADDING_X * 2.0, height);

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(desired_size, sense);

    if !ui.is_rect_visible(rect) {
        return false;
    }

    let painter = ui.painter();
    let is_pressed = enabled && response.is_pointer_button_down_on();
    let is_hovered = enabled && response.hovered();

    let (fill, text_color) = if !enabled {
        (visuals.widgets.inactive.bg_fill, visuals.weak_text_color())
    } else if is_pressed {
        (darken(base_color, 0.15), egui::Color32::WHITE)
    } else if is_hovered {
        (lighten(base_color, 0.10), egui::Color32::WHITE)
    } else {
        (base_color, egui::Color32::WHITE)
    };

    let press_offset = if is_pressed {
        egui::vec2(0.0, 1.0)
    } else {
        egui::vec2(0.0, 0.0)
    };
    let rect = rect.translate(press_offset);

    if enabled {
        let layers: [(f32, u8); 3] = if is_pressed {
            if dark_mode {
                [(0.0, 35), (1.0, 18), (1.5, 8)]
            } else {
                [(0.0, 16), (1.0, 8), (1.5, 4)]
            }
        } else if dark_mode {
            [(0.0, 70), (1.5, 40), (3.0, 18)]
        } else {
            [(0.0, 32), (1.5, 18), (3.0, 8)]
        };
        let shadow_dy = if is_pressed { 1.5 } else { 3.0 };
        for (expand, alpha) in layers {
            let shadow_rect = rect.translate(egui::vec2(0.0, shadow_dy)).expand(expand);
            painter.rect_filled(
                shadow_rect,
                egui::CornerRadius::same((8.0 + expand).round() as u8),
                egui::Color32::from_black_alpha(alpha),
            );
        }
    }

    painter.rect_filled(rect, BTN_ROUNDING, fill);

    let galley = painter.layout_no_wrap(text.to_string(), font_id, text_color);
    let mesh = galley.mesh_bounds;
    let anchor = rect.center() - mesh.center().to_vec2();
    painter.galley(anchor, galley, text_color);

    response.clicked()
}

fn darken(c: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = 1.0 - t;
    egui::Color32::from_rgb(
        (c.r() as f32 * f) as u8,
        (c.g() as f32 * f) as u8,
        (c.b() as f32 * f) as u8,
    )
}

fn lighten(c: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgb(
        (c.r() as f32 + (255.0 - c.r() as f32) * t) as u8,
        (c.g() as f32 + (255.0 - c.g() as f32) * t) as u8,
        (c.b() as f32 + (255.0 - c.b() as f32) * t) as u8,
    )
}
