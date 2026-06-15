//! 通用弹窗组件：自定义标题栏 + icon_button 关闭按钮 + 居中不可调大小。
//!
//! # 用法
//!
//! ```ignore
//! let open = popup::Popup::new("my_popup")
//!     .max_width(400.0)
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
    default_width: Option<f32>,
    default_height: Option<f32>,
    max_width: Option<f32>,
    max_height: Option<f32>,
}

impl Popup {
    /// 创建弹窗，`id` 用作 egui Window 的唯一标识。
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            default_width: None,
            default_height: None,
            max_width: None,
            max_height: None,
        }
    }

    /// 设置弹窗默认宽度（内容自适应时推荐设置，防止撑满屏幕）。
    pub fn default_width(mut self, w: f32) -> Self {
        self.default_width = Some(w);
        self
    }

    /// 设置弹窗默认高度。
    pub fn default_height(mut self, h: f32) -> Self {
        self.default_height = Some(h);
        self
    }

    /// 限制弹窗最大宽度。
    pub fn max_width(mut self, w: f32) -> Self {
        self.max_width = Some(w);
        self
    }

    /// 限制弹窗最大高度。
    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height = Some(h);
        self
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
        let mut win = egui::Window::new(self.id)
            .title_bar(false)
            .collapsible(false)
            .resizable([false, false])
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(8)),
            );

        if let Some(w) = self.default_width {
            win = win.default_width(w);
        }
        if let Some(h) = self.default_height {
            win = win.default_height(h);
        }
        if let Some(w) = self.max_width {
            win = win.max_width(w);
        }
        if let Some(h) = self.max_height {
            win = win.max_height(h);
        }

        win.show(ctx, |ui| {
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
