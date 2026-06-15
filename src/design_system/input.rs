//! 搜索栏 / 设置页共用控件。
//!
//! 设计语言：固定行高 34px、圆角 8px、与文字色一致的细边框。

/// 搜索栏输入框/下拉框的统一行高。
pub const INPUT_HEIGHT: f32 = 34.0;

/// 圆角输入框（前置 material 图标 + 单行 TextEdit）。
///
/// 返回值：`(response, enter_pressed)`。
pub fn icon_text_input(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    icon: crate::material_icons::MaterialIcon,
    width: f32,
    height: f32,
) -> (egui::Response, bool) {
    let visuals = ui.style().visuals.clone();
    const PAD_X: f32 = 8.0;
    const ICON_SIZE: f32 = 14.0;
    const ICON_GAP: f32 = 6.0;

    // 1. 精确分配尺寸
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, height),
        egui::Sense::hover(),
    );

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();

        // 绘制背景 + 边框
        painter.rect_filled(rect, egui::CornerRadius::same(8), visuals.extreme_bg_color);
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(8),
            egui::Stroke::new(1.0, visuals.text_color()),
            egui::StrokeKind::Inside,
        );

        // 绘制图标
        let icon_galley = painter.layout_no_wrap(
            icon.codepoint.to_string(),
            egui::FontId::proportional(ICON_SIZE),
            visuals.weak_text_color(),
        );
        let icon_x = rect.min.x + PAD_X - icon_galley.mesh_bounds.min.x;
        let icon_y = rect.center().y - icon_galley.mesh_bounds.center().y;
        painter.galley(egui::pos2(icon_x, icon_y), icon_galley, visuals.weak_text_color());
    }

    // 2. 精确计算留给输入框的绝对矩形区域
    let edit_left = rect.min.x + PAD_X + ICON_SIZE + ICON_GAP;
    let edit_right = rect.max.x - PAD_X;
    let edit_rect = egui::Rect::from_min_max(
        egui::pos2(edit_left, rect.min.y),
        egui::pos2(edit_right, rect.max.y),
    );

    // 3. 【核心修复】建立一个完全隔离的子 UI 容器，并把它的 max_rect 锁死
    // 这样内部的 TextEdit 无论怎么折腾，都绝对无法越狱污染到外面的 ui
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(edit_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    
    let edit = egui::TextEdit::singleline(text)
        .hint_text(hint)
        .frame(egui::Frame::NONE)
        // 强制输入框至少占满我们给的宽度（但由于子UI限制，它也不会超过这个宽度）
        .desired_width(edit_rect.width());

    // 在隔离的子 UI 里直接添加输入框
    let resp = child_ui.add(edit);

    let enter_pressed = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

    (resp, enter_pressed)
}

/// 圆角 ComboBox 包装：圆角 8、自定义上下三角箭头。
/// `height` 控制控件高度。
pub fn rounded_combo<R>(
    ui: &mut egui::Ui,
    id_salt: &str,
    selected_text: impl Into<egui::WidgetText>,
    width: f32,
    height: f32,
    render_items: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let mut inner: Option<R> = None;
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
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

// ─── 通用圆角控件 ───────────────────────────────────────────────────

/// 通用行高常量（设置页等紧凑行内场景）。
pub const ROW_HEIGHT: f32 = 28.0;

/// 通用圆角输入框：圆角 8、手绘背景+边框、宽高由参数指定。
/// 返回 TextEdit 的 Response。
pub fn rounded_text_input(
    ui: &mut egui::Ui,
    text: &mut String,
    width: f32,
    height: f32,
    hint: Option<&str>,
) -> egui::Response {
    let visuals = ui.style().visuals.clone();
    const PAD_X: f32 = 8.0;

    let mut edit_resp: Option<egui::Response> = None;

    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            let rect = ui.max_rect();
            if ui.is_rect_visible(rect) {
                let painter = ui.painter();
                painter.rect_filled(rect, egui::CornerRadius::same(8), visuals.extreme_bg_color);
                painter.rect_stroke(
                    rect,
                    egui::CornerRadius::same(8),
                    egui::Stroke::new(1.0, visuals.text_color()),
                    egui::StrokeKind::Inside,
                );
            }

            ui.horizontal_centered(|ui| {
                ui.add_space(PAD_X);
                let edit_w = width - PAD_X * 2.0;
                let mut edit = egui::TextEdit::singleline(text)
                    .frame(egui::Frame::NONE)
                    .desired_width(edit_w)
                    .vertical_align(egui::Align::Center);
                if let Some(h) = hint {
                    edit = edit.hint_text(h);
                }
                edit_resp = Some(ui.add(edit));
            });
        },
    );

    edit_resp.expect("text edit always allocated")
}

/// 通用圆角 DragValue：圆角 8、宽高由参数指定。
/// 仅在原生 DragValue 基础上设置圆角和尺寸，不额外绘制边框/背景。
/// 返回 DragValue 的 Response。
pub fn rounded_drag_value<T: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    width: f32,
    height: f32,
) -> egui::Response {
    let mut style: egui::Style = (**ui.style()).clone();
    let r8 = egui::CornerRadius::same(8);
    style.visuals.widgets.inactive.corner_radius = r8;
    style.visuals.widgets.hovered.corner_radius = r8;
    style.visuals.widgets.active.corner_radius = r8;
    style.visuals.widgets.open.corner_radius = r8;

    ui.scope(|ui| {
        ui.set_style(style);
        let drag = egui::DragValue::new(value)
            .range(range)
            .custom_formatter(|n, _| {
                let v = n as i32;
                if v == -1 { "自动".to_string() } else { v.to_string() }
            })
            .custom_parser(|s| s.parse::<f64>().ok());
        ui.add_sized(egui::vec2(width, height), drag)
    })
    .inner
}
