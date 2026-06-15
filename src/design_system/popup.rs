//! 通用弹窗组件：自定义标题栏 + icon_button 关闭按钮 + 居中不可调大小。
//!
//! # 用法
//!
//! ```ignore
//! let open = popup::Popup::new("my_popup")
//!     .show(ctx, |ui| {
//!         // 弹窗内容
//!         ui.label("Hello");
//!     });
//! if !open {
//!     app.show_popup = false; // 清除打开状态
//! }
//! ```

use crate::design_system::button;
use crate::material_icons::icons as mi;

/// 标题栏固定高度。
const TITLE_BAR_H: f32 = 32.0;

/// 通用弹窗 builder。
pub struct Popup {
    id: String,
}

impl Popup {
    /// 创建弹窗，`id` 用作 egui Window 的唯一标识。
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }

    /// 显示弹窗，返回 `true` 表示仍然打开，`false` 表示用户关闭了。
    ///
    /// 关闭方式：点击右上角关闭按钮 / 按 ESC。
    /// 调用方在返回 `false` 时应清除自己的 open 状态。
    pub fn show(
        self,
        ctx: &egui::Context,
        content: impl FnOnce(&mut egui::Ui),
    ) -> bool {
        // ESC 关闭
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return false;
        }

        let mut should_close = false;
        egui::Window::new(self.id)
            .title_bar(false)
            .collapsible(false)
            .resizable([false, false])
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ctx, |ui| {
                // ---- 标题栏（固定高度，右侧关闭按钮） ----
                let (_, title_resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), TITLE_BAR_H),
                    egui::Sense::hover(),
                );
                let title_rect = title_resp.rect;
                let mut title_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(title_rect)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                if button::icon_button(&mut title_ui, mi::ICON_CLOSE, true)
                    .on_hover_text("关闭")
                    .clicked()
                {
                    should_close = true;
                }

                // ---- 内容区 ----
                content(ui);
            });

        !should_close
    }
}
