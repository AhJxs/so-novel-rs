//! 顶部窗口标题栏 — 仅放窗口控制按钮（最小化 / 最大化 / 关闭）+ 拖拽区。
//!
//! 设计：
//! - 单独的 `TopBottomPanel::top("title_bar")`，位于 nav 上方。
//! - 整个区域作为窗口拖拽源（按住可拖动窗口；双击切换最大化）。
//! - 右侧三个 28x28 的窗口控制按钮，关闭按钮 hover 红色。
//! - 标题栏背景与 nav 一致（panel_fill），无 stroke，融入整体。

use crate::design_system::frame;

/// 显示标题栏。`ctx` 用于 send_viewport_cmd（拖拽 / 关闭 / 最大化 / 最小化）。
pub fn show(parent_ui: &mut egui::Ui, ctx: &egui::Context) {
    let dark_mode = ctx.global_style().visuals.dark_mode;
    let is_maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

    egui::Panel::top("title_bar")
        .frame(frame::title_bar_frame(ctx.global_style().as_ref()))
        .show_inside(parent_ui, |ui| {
            // 1. 整个标题栏作为窗口拖拽区
            let drag_resp = ui.interact(
                ui.available_rect_before_wrap(),
                egui::Id::new("title_bar_drag"),
                egui::Sense::drag(),
            );
            if drag_resp.drag_started() || drag_resp.dragged() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if drag_resp.double_clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
            }

            // 2. 右侧排布：关闭 ✕ → 最大化/恢复 → 最小化 —
            // 每个按钮带中文 tooltip；hover 颜色按主题适配（浅色加深，深色变亮）。
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if window_control_button(ui, WindowControl::Close, dark_mode, false)
                    .on_hover_text("关闭")
                    .clicked()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                // 最大化/恢复：图标按当前状态变化（is_maximized=true 时画"双框恢复"）
                let max_tooltip = if is_maximized { "恢复" } else { "最大化" };
                if window_control_button(ui, WindowControl::Maximize, dark_mode, is_maximized)
                    .on_hover_text(max_tooltip)
                    .clicked()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_maximized));
                }
                if window_control_button(ui, WindowControl::Minimize, dark_mode, false)
                    .on_hover_text("最小化")
                    .clicked()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            });
        });
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum WindowControl {
    Minimize,
    Maximize,
    Close,
}

/// 自定义窗口控制按钮（替代原生标题栏）。
///
/// 风格：28x28 方形 + 圆角 4px；hover 变浅灰底；关闭按钮 hover 变红。
/// 图标：手画 svg 风格的线段（减号 / 方框 / 双方框 / 叉号），不用 unicode emoji 避免字体差异。
///
/// `is_maximized`：仅对 Maximize 按钮有意义；为 true 时画"双方框恢复"图标。
fn window_control_button(
    ui: &mut egui::Ui,
    ctrl: WindowControl,
    dark_mode: bool,
    is_maximized: bool,
) -> egui::Response {
    let size = egui::vec2(28.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if !ui.is_rect_visible(rect) {
        return response;
    }

    // hover 颜色：关闭按钮用红色突出"危险"语义；其它按主题适配
    //   - 深色模式：浅底（半透明白）；hover 在暗底上明显
    //   - 浅色模式：深底（半透明黑 alpha 28）；hover 在白底上明显
    let hover_bg = if response.hovered() {
        match ctrl {
            WindowControl::Close => Some(egui::Color32::from_rgb(232, 17, 35)),
            _ => Some(if dark_mode {
                egui::Color32::from_white_alpha(28)
            } else {
                egui::Color32::from_black_alpha(28)
            }),
        }
    } else {
        None
    };

    let painter = ui.painter();
    let rounding = egui::CornerRadius::same(4);

    if let Some(bg) = hover_bg {
        painter.rect_filled(rect, rounding, bg);
    }

    // icon color：hover 时强制白色（与红/灰底对比）；否则按主题取
    let icon_color = if response.hovered() {
        egui::Color32::WHITE
    } else if dark_mode {
        egui::Color32::from_white_alpha(200)
    } else {
        egui::Color32::from_black_alpha(180)
    };

    let center = rect.center();
    let half = 5.0;
    let stroke = egui::Stroke::new(1.5, icon_color);
    match ctrl {
        WindowControl::Minimize => {
            // 横线 —
            painter.line_segment(
                [
                    egui::pos2(center.x - half, center.y),
                    egui::pos2(center.x + half, center.y),
                ],
                stroke,
            );
        }
        WindowControl::Maximize => {
            if is_maximized {
                // "恢复"图标：两个错位的小方框（前层在前，后层错开 2px）
                let small = half - 1.0; // 小一点，给后层让位
                let front = egui::Rect::from_center_size(
                    center + egui::vec2(-1.0, 1.0),
                    egui::vec2(small * 2.0, small * 2.0),
                );
                let back = egui::Rect::from_center_size(
                    center + egui::vec2(1.0, -1.0),
                    egui::vec2(small * 2.0, small * 2.0),
                );
                painter.rect_stroke(back, egui::CornerRadius::same(1), stroke, egui::StrokeKind::Inside);
                painter.rect_stroke(front, egui::CornerRadius::same(1), stroke, egui::StrokeKind::Inside);
            } else {
                // 单方框 ▢
                let r = egui::Rect::from_center_size(center, egui::vec2(half * 2.0, half * 2.0));
                painter.rect_stroke(r, egui::CornerRadius::same(1), stroke, egui::StrokeKind::Inside);
            }
        }
        WindowControl::Close => {
            // 叉号 ✕
            painter.line_segment(
                [
                    egui::pos2(center.x - half, center.y - half),
                    egui::pos2(center.x + half, center.y + half),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x + half, center.y - half),
                    egui::pos2(center.x - half, center.y + half),
                ],
                stroke,
            );
        }
    }

    response
}

/// 在窗口四边/四角拦截光标，让用户可以从无装饰窗口边缘缩放窗口。
///
/// `with_decorations(false)` 关掉了原生边框 → 系统的 resize 命中区也没了，
/// 需要在 egui 的 root rect 外侧 6px 范围内自己 sense + 发 `BeginResize`。
///
/// 调用时机：在 `update()` 主循环里，所有 panel add 完之后调一次。
pub fn handle_window_resize(ctx: &egui::Context) {
    // 最大化时不允许缩放（与原生 Windows 行为一致）
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }

    use egui::{CursorIcon, ResizeDirection, ViewportCommand};

    /// 边缘命中区宽度（鼠标进入这一带就显示 resize 光标）。
    const EDGE: f32 = 6.0;

    let screen_rect = ctx.content_rect();
    let pos = match ctx.pointer_hover_pos() {
        Some(p) => p,
        None => return,
    };
    if !screen_rect.contains(pos) {
        return;
    }

    // 判断光标处于哪一边/哪一角
    let near_left = pos.x - screen_rect.left() < EDGE;
    let near_right = screen_rect.right() - pos.x < EDGE;
    let near_top = pos.y - screen_rect.top() < EDGE;
    let near_bot = screen_rect.bottom() - pos.y < EDGE;

    let (cursor, dir) = match (near_left, near_right, near_top, near_bot) {
        (true, _, true, _) => (CursorIcon::ResizeNwSe, Some(ResizeDirection::NorthWest)),
        (_, true, _, true) => (CursorIcon::ResizeNwSe, Some(ResizeDirection::SouthEast)),
        (_, true, true, _) => (CursorIcon::ResizeNeSw, Some(ResizeDirection::NorthEast)),
        (true, _, _, true) => (CursorIcon::ResizeNeSw, Some(ResizeDirection::SouthWest)),
        (true, _, _, _) => (CursorIcon::ResizeWest, Some(ResizeDirection::West)),
        (_, true, _, _) => (CursorIcon::ResizeEast, Some(ResizeDirection::East)),
        (_, _, true, _) => (CursorIcon::ResizeNorth, Some(ResizeDirection::North)),
        (_, _, _, true) => (CursorIcon::ResizeSouth, Some(ResizeDirection::South)),
        _ => return, // 不在边缘
    };

    ctx.set_cursor_icon(cursor);

    // 在边缘区域按下左键 → 触发 BeginResize
    if let Some(dir) = dir {
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}
