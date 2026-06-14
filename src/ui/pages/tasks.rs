//! 下载任务页。阶段 4b 打磨：
//! - 完成任务的"打开文件" / "打开所在目录"按钮；
//! - 失败 / 取消任务的"重新下载"按钮；
//! - 顶部"清除已完成"按钮。

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::app::{DownloadTask, SoNovelApp};
use crate::models::SearchResult;
use crate::ui::theme;

use crate::util::system::{open_path, reveal_in_folder};
use material_icons::icons as mi;

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    show_summary_bar(ui, app);
    ui.add_space(8.0);

    if app.tasks.is_empty() {
        show_empty_state(ui);
        return;
    }

    // 收集子动作（避免渲染循环里多重借 app）
    let mut to_open: Option<PathBuf> = None;
    let mut to_reveal: Option<PathBuf> = None;
    let mut to_redownload: Option<Box<SearchResult>> = None;

    egui::ScrollArea::vertical()
    .auto_shrink([false; 2])
    .show(ui, |ui| {
        // 卡片宽度循环外算一次，所有任务卡片共用 ——
        // 不在 show_one_task 内调 ui.available_width()：ScrollArea 跨帧缓存
        // content_size 会让逐张读到的值漂移，卡片就跟着变宽。
        
        let card_width = ui.available_width();
        let len = app.tasks.len();
        // 倒序：最新任务在最上面
        for i in (0..len).rev() {
            let action = show_one_task(ui, &mut app.tasks[i], card_width);
            match action {
                TaskAction::None => {}
                TaskAction::Open(p) => to_open = Some(p),
                TaskAction::Reveal(p) => to_reveal = Some(p),
                TaskAction::Redownload(r) => to_redownload = Some(r),
                TaskAction::SetCancelling => {
                    // task 已经在 show_one_task 内调过 cancel.cancel()；这里只翻
                    // UI 中间态标记，让按钮立刻显示"取消中…"。
                    app.tasks[i].cancelling = true;
                }
            }
            ui.add_space(8.0);
        }
    });

    if let Some(p) = to_open {
        if let Err(e) = open_path(&p) {
            app.show_toast(format!("打开失败: {e}"));
        }
    }
    if let Some(p) = to_reveal {
        if let Err(e) = reveal_in_folder(&p) {
            app.show_toast(format!("显示位置失败: {e}"));
        }
    }
    if let Some(r) = to_redownload {
        let _id = app.spawn_download(*r);
        app.show_toast("已重新加入下载");
    }
}

/// 任务列表为空时的空状态。
///
/// 与上方 summary bar 之间留 32px 间距（设计稿要求）；水平居中，
/// 内含大号 material 图标 + 主文案 + 副文案的视觉层级。
/// 不画卡片边框 / 背景 —— 让"空"的氛围更轻量，跟有任务时的 `Frame::group`
/// 卡片在视觉密度上拉开档次。
fn show_empty_state(ui: &mut egui::Ui) {
    // 与上方组件留 32px 间距（show() 已经在 summary bar 之后 add_space(8)，
    // 这里再补 24，加起来正好 32 — 不直接覆盖让调用方仍有"基础间距"语义）。
    ui.add_space(24.0);

    const ICON_SIZE: f32 = 48.0;
    let dark = ui.style().visuals.dark_mode;

    // 占满父宽度 + top_down(Align::Center) → 内容水平居中。
    // 没有 Frame，背景就走外层 panel 默认色；省一层视觉噪声。
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| {
            // 图标 — 用 muted 色不抢戏；ICON_INBOX = 空收件箱，最贴"无任务"语义
            ui.label(
                mi::ICON_INBOX
                    .rich_text()
                    .size(ICON_SIZE)
                    .color(theme::semantic_muted(dark)),
            );
            ui.add_space(10.0);

            // 主文案 — 强字号
            ui.label(
                egui::RichText::new("暂无下载任务")
                    .size(16.0)
                    .strong(),
            );
            ui.add_space(6.0);

            // 副文案 — 引导用户去搜索页。比之前的 .small() 大一档：直接 14pt
            // 不弱化，让指引可读性优先于"次要文案"的视觉降级。
            ui.label(
                egui::RichText::new("去『搜索下载』选一本书试试")
                    .size(14.0)
                    .weak(),
            );
        },
    );
}

/// 顶部摘要条：左侧统计 chip 组 + 右侧"清除记录"主按钮（红色）。
///
/// 设计：
/// - chip 样式参考搜索页的 source-status pill（ACCENT/semantic 浅底 + 描边 + 图标 + 数字），
///   但区分点是没有 host/id 后缀，体积更小（24px 高），各状态用各自的语义色。
/// - "0 条"的 chip 直接不画，避免顶栏永远撑满 — 只有相关时才出现。
/// - "失败" 与 "已取消" 分开统计：失败用 `semantic_warn`（橙）+ WARNING 图标，
///   取消用 `semantic_muted`(灰）+ CANCEL 图标 — 后者属于用户主动操作，
///   不该用警告色干扰视线。
/// - 右侧按钮形状参考搜索页的 `nav_style_button`（圆角填充 + 阴影 + 按下下沉 1px），
///   但底色用 `semantic_danger`，强调"破坏性操作"。disabled 时变灰、无阴影。
fn show_summary_bar(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    let total = app.tasks.len();
    let running = app.tasks.iter().filter(|t| t.is_running()).count();
    let done = app
        .tasks
        .iter()
        .filter(|t| matches!(&t.finished, Some(Ok(_))))
        .count();
    let cancelled = app.tasks.iter().filter(|t| t.is_cancelled()).count();
    let failed = app.tasks.iter().filter(|t| t.is_failed()).count();

    let dark = ui.style().visuals.dark_mode;
    ui.horizontal(|ui| {
        ui.set_min_height(SUMMARY_BAR_HEIGHT);

        // 左侧：从 5 个统计里挑非 0 的画 chip（"总数"始终画）
        stat_chip(ui, mi::ICON_INVENTORY, "总数", total, theme::semantic_muted(dark));
        if running > 0 {
            ui.add_space(6.0);
            stat_chip(
                ui,
                mi::ICON_DOWNLOADING,
                "进行中",
                running,
                theme::semantic_info(dark),
            );
        }
        if done > 0 {
            ui.add_space(6.0);
            stat_chip(
                ui,
                mi::ICON_CHECK_CIRCLE,
                "完成",
                done,
                theme::semantic_success(dark),
            );
        }
        if failed > 0 {
            ui.add_space(6.0);
            stat_chip(
                ui,
                mi::ICON_WARNING,
                "失败",
                failed,
                theme::semantic_warn(dark),
            );
        }
        if cancelled > 0 {
            ui.add_space(6.0);
            // 取消属用户主动操作，灰色 muted 不喧宾夺主
            stat_chip(
                ui,
                mi::ICON_CANCEL,
                "已取消",
                cancelled,
                theme::semantic_muted(dark),
            );
        }

        // 右侧：危险色"清除记录"主按钮，推到行末
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let any_finished = done + failed + cancelled > 0;
            let label = format!("{} 清除记录", mi::ICON_DELETE.codepoint);
            if danger_solid_button(ui, &label, any_finished) {
                app.clear_finished_tasks();
            }
        });
    });
}

/// summary bar 与 nav_style_button 共用的统一行高。
const SUMMARY_BAR_HEIGHT: f32 = 34.0;
/// chip 高度 — 与 search 页 source-status chip 保持一致（视觉重量统一）。
const CHIP_HEIGHT: f32 = 24.0;

/// 单个统计 chip：左侧 icon（彩色）+ 标签 + 加粗数字。
///
/// 视觉与搜索页的 source-status chip 同源（圆角 12 + 状态色低 alpha 底 + 描边），
/// 区别是：
/// - 没有圆形 dot；改用 material icon 直接表达状态语义
/// - 数字加粗与标签拉开层级
fn stat_chip(ui: &mut egui::Ui, icon: material_icons::MaterialIcon, label: &str, count: usize, color: egui::Color32) {
    const ICON_SIZE: f32 = 14.0;
    const PAD_X: f32 = 10.0;
    const GAP_AFTER_ICON: f32 = 6.0;
    const GAP_BEFORE_COUNT: f32 = 6.0;
    const ROUNDING: u8 = 12;

    let dark = ui.style().visuals.dark_mode;
    let visuals = ui.style().visuals.clone();

    // 测量 — label 用 Body 字号，count 稍大 1px 拉层级。
    // 这一轮 layout 拿到的 galley 既用来算 chip 总宽，也直接用来绘制 ——
    // 与搜索页 `chip()` 同源：用 `mesh_bounds.center()`（实际墨迹几何中心）
    // 做垂直对齐，比 `Align2::*_CENTER`（按字体 row 高度中心，含 leading）
    // 更准。CJK + emoji + material icon + 多种字号混排时，墨迹中心法不会
    // 因为每段 leading 不同而上下偏移。
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

    let icon_galley = ui.painter().layout_no_wrap(
        icon.codepoint.to_string(),
        icon_font,
        color,
    );
    let label_galley = ui.painter().layout_no_wrap(
        label.to_string(),
        body_font,
        visuals.text_color(),
    );
    let count_galley = ui.painter().layout_no_wrap(
        count_text,
        count_font,
        color,
    );

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

    // 背景：状态色 ~10-12% alpha；暗色模式下 alpha 略高一点视觉才亮得起来
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

    // 内容：从左到右画 icon → label → count。
    // 三段都走 `painter.galley(anchor, galley, color)` —— anchor 是 galley 左上角，
    // 通过 `rect.center().y - galley.mesh_bounds.center().y` 让墨迹中心钉到 chip
    // 中心线（参考 `search.rs::chip`）。
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

/// "危险色" 实心按钮：与搜索页 `nav_style_button` 形态一致（圆角填充 + 阴影 +
/// 按下下沉 1px），但底色用 `theme::semantic_danger`。disabled 时变灰、无阴影。
///
/// 返回 true 表示**这一帧被点击**。
fn danger_solid_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let dark = ui.style().visuals.dark_mode;
    solid_button(ui, text, enabled, theme::semantic_danger(dark), SUMMARY_BAR_HEIGHT)
}

/// 通用实心按钮工厂：圆角 8 + 阴影 + 按下下沉 1px + hover/pressed 色阶。
///
/// 用 `base_color` 决定主色（搜索页主按钮用 `theme::ACCENT`，清除记录用
/// `semantic_danger`，任务卡片"取消"用 danger，"打开"/"重试"用 ACCENT）。
/// `height` 让卡片内的按钮可以做得比 summary bar 的更紧凑（30 vs 34）。
fn solid_button(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    base_color: egui::Color32,
    height: f32,
) -> bool {
    const BTN_ROUNDING: egui::CornerRadius = egui::CornerRadius::same(8);
    const BTN_PADDING_X: f32 = 14.0;

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

    // 颜色：按下/hover 在原色上叠暗 / 亮的覆盖，靠 alpha 调出层次
    let (fill, text_color) = if !enabled {
        (visuals.widgets.inactive.bg_fill, visuals.weak_text_color())
    } else if is_pressed {
        (darken(base_color, 0.15), egui::Color32::WHITE)
    } else if is_hovered {
        (lighten(base_color, 0.10), egui::Color32::WHITE)
    } else {
        (base_color, egui::Color32::WHITE)
    };

    // 按下下沉 1px
    let press_offset = if is_pressed {
        egui::vec2(0.0, 1.0)
    } else {
        egui::vec2(0.0, 0.0)
    };
    let rect = rect.translate(press_offset);

    // 阴影（仅 enabled）— 形态与 nav_style_button 一致
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

    // 文字 — mesh_bounds 居中
    let galley = painter.layout_no_wrap(text.to_string(), font_id, text_color);
    let mesh = galley.mesh_bounds;
    let anchor = rect.center() - mesh.center().to_vec2();
    painter.galley(anchor, galley, text_color);

    response.clicked()
}

/// 把 `c` 向黑色混合 `t`（0..=1）。仅用于按钮 pressed/hover 的色调微调。
fn darken(c: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = 1.0 - t;
    egui::Color32::from_rgb(
        (c.r() as f32 * f) as u8,
        (c.g() as f32 * f) as u8,
        (c.b() as f32 * f) as u8,
    )
}

/// 把 `c` 向白色混合 `t`。
fn lighten(c: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from_rgb(
        (c.r() as f32 + (255.0 - c.r() as f32) * t) as u8,
        (c.g() as f32 + (255.0 - c.g() as f32) * t) as u8,
        (c.b() as f32 + (255.0 - c.b() as f32) * t) as u8,
    )
}

enum TaskAction {
    None,
    Open(PathBuf),
    Reveal(PathBuf),
    /// SearchResult 较大（含若干 Option<String>）；Box 起来避免大 enum 警告。
    Redownload(Box<SearchResult>),
    /// 用户点了"取消"按钮：上层把 task.cancelling 翻成 true。
    /// 子函数内只有 `&task` 借用，无法直接改 task；走 action 让外层有 `&mut` 时改。
    SetCancelling,
}

/// 渲染一条下载任务卡片。
///
/// **与搜索页 `result_card` 同款视觉**：
/// - 圆角 8 + `inner_margin(symmetric(14, 10))` —— 跟搜索结果卡片像素级一致
/// - `allocate_ui_with_layout(vec2(card_width, 0))` 硬钉宽，杜绝内容撑大 frame
/// - 单行 horizontal：左侧 `#id + 书名 + 作者 · 来源`，右侧 right_to_left 按钮组
/// - 状态信息（进度条 / 耗时 / 输出路径）作为 Frame 内第二行 vertical 排
///
/// `card_width` 由调用方在 ScrollArea 闭包外**一次性**算好传入；不要在本函数
/// 内调 `ui.available_width()`。
fn show_one_task(
    ui: &mut egui::Ui,
    task: &mut DownloadTask,
    card_width: f32,
) -> TaskAction {
    let mut action = TaskAction::None;
    let visuals = ui.style().visuals.clone();
    let dark = visuals.dark_mode;

    // ---- 状态色（与 stat_chip 一致；完成/失败/取消各 1 色） ----
    let status_color = if task.is_running() {
        theme::ACCENT
    } else if task.is_cancelled() {
        theme::semantic_muted(dark)
    } else if task.is_failed() {
        theme::semantic_warn(dark)
    } else {
        theme::semantic_success(dark)
    };
    let hover_fill = if dark {
        egui::Color32::from_white_alpha(10)
    } else {
        egui::Color32::from_black_alpha(8)
    };

    // hover 反馈：Frame::fill 在 show 时定型，借 memory 记上一帧 hover；
    // 跟 result_card 同样的 1 帧延迟技巧。
    let card_id = egui::Id::new(("task_card", task.id));
    let was_hovered = ui
        .ctx()
        .memory(|m| m.data.get_temp::<bool>(card_id).unwrap_or(false));
    let card_hovered = was_hovered && !task.is_running();
    let frame_fill = if card_hovered {
        hover_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    // 边框：1px 状态色（下载中/完成/失败/取消 各色），让卡片**描边本身就是
    // 状态指示** —— 跟 #id 色、进度条色一起构成"状态视觉系统"。
    // 弱化"hover 才描边"的设计：状态色描边稳定可见，hover 只改 fill。
    let frame_stroke = egui::Stroke::new(1.0, status_color);

    let card_inner_width = (card_width - 28.0).max(0.0); // inner_margin symmetric(14,10) → 左右各 14

    // 硬分配固定宽度的盒子（与 result_card 同套路）：
    // - 外 allocate_ui_with_layout(vec2(card_width, 0)) 锁外宽
    // - 内 set_max_width = set_min_width = card_inner_width 锁内宽
    // 这样 Frame 的 min_rect 不会超过 card_width，杜绝 ScrollArea content_size 雪球。
    let frame_resp = ui
        .allocate_ui_with_layout(
            egui::vec2(card_width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_max_width(card_width);
                ui.set_min_width(card_width);

                egui::Frame::new()
                    .fill(frame_fill)
                    .stroke(frame_stroke)
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(card_inner_width);
                        ui.set_min_width(card_inner_width);

                        // ---- 第一行：左（id+书名+作者·来源） / 右（章节计数+最近章节） ----
                        ui.horizontal(|ui| {
                            ui.set_min_height(32.0);

                            // 跟 result_card 一致：子 scope 改按钮 corner_radius = 8
                            let mut style: egui::Style = (**ui.style()).clone();
                            let r8 = egui::CornerRadius::same(8);
                            style.visuals.widgets.inactive.corner_radius = r8;
                            style.visuals.widgets.hovered.corner_radius = r8;
                            style.visuals.widgets.active.corner_radius = r8;
                            ui.set_style(style);

                            // #id —— 字号与作者一致（Body 13pt），颜色 = 状态色，strong
                            // 让 #1 / #2 这种序号在卡片左上角有视觉重量，跟边框+进度条
                            // 三位一体共同表达下载状态
                            ui.label(
                                egui::RichText::new(format!("#{}", task.id))
                                    .strong()
                                    .size(13.0)
                                    .color(status_color),
                            );
                            ui.add_space(8.0);

                            // 书名（强 14.5pt 截断 28）
                            ui.label(
                                egui::RichText::new(truncate(task.book_name(), 28))
                                    .strong()
                                    .size(14.5),
                            );
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("·").weak());
                            ui.add_space(10.0);

                            // 作者
                            let author = task
                                .book_meta
                                .as_ref()
                                .map(|b| b.author.as_str())
                                .or(task.origin.author.as_deref())
                                .unwrap_or("未知");
                            ui.label(truncate(author, 14));
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("·").weak());
                            ui.add_space(10.0);

                            // 来源
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}#{}",
                                    task.origin.source_name, task.origin.source_id
                                ))
                                .weak(),
                            );

                            // 右上角：章节计数（上行）+ 最近章节（下行），上下两行
                            // 排成紧凑信息块。外层 right_to_left 把整块推到卡片最右，
                            // 内层 top_down(Align::Max) 让两块右对齐自然成列。
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.with_layout(
                                        egui::Layout::top_down(egui::Align::Max),
                                        |ui| {
                                            render_task_top_right(ui, task, status_color);
                                        },
                                    );
                                },
                            );
                        });

                        // ---- 第二行：全宽进度条 ----
                        ui.add_space(8.0);
                        show_progress_bar(ui, progress_fraction(task), status_color);

                        // ---- 第三行：左（开始时间+耗时）/ 右（按钮组 — 右下角） ----
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            // 左下：开始时间 + 耗时
                            show_task_time_line(ui, task);
                            // 右下：操作按钮（与 result_card 的"详情 + 下载"同款）
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    action = render_task_action_buttons(ui, task);
                                },
                            );
                        });

                        // ---- 第四行：输出路径 / 失败章节折叠（可选） ----
                        show_task_extra_meta(ui, task);
                    })
                    .response
            },
        )
        .inner;

    // hover 状态写回 memory，供下一帧渲染 fill
    ui.ctx().memory_mut(|m| {
        m.data.insert_temp(card_id, frame_resp.hovered());
    });

    action
}

/// 任务卡片右侧按钮组（按状态分）：
/// - Running：单 `[✕ 取消]`（红）
/// - Running + cancelling：disabled `[✕ 取消中…]`
/// - Completed：主 `[↗ 打开]`（蓝）+ 次 `[📁 位置]`
/// - Failed / Cancelled：单 `[↻ 重试]`（蓝）
///
/// 跟 result_card 的"详情 + 下载"用同款 solid Button —— 圆角 8、min_size 56×28、
/// 不用 `ui.scope` 改样式（避免 right_to_left 里的 cursor 重复扣减问题）。
fn render_task_action_buttons(
    ui: &mut egui::Ui,
    task: &mut DownloadTask,
) -> TaskAction {
    let mut action = TaskAction::None;
    match &task.finished {
        Some(Ok(p)) => {
            // 完成：打开（蓝）+ 位置
            if ui
                .add(
                    egui::Button::new(format!("{} 打开", mi::ICON_OPEN_IN_NEW.codepoint))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(56.0, 28.0)),
                )
                .clicked()
            {
                action = TaskAction::Open(p.clone());
            }
            ui.add_space(6.0);
            if ui.add(
                    egui::Button::new(format!("{} 位置", mi::ICON_FOLDER_OPEN.codepoint))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(56.0, 28.0)),
                )
                .clicked()
                {
                action = TaskAction::Reveal(p.clone());
            }
        }
        Some(Err(_)) => {
            // 失败 / 取消：单 重试
            if ui
                .add(
                    egui::Button::new(format!("{} 重试", mi::ICON_REFRESH.codepoint))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(56.0, 28.0)),
                )
                .clicked()
            {
                action = TaskAction::Redownload(Box::new(task.origin.clone()));
            }
        }
        None => {
            // 运行中：单 取消（红）
            if task.cancelling {
                ui.add_enabled(
                    false,
                    egui::Button::new(format!("{} 取消中…", mi::ICON_CANCEL.codepoint))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(56.0, 28.0)),
                );
            } else if ui
                .add(
                    egui::Button::new(format!("{} 取消", mi::ICON_CANCEL.codepoint))
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(56.0, 28.0)),
                )
                .clicked()
            {
                if let Some(cancel) = task.cancel.as_ref() {
                    cancel.cancel();
                }
                // 立刻翻成"取消中"中间态 —— UI 即时反馈。drain 收到 Progress::Cancelled
                // 时把 cancelling 清回 false 并落到 finished。
                return TaskAction::SetCancelling;
            }
        }
    }
    action
}

/// 任务进度比例：[0, 1] clamp 后供 progress bar 用。`total=0` 给 0，
/// 避免除零；这条只在解析详情前（total 还没填）出现，几帧后就有真值。
fn progress_fraction(task: &DownloadTask) -> f32 {
    let total = task.total_chapters;
    if total == 0 {
        0.0
    } else {
        (task.completed as f32 / total as f32).clamp(0.0, 1.0)
    }
}

/// 卡片右上角：章节计数（xx/xx）+ 最近章节，上下两行排。
///
/// 上行 = 章节计数（状态色，跟 #id、边框、进度条三位一体表达下载状态）
/// 下行 = 最近章节名（弱化灰色 —— 章节名是辅助信息，不抢视觉焦点）
///
/// 上下两行比横排更"卡片角"的视觉：横排会把两行塞在同一个高度，
/// 上 / 下分行让右上角自然形成一个紧凑信息块，呼应在右上方的视觉重心。
fn render_task_top_right(ui: &mut egui::Ui, task: &DownloadTask, status_color: egui::Color32) {
    let total = task.total_chapters;
    // 章节计数：运行中且没解析完 → "解析详情中…"；否则 xx/xx [+ 失败 n]
    let count_text = if total == 0 && task.is_running() {
        "解析详情中…".to_string()
    } else if task.failed > 0 {
        format!("{} / {} 章 · 失败 {}", task.completed, total, task.failed)
    } else {
        format!("{} / {} 章", task.completed, total)
    };
    ui.label(egui::RichText::new(count_text).small().color(status_color));
    if !task.last_chapter_title.is_empty() {
        ui.label(
            egui::RichText::new(format!("最近：{}", truncate(&task.last_chapter_title, 36)))
                .small()
                .weak(),
        );
    }
}

/// 卡片左下角：开始时间（带年月日）+ 耗时。
///
/// 运行中耗时用 info 蓝（"已运行 X"，视觉上提醒在动）；
/// 已完成用弱化灰（"耗时 X"，纯历史信息）。
fn show_task_time_line(ui: &mut egui::Ui, task: &DownloadTask) {
    let dark = ui.style().visuals.dark_mode;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}", format_unix(task.started_at_unix))),
        );
        if let Some(d) = task.elapsed() {
            ui.label(egui::RichText::new("·").small().weak());
            let (label, color) = if task.is_running() {
                (
                    format!("已运行 {}", format_duration(d)),
                    theme::semantic_info(dark),
                )
            } else {
                (
                    format!("耗时 {}", format_duration(d)),
                    ui.style().visuals.weak_text_color(),
                )
            };
            ui.colored_label(color, egui::RichText::new(label));
        }
    });
}

/// 卡片底部可选行：完成时显示输出路径 / 失败时显示原因 / 失败章节明细折叠。
/// 整段用 `ui.small()` 体量，避免跟上面的主信息抢视觉。
fn show_task_extra_meta(ui: &mut egui::Ui, task: &DownloadTask) {
    let dark = ui.style().visuals.dark_mode;
    match &task.finished {
        Some(Ok(p)) => {
            ui.label(
                egui::RichText::new(format!("输出 {}", p.display())),
            );
        }
        Some(Err(reason)) if !task.is_cancelled() => {
            ui.label(
                egui::RichText::new(format!("原因 {reason}"))
                    .small()
                    .color(theme::semantic_warn(dark)),
            );
        }
        _ => {}
    }

    if !task.failures.is_empty() {
        ui.add_space(2.0);
        let header = format!("失败章节（{}）", task.failures.len());
        egui::CollapsingHeader::new(egui::RichText::new(header).small())
            .id_salt(("task_failures", task.id))
            .show(ui, |ui| {
                for (idx, title, reason) in &task.failures {
                    ui.label(
                        egui::RichText::new(format!("第 {idx} 章 《{title}》 — {reason}"))
                            .small(),
                    );
                }
            });
    }
}


/// 自定义进度条：背景灰圆角条 + 状态色填充。
/// 高 8px，圆角 4px，跟卡片整体的"扁平+圆角"风格一致。
fn show_progress_bar(ui: &mut egui::Ui, progress: f32, color: egui::Color32) {
    const BAR_HEIGHT: f32 = 8.0;
    let avail_w = ui.available_width();
    let (rect, _resp) =
        ui.allocate_exact_size(egui::vec2(avail_w, BAR_HEIGHT), egui::Sense::hover());

    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let dark = ui.style().visuals.dark_mode;
    let track_color = if dark {
        egui::Color32::from_white_alpha(28)
    } else {
        egui::Color32::from_black_alpha(20)
    };
    let r = egui::CornerRadius::same(4);
    painter.rect_filled(rect, r, track_color);

    let p = progress.clamp(0.0, 1.0);
    if p > 0.0 {
        let fill_w = rect.width() * p;
        let fill_rect = egui::Rect::from_min_size(rect.min, egui::vec2(fill_w, rect.height()));
        painter.rect_filled(fill_rect, r, color);
    }
}

/// 字符级截断（与搜索页 truncate 同实现，避免跨模块依赖）。
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('…');
    out
}

/// 把 unix 时间戳格式化为本地时间字符串。
///
/// 统一格式 `YYYY-MM-DD HH:MM`，让任务卡片的"开始时间"在跨日 / 跨年场景下
/// 都不会有歧义。之前按"今天 / 昨天 / MM-DD"分支显示的写法在用户翻历史
/// 任务时容易混淆（"昨天 15:30" 到底是今天的昨天还是某历史任务的昨天）。
fn format_unix(unix_secs: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    if unix_secs <= 0 {
        return "未知".to_string();
    }
    let dt = UNIX_EPOCH + Duration::from_secs(unix_secs as u64);
    let date = chrono_like_local_date(dt);
    let hhmm = chrono_like_hhmm(dt);
    format!(
        "{}-{:02}-{:02} {hhmm}",
        chrono_year(date),
        chrono_month(date),
        chrono_day(date)
    )
}

/// `Duration` 格式化为人类可读的"X 分 Y 秒"风格。
///
/// < 1 分钟  → "30 秒"
/// < 1 小时  → "5 分 30 秒"
/// < 1 天    → "2 时 15 分"
/// ≥ 1 天    → "3 天 4 时"
fn format_duration(d: Duration) -> String {
    let total = d.as_secs();
    if total < 60 {
        return format!("{total} 秒");
    }
    if total < 3600 {
        let m = total / 60;
        let s = total % 60;
        return if s == 0 {
            format!("{m} 分")
        } else {
            format!("{m} 分 {s} 秒")
        };
    }
    if total < 86_400 {
        let h = total / 3600;
        let m = (total % 3600) / 60;
        return if m == 0 {
            format!("{h} 时")
        } else {
            format!("{h} 时 {m} 分")
        };
    }
    let days = total / 86_400;
    let h = (total % 86_400) / 3600;
    if h == 0 {
        format!("{days} 天")
    } else {
        format!("{days} 天 {h} 时")
    }
}

// ---- 下面是 std 替代的"calendar 工具"，避免引 chrono 这个大依赖 ----
//
// 用 SystemTime + 算法自己算本地日期/时间。系统时区用 `chrono` 风格的
// `Local time since midnight / days since 1970-01-01` 推导，精度只到天，
// 但对我们这种"今天 / 昨天 / YYYY-MM-DD"的展示需求完全够。

/// 把 `SystemTime` 转换成本地日历的 (year, month, day)。用 std-only 算。
/// 算法来源：Howard Hinnant 的 `days_from_civil` 反向版本。
fn chrono_like_local_date(t: SystemTime) -> (i32, u32, u32) {
    let days = days_from_unix(t);
    civil_from_days(days)
}

/// SystemTime → 距 1970-01-01 本地零点的天数
fn days_from_unix(t: SystemTime) -> i64 {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64;
    // 用本地时区偏移：把 secs 减去当前时区偏移（以天为单位，向下取整）
    let offset = local_tz_offset_secs();
    let local_secs = secs + offset;
    local_secs.div_euclid(86_400)
}

/// `t` 的本地 HH:MM 字符串
fn chrono_like_hhmm(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64;
    let offset = local_tz_offset_secs();
    let local_secs = secs + offset;
    let day_secs = local_secs.rem_euclid(86_400);
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    format!("{h:02}:{m:02}")
}

fn chrono_year(d: (i32, u32, u32)) -> i32 { d.0 }
fn chrono_month(d: (i32, u32, u32)) -> u32 { d.1 }
fn chrono_day(d: (i32, u32, u32)) -> u32 { d.2 }

/// Howard Hinnant's civil_from_days。
/// 输入：距 1970-01-01 的天数。输出：(year, month, day)。
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// 拿本地时区偏移（秒）。读系统时区 ——
/// - Windows：`GetTimeZoneInformation`
/// - macOS / Linux：`localtime_r`
///
/// 拿不到时回退到 UTC（返回 0），让"今天/昨天"判断至少不出 NaN。
fn local_tz_offset_secs() -> i64 {
    match time::UtcOffset::current_local_offset() {
        Ok(off) => off.whole_seconds() as i64,
        Err(e) => {
            // 第一次失败时 warn 一次（每次调用都 warn 太吵）。
            // 用一个 static AtomicBool 守门，简单够用。
            use std::sync::atomic::{AtomicBool, Ordering};
            static WARNED: AtomicBool = AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!("读系统时区失败 ({e})，UI 时间显示回退到 UTC");
            }
            0
        }
    }
}
