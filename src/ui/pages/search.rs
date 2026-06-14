//! 搜索下载页。阶段 4a：接入聚合搜索 + 触发下载。
//!
//! 交互流程：
//! 1. 用户输入关键词 + 选源（全部 / 单源） → 点"搜索"。
//! 2. 后台 `search_aggregated` 并发查每个源，结果通过 mpsc 推回；
//!    UI 表格逐源点亮 / 扩展。
//! 3. 选中某条结果 → "下载"按钮 → `app.spawn_download(result)`，
//!    任务进入 `app.tasks` 列表，由 update 循环排空进度通道。
//! 4. 下载触发后切到"下载任务"页（阶段 4b 实现，本阶段先在搜索页底部
//!    显示一条最近任务摘要做反馈）。

use crate::app::{SearchState, SoNovelApp, SourceStatus};
use crate::models::SearchResult;
use crate::ui::nav::NavPage;
use crate::ui::theme;

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    show_query_bar(ui, app);
    ui.add_space(8.0);
    show_source_status(ui, &app.search);
    ui.add_space(8.0);

    // 用 bottom_up 布局：先画底部"最近任务横幅"（如果有），再让结果列表占剩余高度。
    // 这样结果列表的高度自动随 banner 出现/消失而变化，banner 永远固定在底部。
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        // ---- 底部：最近任务横幅 ----
        // 用户手动 ✕ 关掉后，banner_dismissed_for 记录该任务 id，
        // 直到下次触发新下载（产生新 id）才重新显示。
        let show_banner = match app.tasks.last() {
            Some(t) => app.search.banner_dismissed_for != Some(t.id),
            None => false,
        };
        if show_banner {
            show_task_banner(ui, app);
            ui.add_space(4.0);
        }

        // ---- 上方：结果列表（占剩余高度）----
        // 在 bottom_up 内嵌一个 top_down 区域，让结果列表内部仍然按"从上到下"渲染。
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            show_results(ui, app);
        });
    });
}

/// 底部最近任务横幅。从 `show()` 提取出来便于在 bottom_up 布局里调用。
///
/// 右侧布局（从右到左）：✕ 关闭 → 查看任务 → ...... → 左侧文字。
/// 关闭按钮把当前任务 id 写入 `banner_dismissed_for`，下次新下载产生新 id 时
/// banner 自动重新显示。
fn show_task_banner(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    let t = app.tasks.last().expect("non-empty checked by caller");
    let task_id = t.id;
    // 状态图标 + 文案拆开：图标用 painter 画（见 `task_status_icon`），
    // 文字里去掉了原 emoji 前缀（✅/⚠️/📥），避免 unicode 字符在 CJK 字体
    // 下渲染为豆腐块 — Noto Sans SC 也不保证覆盖 Symbols 块。
    let (status, label) = if t.is_running() {
        (
            TaskStatus::Running,
            format!(
                "下载中：{}（{} / {}）",
                t.book_name(),
                t.completed,
                t.total_chapters.max(t.completed as usize)
            ),
        )
    } else {
        match &t.finished {
            Some(Ok(p)) => (TaskStatus::Completed, format!("已完成：{}", p.display())),
            Some(Err(reason)) => (
                TaskStatus::Warning,
                format!("已结束：{} — {reason}", t.book_name()),
            ),
            None => (TaskStatus::Running, format!("准备中：{}", t.book_name())),
        }
    };

    let visuals = ui.style().visuals.clone();
    let mut go_tasks = false;
    let mut dismiss = false;
    egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(visuals.widgets.noninteractive.bg_stroke)
        .corner_radius(egui::CornerRadius::same(8))
        .outer_margin(egui::Margin::symmetric(0, 8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // 行高 36：action_button 自带 min_size(0, 30) = 30px 实际高度，
                // 加 3px 上下缓冲让 label / ✕ 按钮有清晰的视觉中心，不会贴边。
                // 用外层 `ui.horizontal` 的默认 `Align::Center` 做垂直居中 —
                // 比再嵌一层 `horizontal_centered` 更直接，label 也不再被水平居中（应是左对齐）。
                ui.set_min_height(36.0);
                let mut style: egui::Style = (**ui.style()).clone();
                let r8 = egui::CornerRadius::same(8);
                style.visuals.widgets.inactive.corner_radius = r8;
                style.visuals.widgets.hovered.corner_radius = r8;
                style.visuals.widgets.active.corner_radius = r8;
                ui.set_style(style);

                task_status_icon(ui, status);
                ui.add_space(6.0);
                ui.label(label);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // ✕ 关闭按钮：手画 X 线段，避免 unicode ✕ 在某些字体下不渲染
                    if close_x_button(ui).on_hover_text("隐藏此横幅").clicked() {
                        dismiss = true;
                    }
                    ui.add_space(4.0);
                    if theme::action_button(ui, "查看任务").clicked() {
                        go_tasks = true;
                    }
                });
            });
        });
    if dismiss {
        app.search.banner_dismissed_for = Some(task_id);
    }
    if go_tasks {
        app.current_page = NavPage::Tasks;
    }
}

/// 三件套（输入框 / 下拉 / 搜索按钮）统一高度。
const QUERY_HEIGHT: f32 = 34.0;

use crate::ui::theme::ACCENT;

fn show_query_bar(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    let visuals = ui.style().visuals.clone();

    ui.horizontal(|ui| {
        // ---- 1. 输入框：自定义 Frame（圆角 + border），内嵌 🔍 图标 + TextEdit ----
        // 不能直接给 egui::TextEdit 设圆角；包一层 Frame 自己画背景 + 圆角 + 边框。
        // 固定宽度 360，避免 TextEdit::desired_width 把行撑爆。
        const INPUT_W: f32 = 360.0;
        const ICON_W: f32 = 22.0; // 🔍 字符 + 一点空隙

        let input_frame = egui::Frame::new()
            .fill(visuals.extreme_bg_color)
            // border 颜色与文字色一致（按用户要求），整个输入框是"文字色"线框。
            .stroke(egui::Stroke::new(1.0, visuals.text_color()))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin::symmetric(10, 0));

        let enter_pressed = ui
            .scope(|ui| {
                ui.set_max_width(INPUT_W);
                input_frame
                    .show(ui, |ui| {
                        ui.set_min_size(egui::vec2(INPUT_W, QUERY_HEIGHT));
                        ui.horizontal_centered(|ui| {
                            ui.label(
                                material_icons::icons::ICON_SEARCH
                                    .rich_text()
                                    .size(14.0)
                                    .color(visuals.weak_text_color()),
                            );
                            // TextEdit 占去除图标 + 左右 padding 之后的剩余宽度
                            let edit_w = INPUT_W - ICON_W - 20.0;
                            let edit = egui::TextEdit::singleline(&mut app.search.keyword)
                                .hint_text("书名 / 作者")
                                .frame(egui::Frame::NONE)
                                .desired_width(edit_w)
                                .vertical_align(egui::Align::Center);
                            let resp = ui.add(edit);
                            resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        })
                        .inner
                    })
                    .inner
            })
            .inner;

        ui.add_space(6.0);

        // ---- 2. 书源下拉：固定高度 + 圆角 8px + 自定义箭头（打开时翻转）----
        let current_label = match app.search.source_id {
            None => "全部书源（聚合）".to_string(),
            Some(id) => app
                .rules
                .iter()
                .find(|r| r.id == id)
                .map(|r| format!("{} ({})", r.name, r.id))
                .unwrap_or_else(|| format!("书源 {id}（已下线？）")),
        };
        ui.allocate_ui_with_layout(
            egui::vec2(220.0, QUERY_HEIGHT),
            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
            |ui| {
                // ComboBox 用 style.widgets.{inactive,hovered,active,open}.corner_radius；
                // 这里在子 scope 改 style，不污染外层 ui。
                // 注意 ui.style() 返回 &Arc<Style>；要拿可变引用必须先 clone Style。
                let mut style: egui::Style = (**ui.style()).clone();
                let r = egui::CornerRadius::same(8);
                style.visuals.widgets.inactive.corner_radius = r;
                style.visuals.widgets.hovered.corner_radius = r;
                style.visuals.widgets.active.corner_radius = r;
                style.visuals.widgets.open.corner_radius = r;
                // 加大水平内边距：全局默认 (10, 6) 让"全部书源（聚合）"这种长 label
                // 视觉上贴边；这里只对 ComboBox 及其下拉项生效，不影响其他按钮。
                style.spacing.button_padding = egui::vec2(8.0, 0.0);
                ui.set_style(style);

                egui::ComboBox::from_id_salt("search_source")
                    .selected_text(current_label)
                    .width(220.0)
                    .height(360.0)
                    // 自定义箭头：is_open=true 时画"向上 ▲"，否则"向下 ▼"
                    .icon(|ui, rect, vis, is_open| {
                        let painter = ui.painter();
                        let center = rect.center();
                        let h = (rect.height() * 0.18).clamp(3.0, 5.0);
                        let w = h * 1.4;
                        // is_open=false 时尖朝下（向下三角）；is_open=true 时尖朝上
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
                        ui.selectable_value(&mut app.search.source_id, None, "全部书源（聚合）");
                        for r in &app.rules {
                            if r.disabled {
                                continue;
                            }
                            let label = format!("{} ({})", r.name, r.id);
                            ui.selectable_value(&mut app.search.source_id, Some(r.id), label);
                        }
                    });
            },
        );

        ui.add_space(6.0);

        // ---- 3. 搜索按钮：与导航选中按钮同款（亮蓝填充 + 白字 + 圆角 8px）----
        let search_label = format!("{} 搜索", material_icons::icons::ICON_SEARCH.codepoint);
        let search_clicked = nav_style_button(ui, &search_label, !app.search.running);

        if (search_clicked || enter_pressed) && !app.search.running {
            let _ = app.spawn_search();
        }
        if app.search.running {
            ui.add_space(6.0);
            ui.spinner();
            ui.label(format!(
                "{}/{} 源已返回",
                app.search.received, app.search.expected
            ));
        }
    });

    if let Some(err) = &app.search.last_error {
        ui.add_space(4.0);
        ui.colored_label(
            theme::semantic_danger(ui.style().visuals.dark_mode),
            format!("⚠ {err}"),
        );
    }
}
/// 与 `ui::nav` 的选中按钮同款：亮蓝填充 + 白字 + 圆角 8px + 阴影 + 高度统一。
/// 仅 disabled 时变灰、无阴影。
fn nav_style_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    const BTN_ROUNDING: egui::CornerRadius = egui::CornerRadius::same(8);
    const BTN_PADDING_X: f32 = 18.0;

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
    let desired_size = egui::vec2(text_w + BTN_PADDING_X * 2.0, QUERY_HEIGHT);

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

    // 状态判断：是否被按下（鼠标按住）/ 是否 hover
    let is_pressed = enabled && response.is_pointer_button_down_on();
    let is_hovered = enabled && response.hovered();

    // 颜色
    let (fill, text_color) = if !enabled {
        (visuals.widgets.inactive.bg_fill, visuals.weak_text_color())
    } else if is_pressed {
        // 按下时颜色稍深（点击反馈），用 ACCENT 加点黑色叠加
        (egui::Color32::from_rgb(42, 110, 200), egui::Color32::WHITE)
    } else if is_hovered {
        // hover 时颜色稍亮
        (egui::Color32::from_rgb(72, 148, 240), egui::Color32::WHITE)
    } else {
        (ACCENT, egui::Color32::WHITE)
    };

    // 点击时整个按钮下沉 1px（视觉"按下"反馈），同时阴影变小
    let press_offset = if is_pressed {
        egui::vec2(0.0, 1.0)
    } else {
        egui::vec2(0.0, 0.0)
    };
    let rect = rect.translate(press_offset);

    // 阴影（仅 enabled，按下时阴影更紧贴减半）
    if enabled {
        let layers: [(f32, u8); 3] = if is_pressed {
            // 按下时阴影变浅 + 偏移变小
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

    // 文字 — 用 mesh_bounds 居中（与 nav 按钮一致）
    let galley = painter.layout_no_wrap(text.to_string(), font_id, text_color);
    let mesh = galley.mesh_bounds;
    let anchor = rect.center() - mesh.center().to_vec2();
    painter.galley(anchor, galley, text_color);

    response.clicked()
}

fn show_source_status(ui: &mut egui::Ui, search: &SearchState) {
    if search.source_status.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for (id, name, status) in &search.source_status {
            chip(ui, *id, name, status);
            ui.add_space(4.0);
        }
    });
}
/// 单个书源状态 chip：左侧圆形状态点 + 右侧"书源名#ID"文字。
///
/// 颜色按状态区分：
/// - Pending：灰（等待）
/// - Ok(>0)：绿（搜到结果）
/// - Ok(0)：琥珀（连得通但 0 条）
/// - Err：红（失败）
///
/// 视觉：
/// - 整体一个圆角胶囊（rounding=12px），fill 用状态色 8% alpha 浅底
/// - stroke 1px 用状态色 60% alpha
/// - 左侧 8x8 实心圆 用纯状态色（饱和点）
/// - 文字用普通色（非状态色）以保证可读性
fn chip(ui: &mut egui::Ui, id: i32, name: &str, status: &SourceStatus) {
    // 状态色（饱和原色）。Pending 用语义 muted（暗 / 亮主题下都不过分）；
    // Ok(0) / Ok / Err 都用对应的 success / warn / danger helper。
    let dark = ui.style().visuals.dark_mode;
    let dot_color = match status {
        SourceStatus::Pending => theme::semantic_muted(dark),
        SourceStatus::Ok(0) => {
            // "通了但 0 结果" 用比 success 稍暖的色，与"找得到"区分。
            if dark {
                egui::Color32::from_rgb(240, 190, 110)
            } else {
                egui::Color32::from_rgb(220, 170, 60)
            }
        }
        SourceStatus::Ok(_) => theme::semantic_success(dark),
        SourceStatus::Err(_) => theme::semantic_danger(dark),
    };

    // 文字（不再带 suffix；按用户要求）
    let label_text = format!("{name}#{id}");
    let visuals = ui.style().visuals.clone();
    let dark_mode = visuals.dark_mode;

    // 测量文字
    let font_id = egui::FontId::proportional(
        ui.style()
            .text_styles
            .get(&egui::TextStyle::Body)
            .map(|f| f.size)
            .unwrap_or(13.0),
    );
    let galley =
        ui.painter()
            .layout_no_wrap(label_text.clone(), font_id.clone(), visuals.text_color());

    // 尺寸：左 padding(10) + 状态点(8) + 间隔(6) + 文字 + 右 padding(10)；高 24
    const DOT_SIZE: f32 = 8.0;
    const PAD_X: f32 = 10.0;
    const GAP: f32 = 6.0;
    const HEIGHT: f32 = 24.0;
    let desired = egui::vec2(PAD_X + DOT_SIZE + GAP + galley.size().x + PAD_X, HEIGHT);

    // hover 上 tooltip 显示完整状态（错误信息 / 数量等）
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::hover());
    let response = response.on_hover_text(match status {
        SourceStatus::Pending => "等待响应".to_string(),
        SourceStatus::Ok(n) => format!("找到 {n} 条结果"),
        SourceStatus::Err(reason) => format!("失败：{reason}"),
    });

    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter();

    // 背景：状态色 ~10% alpha；暗色模式下用 white_alpha 叠
    let bg = egui::Color32::from_rgba_unmultiplied(
        dot_color.r(),
        dot_color.g(),
        dot_color.b(),
        if dark_mode { 32 } else { 22 },
    );
    let stroke_color =
        egui::Color32::from_rgba_unmultiplied(dot_color.r(), dot_color.g(), dot_color.b(), 140);
    painter.rect_filled(rect, egui::CornerRadius::same(12), bg);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(12),
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );

    // 状态点（实心圆）
    let dot_center = egui::pos2(rect.left() + PAD_X + DOT_SIZE / 2.0, rect.center().y);
    painter.circle_filled(dot_center, DOT_SIZE / 2.0, dot_color);
    // 给"等待中"的灰点加一圈外环动画感（这里只画静态外环，无动画）
    if matches!(status, SourceStatus::Pending) {
        painter.circle_stroke(
            dot_center,
            DOT_SIZE / 2.0 + 2.0,
            egui::Stroke::new(1.0, stroke_color),
        );
    }

    // 文字
    let text_pos = egui::pos2(rect.left() + PAD_X + DOT_SIZE + GAP, rect.center().y);
    let mesh = galley.mesh_bounds;
    // 垂直居中：rect.center().y 对齐 mesh.center().y；水平左对齐 text_pos.x
    let anchor = egui::pos2(text_pos.x, rect.center().y - mesh.center().y);
    painter.galley(anchor, galley, visuals.text_color());

    let _ = response;
}

fn show_results(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    if app.search.results.is_empty() {
        if app.search.running {
            ui.label("正在等待结果…");
        } else if app.search.last_keyword.is_some() {
            ui.label(
                egui::RichText::new("没有找到结果。可能是关键词不准确，或所有书源都不可用。")
                    .weak(),
            );
        } else {
            ui.label(
                egui::RichText::new("输入关键词，回车开始搜索。聚合搜索会并发请求所有启用的书源。")
                    .weak(),
            );
        }
        return;
    }

    let total = app.search.results.len();
    ui.label(format!(
        "共 {total} 条结果（{}）",
        app.search.last_keyword.as_deref().unwrap_or("（无关键词）")
    ));
    ui.add_space(4.0);

    // 收集动作（避免 borrow 冲突）
    let mut to_download: Option<SearchResult> = None;
    let mut to_select: Option<usize> = None;

    // 列表占满整个剩余宽度 + 高度。auto_shrink([false; 2]) 让 ScrollArea
    // 不收缩到内容尺寸，而是撑满父容器；这样下方有大片空白时也能滚动。
    //
    // 重新搜索后 `pending_scroll_top = true` → 这一帧给 `vertical_scroll_offset(0.0)`
    // 强制滚到顶；之后清零，用户继续手动滑就不再被打断。直接 every-frame 设 0.0
    // 会导致永远滑不动。
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt("search_results_list")
        .auto_shrink([false; 2]);
    if app.search.pending_scroll_top {
        scroll = scroll.vertical_scroll_offset(0.0);
        app.search.pending_scroll_top = false;
    }
    scroll.show(ui, |ui| {
        // 把卡片宽度在循环外算一次，所有卡片共用同一个值。
        // 不能在 result_card 内调 ui.available_width()：在 ScrollArea 里调，
        // 第一张卡片渲染后的 min_rect 会反馈给 ScrollArea，ScrollArea 缓存到下一帧
        // 时给的 inner_ui 宽度可能略不同，造成卡片"逐张变宽"的雪球。
        // 在循环外取一次，硬约束每张卡片用同样的宽度。
        let card_width = ui.available_width();
        let selected = app.search.selected;
        // 每行一张卡片：宽度撑满（available_width），圆角 8，
        // selected 时用 selection 强调色，hover 时浅底反馈。
        for (idx, r) in app.search.results.iter().enumerate() {
            let is_selected = selected == Some(idx);
            let action = result_card(ui, idx, r, is_selected, card_width);
            match action {
                CardAction::Download => to_download = Some(r.clone()),
                CardAction::OpenDetail => {
                    to_select = Some(idx);
                    app.search.detail_popup_for = Some(idx);
                }
                CardAction::None => {}
            }
            ui.add_space(6.0);
        }
    });

    if let Some(idx) = to_select {
        app.select_search_result(idx);
    }
    if let Some(target) = to_download {
        let _id = app.spawn_download(target);
        app.show_toast("已加入下载，可在『下载任务』查看进度");
    }

    // 详情弹窗（点书名后打开）
    show_detail_popup(ui.ctx(), app);
}

/// 详情弹窗：根据 `app.search.detail_popup_for` 渲染当前行的详情。
///
/// 弹窗用 `egui::Window`：自带阴影、圆角、可拖动；按 ESC 或点 ✕ 关闭。
/// 详情数据来自 `select_search_result` 触发的 detail_cache（异步加载）。
fn show_detail_popup(ctx: &egui::Context, app: &mut SoNovelApp) {
    let Some(idx) = app.search.detail_popup_for else {
        return;
    };
    // 索引可能因结果列表重排（filter_sort）失效；safety check
    let Some(r) = app.search.results.get(idx).cloned() else {
        app.search.detail_popup_for = None;
        return;
    };

    // ESC 关闭
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.search.detail_popup_for = None;
        return;
    }

    let mut open = true;
    let title = format!("📖 {}", r.book_name);
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable([true, true])
        .default_width(520.0)
        .default_height(420.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            render_detail_body(ui, app, &r);
        });
    if !open {
        app.search.detail_popup_for = None;
    }
}

fn render_detail_body(ui: &mut egui::Ui, app: &SoNovelApp, r: &SearchResult) {
    use crate::app::{CoverEntry, DetailState};

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("作者：").weak());
        ui.label(r.author.as_deref().unwrap_or("（未知）"));
        ui.separator();
        ui.label(egui::RichText::new("来源：").weak());
        ui.label(format!("{}#{}", r.source_name, r.source_id));
    });
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(6.0);

    let key = (r.source_id, r.url.clone());
    match app.search.detail_cache.get(&key) {
        None => {
            ui.label(egui::RichText::new("准备加载详情…").weak());
        }
        Some(DetailState::Pending) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("正在加载详情…");
            });
        }
        Some(DetailState::Failed(reason)) => {
            ui.colored_label(
                theme::semantic_warn(ui.style().visuals.dark_mode),
                format!("详情加载失败：{reason}"),
            );
        }
        Some(DetailState::Loaded(book)) => {
            // 双栏：左封面（200x280 max），右文字
            ui.horizontal_top(|ui| {
                // 左：封面
                if let Some(cover_url) = book
                    .cover_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    let ckey = (r.source_id, cover_url.to_string());
                    let in_flight = app.search.cover_in_flight.contains(&ckey);
                    ui.allocate_ui(egui::vec2(200.0, 280.0), |ui| {
                        match app.search.cover_cache.get(&ckey) {
                            Some(CoverEntry::Ready(img)) => {
                                ui.add(img.clone().max_size(egui::vec2(200.0, 280.0)));
                            }
                            Some(CoverEntry::Failed(reason)) => {
                                ui.colored_label(
                                    theme::semantic_warn(ui.style().visuals.dark_mode),
                                    format!("封面加载失败：{reason}"),
                                );
                            }
                            None if in_flight => {
                                ui.spinner();
                                ui.label("封面下载中…");
                            }
                            None => {
                                ui.label(egui::RichText::new("封面等待中…").small().weak());
                            }
                        }
                    });
                    ui.add_space(12.0);
                }

                // 右：元信息 + 简介
                ui.vertical(|ui| {
                    if let Some(c) = &book.category {
                        ui.label(format!("分类：{c}"));
                    }
                    if let Some(s) = &book.status {
                        ui.label(format!("状态：{s}"));
                    }
                    if let Some(latest) = &book.latest_chapter {
                        ui.label(format!("最新章节：{latest}"));
                    }
                    if let Some(t) = &book.last_update_time {
                        ui.label(format!("更新时间：{t}"));
                    }
                    if let Some(intro) = &book.intro {
                        ui.add_space(8.0);
                        ui.strong("简介");
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .id_salt("popup_intro_scroll")
                            .max_height(220.0)
                            .show(ui, |ui| {
                                ui.label(intro);
                            });
                    }
                });
            });
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n).collect();
    out.push('…');
    out
}

/// 单条搜索结果卡片产生的动作。
enum CardAction {
    None,
    /// 触发下载
    Download,
    /// 打开详情弹窗（点详情按钮）— 同时会触发选中（拉详情）
    OpenDetail,
}

/// 渲染一张搜索结果卡片。
///
/// `card_width` 由调用方在 ScrollArea 闭包外**一次性**算好传入 —— 不要在本函数
/// 内调 `ui.available_width()`：ScrollArea 跨帧缓存 content_size，逐张读会让
/// 卡片宽度雪球扩张。
///
/// 布局（从左到右）：序号 → 书名（强、不可点） → 副信息 → ... → 详情 + 下载 按钮（贴右）
/// - 卡片宽度严格 = `card_width`（用 `allocate_ui_with_layout(vec2(card_width,0))` 硬钉住）
/// - 圆角 8px；selected 用 ACCENT 色；hover 浅底（1 帧延迟）
/// - 书名是普通 label，不可点击；点击进入详情走右侧"详情"按钮
fn result_card(
    ui: &mut egui::Ui,
    idx: usize,
    r: &SearchResult,
    selected: bool,
    card_width: f32,
) -> CardAction {
    let visuals = ui.style().visuals.clone();
    let dark_mode = visuals.dark_mode;

    let selected_fill = if dark_mode {
        egui::Color32::from_rgba_unmultiplied(58, 134, 230, 35)
    } else {
        egui::Color32::from_rgba_unmultiplied(58, 134, 230, 25)
    };
    let selected_stroke = egui::Color32::from_rgb(58, 134, 230);
    let hover_fill = if dark_mode {
        egui::Color32::from_white_alpha(10)
    } else {
        egui::Color32::from_black_alpha(8)
    };

    // 卡片宽度由调用方钉死；frame 的 inner_margin = symmetric(14, 10)，
    // 所以 horizontal 内层 ui 的可用宽度 = card_width - 28（左右各 14）。
    let card_inner_width = (card_width - 28.0).max(0.0);

    // hover 反馈：Frame::fill 在 show 时定型，借 memory 记上一帧 hover；
    // 视觉延迟 1 帧（~16ms）人感不到。
    let card_id = egui::Id::new(("result_card", idx));
    let was_hovered = ui
        .ctx()
        .memory(|m| m.data.get_temp::<bool>(card_id).unwrap_or(false));

    let frame_fill = if selected {
        selected_fill
    } else if was_hovered {
        hover_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    let frame_stroke = if selected {
        egui::Stroke::new(1.0, selected_stroke)
    } else {
        egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color)
    };

    let mut button_clicked = false;
    let mut detail_clicked = false;

    // 关键：硬分配一个 `card_width × auto_height` 的盒子，让 Frame 在固定宽度的
    // 父 ui 里绘制 —— Frame 只负责绘背景/边框/inner_margin，不再决定卡片外宽。
    // 这样无论内层内容（书名 / 按钮组）实际像素宽是多少，卡片外沿都不会撑大。
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

                        ui.horizontal(|ui| {
                            ui.set_min_height(32.0);

                            // 子 scope 改按钮 corner_radius 为 8，与整体一致
                            let mut style: egui::Style = (**ui.style()).clone();
                            let r8 = egui::CornerRadius::same(8);
                            style.visuals.widgets.inactive.corner_radius = r8;
                            style.visuals.widgets.hovered.corner_radius = r8;
                            style.visuals.widgets.active.corner_radius = r8;
                            ui.set_style(style);

                            ui.label(
                                egui::RichText::new(format!("#{}", idx + 1))
                                    .small()
                                    .weak(),
                            );
                            ui.add_space(8.0);

                            // 书名 — 不可点击的强 label
                            ui.label(
                                egui::RichText::new(truncate(&r.book_name, 28))
                                    .strong()
                                    .size(14.5),
                            );

                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("·").weak());
                            ui.add_space(10.0);
                            ui.label(truncate(r.author.as_deref().unwrap_or("未知"), 14));
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("·").weak());
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}#{}",
                                    r.source_name, r.source_id
                                ))
                                .weak(),
                            );
                           if let Some(latest) = r.latest_chapter.as_deref().filter(|s| !s.is_empty()) {
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("·").weak());
                            ui.add_space(10.0);
                            ui.label(
                                egui::RichText::new(format!("最新：{}", truncate(latest, 28)))
                                    .small()
                                    .weak(),
                            );
                }

                            // 右侧：详情 + 下载 按钮组，靠 right_to_left 推到行末。
                            // 添加顺序即视觉顺序（第一个最右）：先下载再详情 →
                            // 下载在右、详情在左。直接 ui.add Button —— 不嵌
                            // ui.scope，避免在 right_to_left 里 cursor 重复扣减。
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let download = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("下载").size(14.0),
                                        )
                                        .corner_radius(egui::CornerRadius::same(8))
                                        .min_size(egui::vec2(56.0, 28.0)),
                                    );
                                    if download.clicked() {
                                        button_clicked = true;
                                    }
                                    ui.add_space(6.0);
                                    let detail = ui.add(
                                        egui::Button::new(
                                            egui::RichText::new("详情").size(14.0),
                                        )
                                        .corner_radius(egui::CornerRadius::same(8))
                                        .min_size(egui::vec2(56.0, 28.0)),
                                    );
                                    if detail.clicked() {
                                        detail_clicked = true;
                                    }
                                },
                            );
                        });
                    })
                    .response
            },
        )
        .inner;

    // hover 状态写回 memory，供下一帧渲染 fill
    ui.ctx().memory_mut(|m| {
        m.data.insert_temp(card_id, frame_resp.hovered());
    });

    if button_clicked {
        return CardAction::Download;
    }
    if detail_clicked {
        // 详情按钮既触发选中（拉详情），也打开详情弹窗
        return CardAction::OpenDetail;
    }
    CardAction::None
}

/// banner 用的下载任务状态。跟 `task_status_icon` 配合 — 状态决定图标的
/// 形状 + 颜色，避免在 label 文本里塞 unicode emoji。
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum TaskStatus {
    /// 运行中 / 准备中：蓝色下载中图标
    Running,
    /// 已完成：绿色对勾圆
    Completed,
    /// 已结束但带错误/取消：橙色警告三角
    Warning,
}

/// 在 horizontal 布局里画一个 18x18 的状态图标。颜色随主题走
/// `theme::ACCENT` / `semantic_success` / `semantic_warn`。
///
/// 用 `material_icons`（Material Symbols Rounded 字体）渲染 — 跟之前
/// 的 painter 几何方案相比：图标本身跨主题一致、emoji-like 视觉风格，
/// 仍然不依赖任何 CJK 字形（material 字体是独立 codepoint 空间）。
fn task_status_icon(ui: &mut egui::Ui, status: TaskStatus) -> egui::Response {
    use material_icons::icons as mi;
    const SIZE: f32 = 18.0;

    let (rect, response) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let dark_mode = ui.style().visuals.dark_mode;
    let (icon, color) = match status {
        TaskStatus::Running => (mi::ICON_DOWNLOADING, theme::ACCENT),
        TaskStatus::Completed => (mi::ICON_CHECK_CIRCLE, theme::semantic_success(dark_mode)),
        TaskStatus::Warning => (mi::ICON_WARNING, theme::semantic_warn(dark_mode)),
    };

    // 用 painter.text 居中绘制，font family 走 material-icons 让 codepoint 命中。
    // crate 在 initialize() 里把 y_offset_factor 设成 0.05，Align2::CENTER_CENTER
    // 会自动应用，不会偏上。
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon.codepoint,
        egui::FontId::new(SIZE, icon.font_family()),
        color,
    );

    response
}

/// 22x22 的 ✕ 关闭按钮：透明 / 红色 hover 背景 + Material `close` 图标。
///
/// 之前用 painter 手画 ✕ 线段；现在改用 `ICON_CLOSE`（vendor 在
/// `material_icons`），跨主题一致、不依赖 CJK 字形。
/// hover 时的红底效果保留。
fn close_x_button(ui: &mut egui::Ui) -> egui::Response {
    use material_icons::icons::ICON_CLOSE;
    const SIZE: f32 = 22.0;
    const ICON_PX: f32 = 16.0;

    let (rect, response) = ui.allocate_exact_size(egui::vec2(SIZE, SIZE), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let dark_mode = ui.style().visuals.dark_mode;
    let (bg, icon_color) = if response.hovered() {
        (Some(egui::Color32::from_rgb(232, 17, 35)), egui::Color32::WHITE)
    } else {
        let c = if dark_mode {
            egui::Color32::from_white_alpha(180)
        } else {
            egui::Color32::from_black_alpha(160)
        };
        (None, c)
    };

    if let Some(bg) = bg {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(4), bg);
    }

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        ICON_CLOSE.codepoint,
        egui::FontId::new(ICON_PX, ICON_CLOSE.font_family()),
        icon_color,
    );

    response
}
