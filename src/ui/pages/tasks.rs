//! 下载任务页。阶段 4b 打磨：
//! - 完成任务的"打开文件" / "打开所在目录"按钮；
//! - 失败 / 取消任务的"重新下载"按钮；
//! - 顶部"清除已完成"按钮。

use std::path::PathBuf;

use crate::app::{DownloadTask, SoNovelApp};
use crate::models::SearchResult;
use crate::design_system::{button, chip, color};

use crate::util::system::{open_path, reveal_in_folder};
use crate::util::time::{format_duration, format_unix_local};
use crate::material_icons::icons as mi;

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    show_summary_bar(ui, app);
    ui.add_space(8.0);

    if app.tasks.is_empty() {
        chip::empty_state(
            ui,
            mi::ICON_INBOX,
            "暂无下载任务",
            "去『搜索下载』选一本书试试",
        );
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
            app.show_toast_error(format!("打开失败: {e}"));
        }
    }
    if let Some(p) = to_reveal {
        if let Err(e) = reveal_in_folder(&p) {
            app.show_toast_error(format!("显示位置失败: {e}"));
        }
    }
    if let Some(r) = to_redownload {
        let _id = app.spawn_download(*r);
        app.show_toast("已重新加入下载");
    }
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
        ui.set_min_height(button::BAR_HEIGHT);

        // 左侧：从 5 个统计里挑非 0 的画 chip（"总数"始终画）
        chip::stat_chip(ui, mi::ICON_INVENTORY, "总数", total, color::semantic_muted(dark));
        if running > 0 {
            ui.add_space(6.0);
            chip::stat_chip(
                ui,
                mi::ICON_DOWNLOADING,
                "进行中",
                running,
                color::semantic_info(dark),
            );
        }
        if done > 0 {
            ui.add_space(6.0);
            chip::stat_chip(
                ui,
                mi::ICON_CHECK_CIRCLE,
                "完成",
                done,
                color::semantic_success(dark),
            );
        }
        if failed > 0 {
            ui.add_space(6.0);
            chip::stat_chip(
                ui,
                mi::ICON_WARNING,
                "失败",
                failed,
                color::semantic_warn(dark),
            );
        }
        if cancelled > 0 {
            ui.add_space(6.0);
            // 取消属用户主动操作，灰色 muted 不喧宾夺主
            chip::stat_chip(
                ui,
                mi::ICON_CANCEL,
                "已取消",
                cancelled,
                color::semantic_muted(dark),
            );
        }

        // 右侧：危险色"清除记录"主按钮，推到行末
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let any_finished = done + failed + cancelled > 0;
            let label = format!("{} 清除记录", mi::ICON_DELETE.codepoint);
            if button::danger_button(ui, &label, any_finished) {
                app.clear_finished_tasks();
            }
        });
    });
}

// chip / 实心按钮 / darken / lighten 已抽到 design_system crate，多个页面共用。
// 见 theme::{stat_chip, primary_button, danger_button, solid_button}。

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
        color::ACCENT
    } else if task.is_cancelled() {
        color::semantic_muted(dark)
    } else if task.is_failed() {
        color::semantic_warn(dark)
    } else {
        color::semantic_success(dark)
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

                            // #id —— 字号与作者一致（Body 13pt），颜色 = 状态色，strong
                            // 让 #1 / #2 这种序号在卡片左上角有视觉重量，跟边框+进度条
                            // 三位一体共同表达下载状态
                            ui.label(egui::RichText::new(format!("#{}", task.id)));
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
            // 完成：打开 + 位置
            if button::inline_icon(ui, "打开", mi::ICON_OPEN_IN_NEW) {
                action = TaskAction::Open(p.clone());
            }
            ui.add_space(6.0);
            if button::inline_icon(ui, "位置", mi::ICON_FOLDER_OPEN) {
                action = TaskAction::Reveal(p.clone());
            }
        }
        Some(Err(_)) => {
            // 失败 / 取消：单 重试
            if button::inline_icon(ui, "重试", mi::ICON_REFRESH) {
                action = TaskAction::Redownload(Box::new(task.origin.clone()));
            }
        }
        None => {
            // 运行中：单 取消
            if task.cancelling {
                button::InlineButton::new("取消中…")
                    .icon(mi::ICON_CANCEL)
                    .enabled(false)
                    .show(ui);
            } else if button::inline_icon(ui, "取消", mi::ICON_CANCEL) {
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
            egui::RichText::new(format_unix_local(task.started_at_unix).to_string()),
        );
        if let Some(d) = task.elapsed() {
            ui.label(egui::RichText::new("·").small().weak());
            let (label, color) = if task.is_running() {
                (
                    format!("已运行 {}", format_duration(d)),
                    color::semantic_info(dark),
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
                    .color(color::semantic_warn(dark)),
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

// 时间格式化（开始时间 / 耗时）已抽到 `crate::util::time` —— 多个页面共用。

