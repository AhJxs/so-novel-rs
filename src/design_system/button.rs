//! 按钮工厂：各样式按钮组件。
//!
//! ## 按钮体系总览
//!
//! | 类别 | 函数 | 高度 | 宽度 | 填充 | 圆角 | 用途 |
//! |------|------|------|------|------|------|------|
//! | 填充（主操作） | `primary_button` | 34 | 自适应 | ACCENT 蓝 | 8 | 搜索/确认等主操作 |
//! | 填充（危险操作） | `danger_button` | 34 | 自适应 | 语义红 | 8 | 清除记录等破坏性操作 |
//! | 填充（成功确认） | `success_button` | 34 | 自适应 | 语义绿 | 8 | 确认/完成等正向操作 |
//! | 填充（警告提示） | `warning_button` | 34 | 自适应 | 语义橙 | 8 | 注意/谨慎操作 |
//! | 幽灵（描边） | `ghost_button` | 34 | 自适应 | 透明+边框 | 8 | 次要/取消操作 |
//! | 文字（无框） | `text_button` | 34 | 自适应 | 透明 | 8 | 最弱层级的操作 |
//! | 行内（卡片） | `inline`/`inline_icon`/… | 28 | 自适应 | 主题色/语义色 | 8 | 卡片行内操作按钮 |
//! | 图标专用 | `icon_button` | 28 | 28 | 主题色 | 6 | 纯图标按钮 |
//! | 通用 | `button` | 默认 | 自适应 | 主题色 | 6 | 最基础的按钮 |
//! | 小型 | `small_button` | 默认 | 自适应 | 主题色 | 4 | 紧凑场景 |

use super::color::{self, ACCENT};
use crate::material_icons::MaterialIcon;

// ─── 基础按钮（egui::Button 封装）──────────────────────────────────

/// 通用按钮：字号 14pt、圆角 6。
pub fn button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(14.0))
            .corner_radius(egui::CornerRadius::same(6)),
    )
}

/// 小型按钮：字号 13pt、圆角 4、紧凑内边距。
pub fn small_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).size(13.0))
            .corner_radius(egui::CornerRadius::same(4))
            .small(),
    )
}

/// 填充/幽灵/文字按钮的统一行高。
pub const BAR_HEIGHT: f32 = 34.0;

// ─── 填充按钮（自定义绘制）──────────────────────────────────────────

/// 亮蓝填充主按钮。`enabled = false` 时变灰。返回 click 状态。
pub fn primary_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    filled_button(ui, text, enabled, ACCENT)
}

/// 危险色填充按钮（清除记录 / 删除等破坏性操作）。
pub fn danger_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let dark = ui.style().visuals.dark_mode;
    filled_button(ui, text, enabled, color::semantic_danger(dark))
}

/// 成功色填充按钮（确认 / 完成等正向操作）。
pub fn success_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let dark = ui.style().visuals.dark_mode;
    filled_button(ui, text, enabled, color::semantic_success(dark))
}

/// 警告色填充按钮（注意 / 谨慎操作）。
pub fn warning_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let dark = ui.style().visuals.dark_mode;
    filled_button(ui, text, enabled, color::semantic_warn(dark))
}

/// 实心填充按钮：圆角 8、高度 BAR_HEIGHT、宽度随内容、hover/pressed 自动派生色阶。
///
/// 返回 true 表示**这一帧被点击**。
fn filled_button(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    base_color: egui::Color32,
) -> bool {
    const CORNER_RADIUS: egui::CornerRadius = egui::CornerRadius::same(8);
    const PAD_X: f32 = 18.0;
    const HEIGHT: f32 = BAR_HEIGHT;

    let visuals = ui.style().visuals.clone();
    let font_id = egui::FontId::proportional(
        ui.style()
            .text_styles
            .get(&egui::TextStyle::Button)
            .map(|f| f.size)
            .unwrap_or(14.0),
    );

    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), egui::Color32::WHITE);
    let text_w = galley.size().x;
    let desired_size = egui::vec2(text_w + PAD_X * 2.0, HEIGHT);

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

    painter.rect_filled(rect, CORNER_RADIUS, fill);

    let galley = painter.layout_no_wrap(text.to_string(), font_id, text_color);
    let mesh = galley.mesh_bounds;
    let anchor = rect.center() - mesh.center().to_vec2();
    painter.galley(anchor, galley, text_color);

    response.clicked()
}

// ─── 幽灵按钮（描边，透明背景）────────────────────────────────────

/// 幽灵按钮：透明背景 + 主题边框，hover 时微弱填充。
/// 高度 BAR_HEIGHT、宽度随内容。返回 click 状态。
pub fn ghost_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    const CORNER_RADIUS: egui::CornerRadius = egui::CornerRadius::same(8);
    const PAD_X: f32 = 18.0;
    const HEIGHT: f32 = BAR_HEIGHT;

    let visuals = ui.style().visuals.clone();
    let font_id = egui::FontId::proportional(
        ui.style()
            .text_styles
            .get(&egui::TextStyle::Button)
            .map(|f| f.size)
            .unwrap_or(14.0),
    );

    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), visuals.text_color());
    let text_w = galley.size().x;
    let desired_size = egui::vec2(text_w + PAD_X * 2.0, HEIGHT);

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

    let stroke_color = if !enabled {
        visuals.weak_text_color()
    } else {
        visuals.text_color()
    };

    let fill = if !enabled {
        egui::Color32::TRANSPARENT
    } else if is_pressed {
        visuals.widgets.active.bg_fill
    } else if is_hovered {
        visuals.widgets.hovered.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };

    let text_color = if !enabled {
        visuals.weak_text_color()
    } else {
        visuals.text_color()
    };

    painter.rect_filled(rect, CORNER_RADIUS, fill);
    painter.rect_stroke(
        rect,
        CORNER_RADIUS,
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );

    let galley = painter.layout_no_wrap(text.to_string(), font_id, text_color);
    let mesh = galley.mesh_bounds;
    let anchor = rect.center() - mesh.center().to_vec2();
    painter.galley(anchor, galley, text_color);

    response.clicked()
}

// ─── 文字按钮（无背景无边框）────────────────────────────────────────

/// 文字按钮：无背景、无边框，hover 时微弱高亮。
/// 高度 BAR_HEIGHT、宽度随内容。返回 click 状态。
pub fn text_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    const PAD_X: f32 = 12.0;
    const HEIGHT: f32 = BAR_HEIGHT;

    let visuals = ui.style().visuals.clone();
    let font_id = egui::FontId::proportional(
        ui.style()
            .text_styles
            .get(&egui::TextStyle::Button)
            .map(|f| f.size)
            .unwrap_or(14.0),
    );

    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), visuals.text_color());
    let text_w = galley.size().x;
    let desired_size = egui::vec2(text_w + PAD_X * 2.0, HEIGHT);

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

    let text_color = if !enabled {
        visuals.weak_text_color()
    } else if is_pressed {
        visuals.widgets.active.fg_stroke.color
    } else if is_hovered {
        visuals.widgets.hovered.fg_stroke.color
    } else {
        visuals.text_color()
    };

    // hover/pressed 时加微弱高亮
    if enabled && (is_hovered || is_pressed) {
        let fill = if is_pressed {
            visuals.widgets.active.bg_fill
        } else {
            visuals.widgets.hovered.bg_fill
        };
        painter.rect_filled(rect, egui::CornerRadius::same(4), fill);
    }

    let galley = painter.layout_no_wrap(text.to_string(), font_id, text_color);
    let mesh = galley.mesh_bounds;
    let anchor = rect.center() - mesh.center().to_vec2();
    painter.galley(anchor, galley, text_color);

    response.clicked()
}

// ─── 图标按钮（纯图标，正方形）──────────────────────────────────────

/// 纯图标按钮：28×28 正方形、圆角 6、hover 时微弱高亮。
/// 返回 Response，可链式调用 `.on_hover_text()` 等方法。
pub fn icon_button(ui: &mut egui::Ui, icon: MaterialIcon, enabled: bool) -> egui::Response {
    const SIZE: f32 = 28.0;
    const CORNER_RADIUS: egui::CornerRadius = egui::CornerRadius::same(6);
    const ICON_SIZE: f32 = 18.0;

    let visuals = ui.style().visuals.clone();

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), sense);

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();
    let is_pressed = enabled && response.is_pointer_button_down_on();
    let is_hovered = enabled && response.hovered();

    let (fill, text_color) = if !enabled {
        (egui::Color32::TRANSPARENT, visuals.weak_text_color())
    } else if is_pressed {
        (visuals.widgets.active.bg_fill, visuals.widgets.active.fg_stroke.color)
    } else if is_hovered {
        (visuals.widgets.hovered.bg_fill, visuals.widgets.hovered.fg_stroke.color)
    } else {
        (egui::Color32::TRANSPARENT, visuals.text_color())
    };

    if fill != egui::Color32::TRANSPARENT {
        painter.rect_filled(rect, CORNER_RADIUS, fill);
    }

    let galley = painter.layout_no_wrap(
        icon.codepoint.to_string(),
        egui::FontId::proportional(ICON_SIZE),
        text_color,
    );
    let mesh = galley.mesh_bounds;
    let anchor = rect.center() - mesh.center().to_vec2();
    painter.galley(anchor, galley, text_color);

    response
}

// ─── InlineButton：卡片行内按钮 ────────────────────────────────────

/// [`InlineButton`] 的视觉变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineVariant {
    /// 默认：跟随 egui 主题色（与默认 Button 视觉一致）。
    Normal,
    /// 危险色填充（`semantic_danger`）+ 白色文字。
    Danger,
    /// 成功色填充（`semantic_success`）+ 白色文字。
    Success,
    /// 警告色填充（`semantic_warn`）+ 白色文字。
    Warning,
}

/// 卡片行内按钮 builder：圆角 8、高度 28、宽度自适应内容，支持可选图标前缀。
///
/// # 用法
///
/// ```ignore
/// // 普通文字按钮
/// if button::inline(ui, "下载") { ... }
///
/// // 带图标
/// if button::inline_icon(ui, "打开", mi::ICON_OPEN_IN_NEW) { ... }
///
/// // 危险色
/// if button::inline_danger(ui, "确认删除") { ... }
///
/// // 成功色
/// if button::inline_success(ui, "完成") { ... }
///
/// // 警告色
/// if button::inline_warning(ui, "注意") { ... }
///
/// // 完整 builder（禁用 + 图标）
/// button::InlineButton::new("取消中…")
///     .icon(mi::ICON_CANCEL)
///     .enabled(false)
///     .show(ui);
/// ```
pub struct InlineButton {
    text: String,
    icon: Option<MaterialIcon>,
    variant: InlineVariant,
    enabled: bool,
}

impl InlineButton {
    const CORNER_RADIUS: egui::CornerRadius = egui::CornerRadius::same(8);
    const FONT_SIZE: f32 = 14.0;
    const HEIGHT: f32 = 28.0;
    const PAD_X: f32 = 12.0;

    /// 创建一个新的行内按钮。
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            icon: None,
            variant: InlineVariant::Normal,
            enabled: true,
        }
    }

    /// 设置图标前缀（显示在文字左侧）。
    pub fn icon(mut self, icon: MaterialIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// 设置视觉变体（Normal / Danger / Success / Warning）。
    pub fn variant(mut self, v: InlineVariant) -> Self {
        self.variant = v;
        self
    }

    /// 是否启用。禁用时变灰且不可点击。
    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }

    /// 渲染按钮，返回是否被点击。
    pub fn show(self, ui: &mut egui::Ui) -> bool {
        let label = if let Some(icon) = self.icon {
            format!("{} {}", icon.codepoint, self.text)
        } else {
            self.text
        };

        let font_id = egui::FontId::proportional(Self::FONT_SIZE);
        let visuals = ui.style().visuals.clone();
        let dark_mode = visuals.dark_mode;

        // 测量文字宽度，计算自适应尺寸
        let galley = ui
            .painter()
            .layout_no_wrap(label.clone(), font_id.clone(), egui::Color32::WHITE);
        let text_w = galley.size().x;
        let desired_size = egui::vec2(text_w + Self::PAD_X * 2.0, Self::HEIGHT);

        let sense = if self.enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(desired_size, sense);

        if !ui.is_rect_visible(rect) {
            return false;
        }

        let painter = ui.painter();
        let is_pressed = self.enabled && response.is_pointer_button_down_on();
        let is_hovered = self.enabled && response.hovered();

        let (fill, text_color) = match self.variant {
            InlineVariant::Normal => {
                let wv = if !self.enabled {
                    &visuals.widgets.inactive
                } else if is_pressed {
                    &visuals.widgets.active
                } else if is_hovered {
                    &visuals.widgets.hovered
                } else {
                    &visuals.widgets.inactive
                };
                let tc = if !self.enabled {
                    visuals.weak_text_color()
                } else {
                    wv.fg_stroke.color
                };
                (wv.bg_fill, tc)
            }
            InlineVariant::Danger => {
                let base_color = color::semantic_danger(dark_mode);
                if !self.enabled {
                    (visuals.widgets.inactive.bg_fill, visuals.weak_text_color())
                } else if is_pressed {
                    (darken(base_color, 0.15), egui::Color32::WHITE)
                } else if is_hovered {
                    (lighten(base_color, 0.10), egui::Color32::WHITE)
                } else {
                    (base_color, egui::Color32::WHITE)
                }
            }
            InlineVariant::Success => {
                let base_color = color::semantic_success(dark_mode);
                if !self.enabled {
                    (visuals.widgets.inactive.bg_fill, visuals.weak_text_color())
                } else if is_pressed {
                    (darken(base_color, 0.15), egui::Color32::WHITE)
                } else if is_hovered {
                    (lighten(base_color, 0.10), egui::Color32::WHITE)
                } else {
                    (base_color, egui::Color32::WHITE)
                }
            }
            InlineVariant::Warning => {
                let base_color = color::semantic_warn(dark_mode);
                if !self.enabled {
                    (visuals.widgets.inactive.bg_fill, visuals.weak_text_color())
                } else if is_pressed {
                    (darken(base_color, 0.15), egui::Color32::WHITE)
                } else if is_hovered {
                    (lighten(base_color, 0.10), egui::Color32::WHITE)
                } else {
                    (base_color, egui::Color32::WHITE)
                }
            }
        };

        // 绘制背景
        painter.rect_filled(rect, Self::CORNER_RADIUS, fill);

        // Normal 变体：绘制边框（如有）
        if self.variant == InlineVariant::Normal {
            let wv = if !self.enabled {
                &visuals.widgets.inactive
            } else if is_pressed {
                &visuals.widgets.active
            } else if is_hovered {
                &visuals.widgets.hovered
            } else {
                &visuals.widgets.inactive
            };
            if wv.bg_stroke.width > 0.0 {
                painter.rect_stroke(
                    rect,
                    Self::CORNER_RADIUS,
                    wv.bg_stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }

        // 绘制文字（居中）
        let galley = painter.layout_no_wrap(label, font_id, text_color);
        let mesh = galley.mesh_bounds;
        let anchor = rect.center() - mesh.center().to_vec2();
        painter.galley(anchor, galley, text_color);

        response.clicked()
    }
}

// ─── InlineButton 便捷函数 ─────────────────────────────────────────

/// 普通行内按钮（无图标，Normal 变体）。返回是否被点击。
pub fn inline(ui: &mut egui::Ui, text: &str) -> bool {
    InlineButton::new(text).show(ui)
}

/// 带图标的行内按钮（Normal 变体）。返回是否被点击。
pub fn inline_icon(ui: &mut egui::Ui, text: &str, icon: MaterialIcon) -> bool {
    InlineButton::new(text).icon(icon).show(ui)
}

/// 危险色行内按钮（无图标）。返回是否被点击。
pub fn inline_danger(ui: &mut egui::Ui, text: &str) -> bool {
    InlineButton::new(text).variant(InlineVariant::Danger).show(ui)
}

/// 带图标的危险色行内按钮。返回是否被点击。
pub fn inline_danger_icon(ui: &mut egui::Ui, text: &str, icon: MaterialIcon) -> bool {
    InlineButton::new(text).icon(icon).variant(InlineVariant::Danger).show(ui)
}

/// 成功色行内按钮（无图标）。返回是否被点击。
pub fn inline_success(ui: &mut egui::Ui, text: &str) -> bool {
    InlineButton::new(text).variant(InlineVariant::Success).show(ui)
}

/// 带图标的成功色行内按钮。返回是否被点击。
pub fn inline_success_icon(ui: &mut egui::Ui, text: &str, icon: MaterialIcon) -> bool {
    InlineButton::new(text).icon(icon).variant(InlineVariant::Success).show(ui)
}

/// 警告色行内按钮（无图标）。返回是否被点击。
pub fn inline_warning(ui: &mut egui::Ui, text: &str) -> bool {
    InlineButton::new(text).variant(InlineVariant::Warning).show(ui)
}

/// 带图标的警告色行内按钮。返回是否被点击。
pub fn inline_warning_icon(ui: &mut egui::Ui, text: &str, icon: MaterialIcon) -> bool {
    InlineButton::new(text).icon(icon).variant(InlineVariant::Warning).show(ui)
}

// ─── Scope 辅助 ──────────────────────────────────────────────────────

/// 临时 scope 辅助：将 widgets 的 corner_radius 统一设为 8，
/// 供仍需使用原生 `egui::Button` 但希望圆角一致的场景。
pub fn with_rounded_corners<R>(
    ui: &mut egui::Ui,
    f: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let mut style: egui::Style = (**ui.style()).clone();
    let r8 = egui::CornerRadius::same(8);
    style.visuals.widgets.inactive.corner_radius = r8;
    style.visuals.widgets.hovered.corner_radius = r8;
    style.visuals.widgets.active.corner_radius = r8;
    style.visuals.widgets.open.corner_radius = r8;
    ui.scope(|ui| {
        ui.set_style(style);
        f(ui)
    })
    .inner
}

// ─── 内部工具 ───────────────────────────────────────────────────────

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
