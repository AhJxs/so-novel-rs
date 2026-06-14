//! 搜索栏 / 过滤栏共用控件。
//!
//! 设计语言：固定行高 34px、圆角 8px、与文字色一致的细边框。

/// 搜索栏 / 过滤栏的统一行高。
pub const QUERY_HEIGHT: f32 = 34.0;

/// 圆角输入框（前置 material 图标 + 单行 TextEdit）。
///
/// 返回值：`(response, enter_pressed)`。
pub fn search_input(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    icon: crate::material_icons::MaterialIcon,
    width: f32,
) -> (egui::Response, bool) {
    let visuals = ui.style().visuals.clone();
    const ICON_W: f32 = 22.0;

    let input_frame = egui::Frame::new()
        .fill(visuals.extreme_bg_color)
        .stroke(egui::Stroke::new(1.0, visuals.text_color()))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 0));

    let mut enter_pressed = false;
    let mut edit_resp: Option<egui::Response> = None;

    ui.scope(|ui| {
        ui.set_max_width(width);
        input_frame.show(ui, |ui| {
            ui.set_min_size(egui::vec2(width, QUERY_HEIGHT));
            ui.horizontal_centered(|ui| {
                ui.label(
                    icon.rich_text()
                        .size(14.0)
                        .color(visuals.weak_text_color()),
                );
                let edit_w = width - ICON_W - 20.0;
                let edit = egui::TextEdit::singleline(text)
                    .hint_text(hint)
                    .frame(egui::Frame::NONE)
                    .desired_width(edit_w)
                    .vertical_align(egui::Align::Center);
                let resp = ui.add(edit);
                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    enter_pressed = true;
                }
                edit_resp = Some(resp);
            });
        });
    });

    (edit_resp.expect("text edit always allocated"), enter_pressed)
}

/// 圆角 ComboBox 包装：固定行高 34px、圆角 8px、自定义上下三角箭头。
pub fn rounded_combo<R>(
    ui: &mut egui::Ui,
    id_salt: &str,
    selected_text: impl Into<egui::WidgetText>,
    width: f32,
    render_items: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let mut inner: Option<R> = None;
    ui.allocate_ui_with_layout(
        egui::vec2(width, QUERY_HEIGHT),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            let mut style: egui::Style = (**ui.style()).clone();
            let r = egui::CornerRadius::same(8);
            style.visuals.widgets.inactive.corner_radius = r;
            style.visuals.widgets.hovered.corner_radius = r;
            style.visuals.widgets.active.corner_radius = r;
            style.visuals.widgets.open.corner_radius = r;
            style.spacing.button_padding = egui::vec2(8.0, 0.0);
            ui.set_style(style);

            egui::ComboBox::from_id_salt(id_salt)
                .selected_text(selected_text)
                .width(width)
                .height(360.0)
                .icon(|ui, rect, vis, is_open| {
                    let painter = ui.painter();
                    let center = rect.center();
                    let h = (rect.height() * 0.18).clamp(3.0, 5.0);
                    let w = h * 1.4;
                    let dir = if is_open { -1.0 } else { 1.0 };
                    let p1 = egui::pos2(center.x - w, center.y - h * dir);
                    let p2 = egui::pos2(center.x + w, center.y - h * dir);
                    let p3 = egui::pos2(center.x, center.y + h * dir);
                    painter.add(egui::Shape::convex_polygon(
                        vec![p1, p2, p3],
                        vis.fg_stroke.color,
                        egui::Stroke::NONE,
                    ));
                })
                .show_ui(ui, |ui| {
                    inner = Some(render_items(ui));
                });
        },
    );
    inner
}
