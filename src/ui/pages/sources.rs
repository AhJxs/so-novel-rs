//! 书源管理页：连通性检测 + 启用/禁用 toggle + 从 JSON 导入新书源。
//!
//! - 顶部 summary bar：左侧统计 chip（总数 / 启用 / 禁用 / 上次检测可用），
//!   右侧两个亮蓝实心按钮："添加" + "测速"。"测速"运行中时按钮变灰、显示 spinner。
//! - 启用/禁用立即持久化到 `sonovel.db` 的 `source_overrides` 表，下次启动保留偏好。
//! - 添加书源走 native file picker 选 JSON：支持单条对象或数组（兼容 main.json 格式）。
//! - 表格风格暂不动 —— 之后可参考下载任务卡片化，本次先不扩范围。

use crate::app::SoNovelApp;
use crate::crawler::health::SourceHealth;
use crate::ui::theme;
use material_icons::icons as mi;

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    show_toolbar(ui, app);
    ui.add_space(8.0);

    if let Some(err) = &app.rule_load_error {
        ui.colored_label(
            theme::semantic_danger(ui.style().visuals.dark_mode),
            format!("规则加载失败: {err}"),
        );
        ui.add_space(8.0);
    }

    show_table(ui, app);
}

/// 顶部 summary bar：左 chip 组 + 右按钮组。
///
/// chip 设计语言与下载任务页 `show_summary_bar` 完全一致（同 `theme::stat_chip`）：
/// - 总数始终画
/// - 启用 / 禁用 / 可用：值 > 0 时才出现，避免顶栏永远撑满
fn show_toolbar(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    let total = app.rules.len();
    let disabled = app.rules.iter().filter(|r| r.disabled).count();
    let enabled = total - disabled;

    // "可用"取上次 health-check 的结果（若有）。检测中（running）时不显示，
    // 避免一个旧值卡在那里误导用户。
    let available_after_check = if !app.sources_state.health.is_empty() && !app.sources_state.running
    {
        Some(
            app.sources_state
                .health
                .values()
                .filter(|h| {
                    h.error.is_none() && matches!(h.http_status, Some(s) if (200..400).contains(&s))
                })
                .count(),
        )
    } else {
        None
    };

    let dark = ui.style().visuals.dark_mode;
    ui.horizontal(|ui| {
        ui.set_min_height(theme::QUERY_HEIGHT);

        // ---- 左：统计 chip ----
        theme::stat_chip(ui, mi::ICON_DNS, "总数", total, theme::semantic_muted(dark));
        if enabled > 0 {
            ui.add_space(6.0);
            theme::stat_chip(
                ui,
                mi::ICON_CHECK_CIRCLE,
                "启用",
                enabled,
                theme::semantic_success(dark),
            );
        }
        if disabled > 0 {
            ui.add_space(6.0);
            theme::stat_chip(
                ui,
                mi::ICON_BLOCK,
                "禁用",
                disabled,
                theme::semantic_warn(dark),
            );
        }
        if let Some(ok) = available_after_check {
            ui.add_space(6.0);
            theme::stat_chip(
                ui,
                mi::ICON_NETWORK_CHECK,
                "可用",
                ok,
                theme::semantic_info(dark),
            );
        }

        // 检测中的进度提示：紧跟左侧 chip 之后；spinner 在前、文本在后，
        // 视觉上跟"在做的事 = 测速进度"对齐。检测结束自动消失。
        if app.sources_state.running {
            ui.add_space(10.0);
            ui.spinner();
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!(
                    "{}/{} 已返回",
                    app.sources_state.received, app.sources_state.expected
                )),
            );
        }

        // ---- 右：按钮组（推到行末，从右到左:测速 → 添加） ----
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // 测速：检测中 disabled；rules 空时也 disabled
            let speed_label = format!("{} 测速", mi::ICON_SPEED.codepoint);
            let speed_enabled = !app.sources_state.running && !app.rules.is_empty();
            if theme::primary_button(ui, &speed_label, speed_enabled) {
                app.spawn_health_check();
            }

            ui.add_space(6.0);

            // 添加：弹原生文件对话框选 JSON
            let add_label = format!("{} 添加", mi::ICON_ADD.codepoint);
            if theme::primary_button(ui, &add_label, true) {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON 规则文件", &["json", "json5"])
                    .add_filter("所有文件", &["*"])
                    .set_title("选择书源 JSON 文件")
                    .pick_file()
                {
                    app.add_sources_from_file(&path);
                }
            }
        });
    });
}

fn show_table(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    if app.rules.is_empty() {
        // 与下载任务 / 本地书库空态同款（图标 + 主副文案居中）。
        theme::empty_state(
            ui,
            mi::ICON_DNS,
            "暂无书源",
            "点击右上角『添加』从 JSON 文件导入",
        );
        return;
    }

    // 收集动作（避免 borrow 冲突；与 library 卡片同模式）
    let mut to_toggle: Option<i32> = None;
    let mut to_request_delete: Option<i32> = None;
    let mut to_confirm_delete: Option<i32> = None;
    let mut cancel_pending = false;

    let pending = app.sources_state.pending_delete;

    egui::ScrollArea::vertical()
        .id_salt("sources_list")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            // 卡片宽度循环外算一次，所有卡片共用同一个值。
            // 与 search / library 同样的处理 — 避免 ScrollArea 跨帧反馈造成卡片"逐张变宽"。
            let card_width = ui.available_width();
            for (idx, r) in app.rules.iter().enumerate() {
                let health = app.sources_state.health.get(&r.id);
                let is_pending = pending == Some(r.id);
                match source_card(ui, idx, r, health, is_pending, card_width) {
                    SourceAction::None => {}
                    SourceAction::Toggle => to_toggle = Some(r.id),
                    SourceAction::RequestDelete => to_request_delete = Some(r.id),
                    SourceAction::ConfirmDelete => to_confirm_delete = Some(r.id),
                    SourceAction::CancelDelete => cancel_pending = true,
                }
                ui.add_space(6.0);
            }
        });

    if let Some(id) = to_toggle {
        app.toggle_source_disabled(id);
        app.show_toast(format!(
            "书源 #{id} 已{}",
            if app.source_overrides.is_disabled(id) {
                "禁用"
            } else {
                "启用"
            }
        ));
    }
    if let Some(id) = to_request_delete {
        app.sources_state.pending_delete = Some(id);
    }
    if cancel_pending {
        app.sources_state.pending_delete = None;
    }
    if let Some(id) = to_confirm_delete {
        app.delete_source(id);
    }
}

/// 卡片点击产生的动作（统一收尾在 show_table 末尾），避免循环里多重借 app。
#[derive(Debug, Clone, Copy)]
enum SourceAction {
    None,
    /// 启用/禁用切换
    Toggle,
    /// 第一次点删除 → 进入二次确认态
    RequestDelete,
    /// 二次确认通过 → 真删
    ConfirmDelete,
    /// 二次确认中点取消
    CancelDelete,
}

/// 单条书源卡片。
///
/// 布局（与 library 卡片节奏一致）：
/// - 第一行：`#id + 书源名（strong）+ [需代理] chip` ⋯ 右侧按钮组
/// - 第二行（small + weak）：`URL · 延迟 · 状态`
///   测速前没有 health 数据 → 第二行只剩 URL；测速后追加彩色延迟 + 状态。
///
/// 右侧按钮组分两种状态：
/// - **常态**：`启用/禁用` + `删除`
/// - **二次确认**（`pending_delete = true`）：红色 `确认删除` + `取消`
///
/// 视觉与 search 结果卡 / library 卡片同源：
/// - 圆角 8、1px 描边、hover 浅底（memory 缓存 1 帧延迟反馈）
/// - **禁用态**：frame 浅底 + 整张卡片字体弱化，与启用态拉开层级
fn source_card(
    ui: &mut egui::Ui,
    idx: usize,
    r: &crate::models::Rule,
    health: Option<&SourceHealth>,
    pending_delete: bool,
    card_width: f32,
) -> SourceAction {
    let visuals = ui.style().visuals.clone();
    let dark = visuals.dark_mode;

    let hover_fill = if dark {
        egui::Color32::from_white_alpha(10)
    } else {
        egui::Color32::from_black_alpha(8)
    };
    // 禁用态浅底：用 muted 色派生一个非常淡的覆盖。比 hover 还淡，
    // 让"禁用"看着像 "凹" 下去而非被高亮。
    let disabled_fill = if dark {
        egui::Color32::from_white_alpha(4)
    } else {
        egui::Color32::from_black_alpha(4)
    };

    let card_inner_width = (card_width - 28.0).max(0.0);

    // hover 状态用 memory 缓存 1 帧（与 library / search 同款）。
    let card_id = egui::Id::new(("source_card", idx));
    let was_hovered = ui
        .ctx()
        .memory(|m| m.data.get_temp::<bool>(card_id).unwrap_or(false));

    let frame_fill = if r.disabled {
        // 禁用 + hover 时仍给 hover 反馈，但底色稍混合一下避免反差太大
        if was_hovered {
            hover_fill
        } else {
            disabled_fill
        }
    } else if was_hovered {
        hover_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    let frame_stroke = egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color);

    let mut action = SourceAction::None;

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
                            // 与卡片整体一致的圆角 8 按钮
                            let mut style: egui::Style = (**ui.style()).clone();
                            let r8 = egui::CornerRadius::same(8);
                            style.visuals.widgets.inactive.corner_radius = r8;
                            style.visuals.widgets.hovered.corner_radius = r8;
                            style.visuals.widgets.active.corner_radius = r8;
                            ui.set_style(style);

                            // ---- 左：标题行 + 元数据行 ----
                            ui.vertical(|ui| {
                                // 第一行：#id + 名 + [需代理]
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(format!("#{}", r.id)));
                                    ui.add_space(8.0);

                                    // 名 — 启用态 strong，禁用态降级到普通 + weak
                                    let name = if r.name.trim().is_empty() {
                                        "(未命名)".to_string()
                                    } else {
                                        r.name.clone()
                                    };
                                    let name_text = egui::RichText::new(name).size(14.5);
                                    let name_text = if r.disabled {
                                        name_text.weak()
                                    } else {
                                        name_text.strong()
                                    };
                                    ui.label(name_text);

                                    // 禁用态：紧跟名后 small "已禁用" 红色标
                                    if r.disabled {
                                        ui.add_space(8.0);
                                        ui.label(
                                            egui::RichText::new("已禁用")
                                                .small()
                                                .color(theme::semantic_danger(dark)),
                                        );
                                    }

                                    // 需代理：semantic_warn 小标
                                    if r.need_proxy {
                                        ui.add_space(8.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{} 需代理",
                                                mi::ICON_VPN_KEY.codepoint
                                            ))
                                            .small()
                                            .color(theme::semantic_warn(dark)),
                                        );
                                    }
                                });

                                ui.add_space(2.0);

                                // 第二行：URL · 延迟 · 状态
                                ui.horizontal(|ui| {
                                    ui.add_space(24.0); // 与 library 卡片同样的缩进

                                    // URL — 始终显示
                                    ui.label(
                                        egui::RichText::new(truncate(&r.url, 60))
                                            .small()
                                            .weak(),
                                    );

                                    // 测过速才追加延迟 / 状态
                                    if let Some(h) = health {
                                        ui.add_space(8.0);
                                        ui.label(
                                            egui::RichText::new("·").small().weak(),
                                        );
                                        ui.add_space(8.0);
                                        let (text, color) = latency_label(h, dark);
                                        ui.label(egui::RichText::new(text).small().color(color));

                                        ui.add_space(8.0);
                                        ui.label(
                                            egui::RichText::new("·").small().weak(),
                                        );
                                        ui.add_space(8.0);
                                        let (text, color) = status_label(h, dark);
                                        ui.label(egui::RichText::new(text).small().color(color));
                                    }
                                });
                            });

                            // ---- 右：按钮组（贴右、垂直居中于整张卡片） ----
                            // rtl 下添加顺序 = 视觉右起。常态：[启用/禁用]  [删除]（删除最右）
                            // 二次确认态：[取消]  [确认删除（红，最右）]
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if pending_delete {
                                        // 红色 "确认删除"（最右）
                                        let confirm = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new("确认删除")
                                                    .size(14.0)
                                                    .color(egui::Color32::WHITE),
                                            )
                                            .fill(theme::semantic_danger(dark))
                                            .corner_radius(egui::CornerRadius::same(8))
                                            .min_size(egui::vec2(72.0, 28.0)),
                                        );
                                        if confirm.clicked() {
                                            action = SourceAction::ConfirmDelete;
                                        }
                                        ui.add_space(6.0);
                                        // 普通 "取消"
                                        let cancel = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new("取消").size(14.0),
                                            )
                                            .corner_radius(egui::CornerRadius::same(8))
                                            .min_size(egui::vec2(56.0, 28.0)),
                                        );
                                        if cancel.clicked() {
                                            action = SourceAction::CancelDelete;
                                        }
                                    } else {
                                        // 删除（最右）
                                        let del = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new("删除").size(14.0),
                                            )
                                            .corner_radius(egui::CornerRadius::same(8))
                                            .min_size(egui::vec2(56.0, 28.0)),
                                        );
                                        if del.clicked() {
                                            action = SourceAction::RequestDelete;
                                        }
                                        ui.add_space(6.0);
                                        // 启用/禁用 toggle
                                        let label = if r.disabled { "启用" } else { "禁用" };
                                        let toggle = ui.add(
                                            egui::Button::new(
                                                egui::RichText::new(label).size(14.0),
                                            )
                                            .corner_radius(egui::CornerRadius::same(8))
                                            .min_size(egui::vec2(56.0, 28.0)),
                                        );
                                        if toggle.clicked() {
                                            action = SourceAction::Toggle;
                                        }
                                    }
                                },
                            );
                        });
                    })
                    .response
            },
        )
        .inner;

    ui.ctx().memory_mut(|m| {
        m.data.insert_temp(card_id, frame_resp.hovered());
    });

    action
}

/// 延迟字段的 (文本, 颜色)：
/// - <=400ms 绿；401..1500ms 黄；>=1500ms 暖橙；error 红 "超时/不通"；无数据 "-" 弱色。
fn latency_label(h: &SourceHealth, dark: bool) -> (String, egui::Color32) {
    if h.error.is_some() {
        return ("超时/不通".to_string(), theme::semantic_danger(dark));
    }
    let color = match h.delay_ms {
        0..=400 => theme::semantic_success(dark),
        401..=1500 => {
            if dark {
                egui::Color32::from_rgb(240, 200, 100)
            } else {
                egui::Color32::from_rgb(220, 180, 80)
            }
        }
        _ => theme::semantic_warn(dark),
    };
    (format!("{} ms", h.delay_ms), color)
}

/// 状态字段的 (文本, 颜色)：HTTP 200..399 绿、其它 HTTP 状态码暖橙、纯 error 红。
fn status_label(h: &SourceHealth, dark: bool) -> (String, egui::Color32) {
    if let Some(code) = h.http_status {
        let color = if (200..400).contains(&code) {
            theme::semantic_success(dark)
        } else {
            theme::semantic_warn(dark)
        };
        return (format!("HTTP {code}"), color);
    }
    if let Some(e) = &h.error {
        return (truncate(e, 32), theme::semantic_danger(dark));
    }
    ("-".to_string(), theme::semantic_muted(dark))
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}
