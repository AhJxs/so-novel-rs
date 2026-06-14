//! 统计 chip + 空态视图。

use super::color::semantic_muted;

/// 统计 chip：左侧 material 图标（彩色）+ 标签 + 加粗数字。
pub fn stat_chip(
    ui: &mut egui::Ui,
    icon: crate::material_icons::MaterialIcon,
    label: &str,
    count: usize,
    color: egui::Color32,
) {
    const ICON_SIZE: f32 = 14.0;
    const PAD_X: f32 = 10.0;
    const GAP_AFTER_ICON: f32 = 6.0;
    const GAP_BEFORE_COUNT: f32 = 6.0;
    const ROUNDING: u8 = 12;
    const CHIP_HEIGHT: f32 = 24.0;

    let dark = ui.style().visuals.dark_mode;
    let visuals = ui.style().visuals.clone();

    let body_font = egui::FontId::proportional(
        ui.style()
            .text_styles
            .get(&egui::TextStyle::Body)
            .map(|f| f.size)
            .unwrap_or(13.0),
    );
    let count_font = egui::FontId::proportional(body_font.size + 1.0);
    let icon_font = egui::FontId::new(ICON_SIZE, icon.font_family());
    let count_text = count.to_string();

    let icon_galley = ui
        .painter()
        .layout_no_wrap(icon.codepoint.to_string(), icon_font, color);
    let label_galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), body_font, visuals.text_color());
    let count_galley = ui
        .painter()
        .layout_no_wrap(count_text, count_font, color);

    let total_w = PAD_X
        + icon_galley.size().x
        + GAP_AFTER_ICON
        + label_galley.size().x
        + GAP_BEFORE_COUNT
        + count_galley.size().x
        + PAD_X;
    let desired = egui::vec2(total_w, CHIP_HEIGHT);

    let (rect, _resp) = ui.allocate_exact_size(desired, egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter();

    let bg = egui::Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        if dark { 32 } else { 22 },
    );
    let stroke_color =
        egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 140);
    painter.rect_filled(rect, egui::CornerRadius::same(ROUNDING), bg);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(ROUNDING),
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );

    let center_y = rect.center().y;
    let mut x = rect.left() + PAD_X;

    let icon_anchor = egui::pos2(x, center_y - icon_galley.mesh_bounds.center().y);
    let icon_w = icon_galley.size().x;
    painter.galley(icon_anchor, icon_galley, color);
    x += icon_w + GAP_AFTER_ICON;

    let label_anchor = egui::pos2(x, center_y - label_galley.mesh_bounds.center().y);
    let label_w = label_galley.size().x;
    painter.galley(label_anchor, label_galley, visuals.text_color());
    x += label_w + GAP_BEFORE_COUNT;

    let count_anchor = egui::pos2(x, center_y - count_galley.mesh_bounds.center().y);
    painter.galley(count_anchor, count_galley, color);
}

/// 通用空态视图：大号图标 + 主文案 + 副文案，水平居中。
pub fn empty_state(
    ui: &mut egui::Ui,
    icon: crate::material_icons::MaterialIcon,
    primary: &str,
    secondary: &str,
) {
    ui.add_space(24.0);
    const ICON_SIZE: f32 = 48.0;
    let dark = ui.style().visuals.dark_mode;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            ui.label(
                icon.rich_text()
                    .size(ICON_SIZE)
                    .color(semantic_muted(dark)),
            );
            ui.add_space(10.0);
            ui.label(egui::RichText::new(primary).size(16.0).strong());
            ui.add_space(6.0);
            ui.label(egui::RichText::new(secondary).size(14.0).weak());
        },
    );
}
