//! 顶部水平导航（仿 egui.rs demo 风格：单行按钮组 + 圆角 + 阴影）。
//!
//! 实现要点（避免上一版的两个 bug）：
//! - 用 `ui.allocate_exact_size(rect, Sense::click())` 自己分配 clickable 矩形，
//!   返回的 Response 才能被 `.clicked()` 触发。**不能**用 `Frame::show().response`，
//!   因为 Frame 内部的 `allocate_space` 只配 `Sense::hover()`，点击会丢失。
//! - 阴影手工用 painter 多层 rect_filled 模拟（offset 0,2 + 多级 alpha + expand），
//!   比 `Frame::shadow` 更可控，能保证渲染顺序在 fill 之前。
//!
//! 视觉：
//! - 选中态：亮蓝填充 + 白色加粗文字 + 圆角 8px + 下落阴影
//! - 未选中：透明背景 + 默认色文字 + 圆角 8px + 1px 浅边；hover 时浅蓝边 + 浅阴影
//! - 标题区：左侧 emoji + 强标题 + 副标题（弱色小字）

use crate::app::SoNovelApp;
use crate::ui::theme;
use material_icons::icons as mi;

/// 导航按钮统一圆角。
const NAV_BTN_ROUNDING: egui::CornerRadius = egui::CornerRadius::same(8);

/// 按钮文字 + 上下 padding 推算尺寸。
const NAV_BTN_PADDING_X: f32 = 14.0;
const NAV_BTN_PADDING_Y: f32 = 8.0;

/// 导航栏内容。在 `app.rs` 已建好的 `TopBottomPanel::top("nav")` 闭包里调用。
///
/// 窗口控制按钮（最小化/最大化/关闭）+ 拖拽 已挪到独立的 `title_bar` 模块，
/// 本模块只负责导航功能：标题区 + 6 个页面按钮 + toast。
pub fn show_in_panel(ui: &mut egui::Ui, ctx: &egui::Context, app: &mut SoNovelApp) {
    ui.horizontal(|ui| {
        ui.add_space(14.0);

        // ---- 左：标题区 ----
        ui.vertical(|ui| {
            // 固定高度：标题区文字跨主题不重新 layout，避免切换主题时 nav 高度跳动
            ui.set_min_height(36.0);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    mi::ICON_MENU_BOOK
                        .rich_text()
                        .size(18.0)
                        .color(theme::ACCENT),
                );
                ui.add_space(4.0);
                ui.strong(egui::RichText::new("So Novel").size(15.0));
            });
            ui.label(egui::RichText::new("Rust 桌面客户端").small().weak());
            ui.add_space(4.0);
        });

        ui.add_space(18.0);
        ui.separator();
        ui.add_space(14.0);

        // ---- 中：导航按钮组（点击切换 current_page）----
        let mut to_switch: Option<NavPage> = None;
        for page in NavPage::all() {
            if nav_button(ui, *page, app.current_page == *page).clicked() {
                to_switch = Some(*page);
            }
            ui.add_space(4.0);
        }
        if let Some(p) = to_switch {
            app.current_page = p;
        }

        // ---- 右：主题切换 + toast 推到尽头 ----
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(14.0);
            theme_segmented_control(ui, ctx);
            ui.add_space(8.0);
            if let Some((msg, t)) = &app.toast {
                toast_pill(ui, msg, t.elapsed().as_secs_f32());
            }
        });
    });
}

/// 导航栏右侧的 toast 提示：26px 圆角胶囊 + ACCENT 描边 + 浅 ACCENT 底色 + 左侧锚点小圆点。
///
/// 视觉：
/// - 圆角全填充（pill），与右侧 26px 主题按钮视觉重量一致
/// - 底色用 `theme::ACCENT` 低 alpha 染色；1px 描边略亮 — 在 light/dark 下都能从导航条"浮起来"
/// - 左侧 4px 实心小圆点 + 一段 8px 间距 + 文字 13pt bold accent
/// - 暗色模式加一层极弱的下投影；浅色模式靠描边和底色即可
///
/// 动画：
/// - 出现时 0.18s 淡入；消失前 0.4s 淡出（与 app.rs 中 4s 自动消失配合，最后一帧平滑收尾）
/// - 透明度统一乘到 bg / stroke / dot / text 四层，避免单独看某层突兀
///
/// 文本超宽时按字符砍 + `…`，避免压扁 nav 布局；上限 320px。
fn toast_pill(ui: &mut egui::Ui, msg: &str, elapsed: f32) {
    const PILL_H: f32 = 26.0;
    const PAD_X: f32 = 12.0;
    const DOT_SIZE: f32 = 5.0;
    const DOT_GAP: f32 = 8.0;
    const MAX_TEXT_W: f32 = 320.0;
    const FADE_IN: f32 = 0.18;
    const FADE_OUT_START: f32 = 3.6;
    const LIFETIME: f32 = 4.0;

    // 整体 alpha：淡入 0..0.18s，平台期 0.18..3.6s，淡出 3.6..4.0s
    let a_in = (elapsed / FADE_IN).clamp(0.0, 1.0);
    let a_out = ((LIFETIME - elapsed) / (LIFETIME - FADE_OUT_START)).clamp(0.0, 1.0);
    let alpha = a_in * a_out;
    if alpha <= 0.001 {
        // 完全透明：仍要占位以免 nav 高度跳变（toast 出现/消失不应让主题按钮位置抖）
        ui.allocate_exact_size(egui::vec2(1.0, PILL_H), egui::Sense::hover());
        return;
    }

    let dark_mode = ui.style().visuals.dark_mode;
    let font_id = egui::FontId::proportional(13.0);
    let accent = theme::ACCENT;

    // 测量原始文本；超宽就按字符砍 + …，逐次 layout 直到塞进 MAX_TEXT_W
    let mut text = msg.to_string();
    let initial_w = ui
        .painter()
        .layout_no_wrap(text.clone(), font_id.clone(), accent)
        .size()
        .x;
    if initial_w > MAX_TEXT_W {
        // 消息都很短，O(n) 砍到 1 字符也最多几十次 layout
        let mut trimmed: String = msg.chars().take(40).collect();
        loop {
            let w = ui
                .painter()
                .layout_no_wrap(format!("{trimmed}…"), font_id.clone(), accent)
                .size()
                .x;
            if w <= MAX_TEXT_W || trimmed.chars().count() <= 1 {
                text = format!("{trimmed}…");
                break;
            }
            trimmed.pop();
        }
    }

    // 终稿 layout：先拿到测量值供 pill 宽度计算（galley 持有，不依赖 ui）
    let measure_galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font_id.clone(), accent);
    let text_w = measure_galley.size().x;
    let pill_w = (text_w + PAD_X * 2.0 + DOT_SIZE + DOT_GAP).max(56.0);
    let pill_size = egui::vec2(pill_w, PILL_H);

    // 关键：必须 allocate_exact_size 拿到可绘制的 rect，且让父布局知道占位大小，
    // 否则 right_to_left 里它会变成 0 宽而不可见。
    let (rect, _) = ui.allocate_exact_size(pill_size, egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }

    // 拿到 painter 之后不再触碰 ui，避免与 allocate_exact_size 的可变借用冲突
    let painter = ui.painter();
    let rounding = egui::CornerRadius::same((PILL_H / 2.0).round() as u8);

    // 阴影（仅暗色）：下移 2px + expand 1.5px，单层低 alpha
    if dark_mode {
        let shadow_rect = rect.translate(egui::vec2(0.0, 2.0)).expand(1.5);
        painter.rect_filled(
            shadow_rect,
            egui::CornerRadius::same(((PILL_H / 2.0) + 1.5).round() as u8),
            egui::Color32::from_black_alpha((28.0 * alpha) as u8),
        );
    }

    // 底色：ACCENT 低 alpha；light 更克制，dark 更明显
    let bg = if dark_mode {
        egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), (38.0 * alpha) as u8)
    } else {
        egui::Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), (22.0 * alpha) as u8)
    };
    painter.rect_filled(rect, rounding, bg);

    // 描边：1px inside；暗色更亮一点，浅色更柔
    let stroke_alpha = if dark_mode { 180.0 } else { 150.0 };
    painter.rect_stroke(
        rect,
        rounding,
        egui::Stroke::new(
            1.0,
            egui::Color32::from_rgba_unmultiplied(
                accent.r(),
                accent.g(),
                accent.b(),
                (stroke_alpha * alpha) as u8,
            ),
        ),
        egui::StrokeKind::Inside,
    );

    // 左侧锚点小圆点：垂直居中
    let dot_center = egui::pos2(rect.left() + PAD_X + DOT_SIZE / 2.0, rect.center().y);
    painter.circle_filled(
        dot_center,
        DOT_SIZE / 2.0,
        egui::Color32::from_rgba_unmultiplied(
            accent.r(),
            accent.g(),
            accent.b(),
            (255.0 * alpha) as u8,
        ),
    );

    // 文字：圆点右侧，留 DOT_GAP 间距。重新 layout 一次拿到真正用于绘制的 galley
    let galley = painter.layout_no_wrap(text, font_id, accent);
    let text_x = rect.left() + PAD_X + DOT_SIZE + DOT_GAP;
    let mesh = galley.mesh_bounds;
    // 让文字 mesh 中心对齐到 (text_x + text_w/2, rect.center().y)
    let anchor = egui::pos2(
        text_x + text_w / 2.0 - mesh.center().x,
        rect.center().y - mesh.center().y,
    );
    let text_color = egui::Color32::from_rgba_unmultiplied(
        accent.r(),
        accent.g(),
        accent.b(),
        (255.0 * alpha) as u8,
    );
    painter.galley(anchor, galley, text_color);
}

/// 渲染单个导航按钮。
///
/// 用 `ui.allocate_exact_size` 拿到 clickable rect 后手工画 fill/stroke/shadow/text。
/// 这种写法对 Frame 不友好时是更可靠的方式 — 见模块顶部注释。
fn nav_button(ui: &mut egui::Ui, page: NavPage, selected: bool) -> egui::Response {
    let visuals = ui.style().visuals.clone();
    let dark_mode = visuals.dark_mode;

    let text = page.label();
    let text_color = if selected {
        egui::Color32::WHITE
    } else {
        visuals.text_color()
    };
    let font_id = egui::FontId::proportional(
        ui.style()
            .text_styles
            .get(&egui::TextStyle::Button)
            .map(|f| f.size)
            .unwrap_or(14.0),
    );

    // 1. measure：用 painter.layout_no_wrap 算文本宽高
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font_id.clone(), text_color);
    let desired_size = galley.size() + egui::vec2(NAV_BTN_PADDING_X * 2.0, NAV_BTN_PADDING_Y * 2.0);

    // 2. 分配 clickable rect（关键：Sense::click 才能被 .clicked() 触发）
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();

    // 3. 画阴影（仅选中态）：3 层 rect_filled 模拟 blur + offset
    if selected {
        // 深色模式阴影更深，浅色模式更柔
        let layers: [(f32, u8); 3] = if dark_mode {
            [(0.0, 70), (1.5, 40), (3.0, 18)]
        } else {
            [(0.0, 32), (1.5, 18), (3.0, 8)]
        };
        for (expand, alpha) in layers {
            let shadow_rect = rect
                .translate(egui::vec2(0.0, 3.0)) // 下移 3px
                .expand(expand);
            painter.rect_filled(
                shadow_rect,
                egui::CornerRadius::same((8.0 + expand).round() as u8),
                egui::Color32::from_black_alpha(alpha),
            );
        }
    }

    // 4. hover 反馈（仅未选中态）：浅底色 + 浅蓝边
    if !selected && response.hovered() {
        painter.rect_filled(
            rect,
            NAV_BTN_ROUNDING,
            if dark_mode {
                egui::Color32::from_white_alpha(18)
            } else {
                egui::Color32::from_black_alpha(10)
            },
        );
    }

    // 5. fill（选中态）
    if selected {
        painter.rect_filled(rect, NAV_BTN_ROUNDING, theme::ACCENT);
    }

    // 6. stroke
    let stroke = if selected {
        egui::Stroke::NONE
    } else if response.hovered() {
        egui::Stroke::new(1.0, theme::ACCENT)
    } else {
        egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color)
    };
    if stroke.width > 0.0 {
        painter.rect_stroke(rect, NAV_BTN_ROUNDING, stroke, egui::StrokeKind::Inside);
    }

    // 7. text — 用 galley.mesh_bounds（字符真实墨迹包围盒）居中。
    //    painter.text() 内部用 galley.size()，但 size().y 包含 line_gap/leading，
    //    导致 emoji + CJK 混排时视觉偏上。这里手动 layout + 按 mesh_bounds 居中。
    let galley = painter.layout_no_wrap(text.to_string(), font_id, text_color);
    // mesh_bounds 是真实字符像素范围（不含字体 leading）；以 rect.center 对齐到
    // mesh_bounds.center 的方式定位左上角。
    let mesh = galley.mesh_bounds;
    // galley 绘制时左上角是 anchor 点；mesh 在 galley 内的偏移是 mesh.min。
    // 我们希望 mesh.center() == rect.center()，所以 anchor = rect.center - mesh.center
    let anchor = rect.center() - mesh.center().to_vec2();
    painter.galley(anchor, galley, text_color);

    response
}

/// 主题切换段控（segmented control）：☀ Light / 💻 System / 🌙 Dark。
///
/// 三个 26x26 圆角按钮拼成一个胶囊；当前选中态用 theme::ACCENT 浅底色 + 蓝边。
/// 图标用 Material Symbols (`ICON_LIGHT_MODE` / `ICON_COMPUTER` / `ICON_DARK_MODE`)。
fn theme_segmented_control(ui: &mut egui::Ui, ctx: &egui::Context) {
    use egui::ThemePreference;
    let current = ctx.options(|o| o.theme_preference);

    // right_to_left 布局里 add 顺序与显示顺序一致（从右到左）：Dark → System → Light
    // 所以视觉上左 ☀、中 💻、右 🌙。
    if theme_icon_button(ui, ThemeIcon::Dark, current == ThemePreference::Dark)
        .on_hover_text("深色")
        .clicked()
    {
        ctx.set_theme(ThemePreference::Dark);
    }
    if theme_icon_button(ui, ThemeIcon::System, current == ThemePreference::System)
        .on_hover_text("跟随系统")
        .clicked()
    {
        ctx.set_theme(ThemePreference::System);
    }
    if theme_icon_button(ui, ThemeIcon::Light, current == ThemePreference::Light)
        .on_hover_text("浅色")
        .clicked()
    {
        ctx.set_theme(ThemePreference::Light);
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum ThemeIcon {
    Light,
    System,
    Dark,
}

fn theme_icon_button(ui: &mut egui::Ui, icon: ThemeIcon, selected: bool) -> egui::Response {
    let size = egui::vec2(26.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let visuals = ui.style().visuals.clone();
    let dark_mode = visuals.dark_mode;
    let painter = ui.painter();
    let rounding = egui::CornerRadius::same(6);

    // 选中态：theme::ACCENT 浅底 + 蓝边；hover：浅灰底；其它：透明
    if selected {
        painter.rect_filled(
            rect,
            rounding,
            egui::Color32::from_rgba_unmultiplied(58, 134, 230, if dark_mode { 50 } else { 35 }),
        );
        painter.rect_stroke(
            rect,
            rounding,
            egui::Stroke::new(1.0, theme::ACCENT),
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

    // 图标颜色：选中用 ACCENT，否则跟主题
    let icon_color = if selected {
        theme::ACCENT
    } else if dark_mode {
        egui::Color32::from_white_alpha(200)
    } else {
        egui::Color32::from_black_alpha(180)
    };

    // 之前用 painter 手画 ☀/💻/🌙（圆 + 8 射线 / 方框 + 底座 / 月牙弧线段），
    // 现在改用 Material Symbols：跨主题一致、不依赖任何 CJK/emoji 字形。
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

/// 主导航页签。
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum NavPage {
    Search,
    Tasks,
    Library,
    Sources,
    Settings,
    About,
}

impl NavPage {
    /// 导航按钮显示文字。返回 `String` 是因为里面塞了 material icon codepoint —
    /// codepoint 是 `&'static str`，拼到 `&'static str` 上做不到，必须在运行时
    /// 拼成 `String`。
    ///
    /// 渲染上：egui 拿到这个字符串后，字符级查找字体 — 前缀的 material codepoint
    /// 在 `material_icons` 注册的 `material-icons` 家族里命中（兜底被
    /// crate 设成 Lowest priority，跨族查找能 fallback 到），后半段中文落到
    /// Noto Sans SC。一行字符串两种字体共存，不需要手动分段。
    pub fn label(self) -> String {
        match self {
            NavPage::Search => format!("{} 搜索下载", mi::ICON_SEARCH.codepoint),
            NavPage::Tasks => format!("{} 下载任务", mi::ICON_DOWNLOAD.codepoint),
            NavPage::Library => format!("{} 本地书库", mi::ICON_LIBRARY_BOOKS.codepoint),
            NavPage::Sources => format!("{} 书源管理", mi::ICON_LANGUAGE.codepoint),
            NavPage::Settings => format!("{} 设置", mi::ICON_SETTINGS.codepoint),
            NavPage::About => format!("{} 关于", mi::ICON_INFO.codepoint),
        }
    }

    pub fn all() -> &'static [NavPage] {
        &[
            NavPage::Search,
            NavPage::Tasks,
            NavPage::Library,
            NavPage::Sources,
            NavPage::Settings,
            NavPage::About,
        ]
    }
}
