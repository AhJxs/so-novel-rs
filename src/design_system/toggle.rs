//! iOS 风格 toggle 开关。

use crate::design_system::color;

/// iOS 风格 toggle 开关（增强版）。
///
/// - 尺寸：40 × 22 px（比默认 interact_size 更大、更易点按）
/// - 动画：滑块位移 + 背景色渐变 + 滑块缩放弹性（先缩小再恢复）
/// - 颜色：on 时用 ACCENT 填充；off 时用暗灰轨道 + 白色滑块
/// - 滑块带 1px 浅灰阴影，营造浮起感
pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    // ---- 尺寸常量 ----
    const WIDTH: f32 = 40.0;
    const HEIGHT: f32 = 22.0;
    const KNOB_RADIUS: f32 = 8.0; // 滑块基础半径
    const KNOB_PAD: f32 = 3.0; // 滑块边缘与轨道内壁的间距

    let desired_size = egui::vec2(WIDTH, HEIGHT);
    let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, "")
    });

    if ui.is_rect_visible(rect) {
        let dark = ui.style().visuals.dark_mode;

        // 动画值：0 = off，1 = on（平滑过渡）
        let how_on = ui.ctx().animate_bool_responsive(response.id, *on);

        // ---- 滑块弹性缩放 ----
        // 拨动瞬间先缩小到 0.85，再弹回 1.0，模拟 iOS 的"按压回弹"
        let toggle_t = ui.ctx().animate_value_with_time(
            response.id.with("knob_scale"),
            if *on { 1.0 } else { 0.0 },
            0.15,
        );
        // 用 sin 曲线让缩放"过冲"：中间 0 → 两端 1
        let scale_raw = if toggle_t > 0.01 && toggle_t < 0.99 {
            1.0 - 0.15 * (toggle_t * std::f32::consts::PI).sin()
        } else {
            1.0
        };
        let knob_scale = scale_raw.max(0.8);

        // ---- 轨道颜色（渐变过渡） ----
        let on_fill = color::ACCENT;
        let off_fill = if dark {
            egui::Color32::from_rgb(55, 55, 58)
        } else {
            egui::Color32::from_rgb(200, 200, 202)
        };
        let track_fill = color_lerp(off_fill, on_fill, how_on);

        // ---- 绘制轨道 ----
        let track_radius = 0.5 * rect.height();
        let track_stroke = egui::Stroke::new(
            0.5,
            if dark {
                egui::Color32::from_white_alpha(20)
            } else {
                egui::Color32::from_black_alpha(15)
            },
        );
        ui.painter().rect(
            rect,
            track_radius,
            track_fill,
            track_stroke,
            egui::StrokeKind::Inside,
        );

        // ---- 绘制轨道内侧微光（on 状态时更亮） ----
        if how_on > 0.01 {
            let inner = rect.shrink(1.5);
            let inner_fill = egui::Color32::from_rgba_unmultiplied(
                on_fill.r(),
                on_fill.g(),
                on_fill.b(),
                (30.0 * how_on) as u8,
            );
            ui.painter().rect(
                inner,
                track_radius - 1.0,
                inner_fill,
                egui::Stroke::NONE,
                egui::StrokeKind::Inside,
            );
        }

        // ---- 滑块位置 ----
        // 中心 x 的行程：off 时贴左，on 时贴右，间距 KNOB_PAD
        let knob_center_left = rect.left() + KNOB_PAD + KNOB_RADIUS;
        let knob_center_right = rect.right() - KNOB_PAD - KNOB_RADIUS;
        let knob_x = egui::lerp(knob_center_left..=knob_center_right, how_on);
        let knob_center = egui::pos2(knob_x, rect.center().y);
        // 缩放只影响绘制半径，不影响中心位置
        let knob_draw_radius = KNOB_RADIUS * knob_scale;

        // ---- 滑块阴影 ----
        let shadow_offset = 1.0;
        let shadow_color = if dark {
            egui::Color32::from_black_alpha(50)
        } else {
            egui::Color32::from_black_alpha(30)
        };
        ui.painter().circle(
            egui::pos2(knob_center.x, knob_center.y + shadow_offset),
            knob_draw_radius,
            shadow_color,
            egui::Stroke::NONE,
        );

        // ---- 滑块主体 ----
        let knob_fill = if dark {
            egui::Color32::from_rgb(235, 235, 237)
        } else {
            egui::Color32::WHITE
        };
        let knob_stroke = egui::Stroke::new(
            0.5,
            if dark {
                egui::Color32::from_white_alpha(30)
            } else {
                egui::Color32::from_black_alpha(18)
            },
        );
        ui.painter()
            .circle(knob_center, knob_draw_radius, knob_fill, knob_stroke);
    }

    response
}

/// 线性插值两个 Color32 的 RGB 分量。
fn color_lerp(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
    )
}
