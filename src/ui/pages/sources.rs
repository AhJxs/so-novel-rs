//! 书源管理页。阶段 4c：连通性检测 + 启用/禁用 toggle（持久化到 sidecar JSON）。
//!
//! - 顶部："检测全部"按钮（HEAD 请求并发，5s 超时）+ 进度计数；
//! - 表格：ID / 书源 / URL / 代理 / 启用 / 延迟 / 状态 / 操作（启用/禁用）。
//! - 启用/禁用立即持久化到 `<config>/source-overrides.json`，下次启动保留偏好。

use crate::app::SoNovelApp;
use crate::crawler::health::SourceHealth;
use crate::ui::theme;
use material_icons::icons as mi;

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    ui.heading("书源管理");
    ui.add_space(4.0);
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

fn show_toolbar(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    ui.horizontal(|ui| {
        ui.label(format!(
            "当前激活规则文件: {}（共 {} 个书源）",
            app.config.active_rules,
            app.rules.len()
        ));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let btn = ui.add_enabled(
                !app.sources_state.running && !app.rules.is_empty(),
                egui::Button::new(format!("{} 检测全部", mi::ICON_LANGUAGE.codepoint)),
            );
            if btn.clicked() {
                app.spawn_health_check();
            }
            if app.sources_state.running {
                ui.spinner();
                ui.label(format!(
                    "{}/{} 已返回",
                    app.sources_state.received, app.sources_state.expected
                ));
            }
        });
    });

    if !app.sources_state.health.is_empty() && !app.sources_state.running {
        ui.add_space(4.0);
        let total = app.sources_state.health.len();
        let ok = app
            .sources_state
            .health
            .values()
            .filter(|h| {
                h.error.is_none() && matches!(h.http_status, Some(s) if (200..400).contains(&s))
            })
            .count();
        ui.label(
            egui::RichText::new(format!("上次检测：{ok} / {total} 可用"))
                .small()
                .weak(),
        );
    }
}

fn show_table(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    if app.rules.is_empty() {
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.label("规则文件为空或加载失败。请检查 [设置] 中的 active-rules。");
            });
        return;
    }

    // 收集 toggle 动作（避免 borrow 冲突）
    let mut to_toggle: Option<i32> = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("sources_grid")
            .striped(true)
            .min_col_width(60.0)
            .show(ui, |ui| {
                ui.strong("ID");
                ui.strong("书源");
                ui.strong("URL");
                ui.strong("代理");
                ui.strong("延迟");
                ui.strong("状态");
                ui.strong("启用");
                ui.strong("操作");
                ui.end_row();

                for r in &app.rules {
                    ui.label(r.id.to_string());
                    ui.label(&r.name);
                    ui.label(truncate(&r.url, 40));
                    ui.label(if r.need_proxy { "需要代理" } else { "" });

                    // 延迟 + 状态：从 sources_state.health 取
                    let health = app.sources_state.health.get(&r.id);
                    show_latency_cell(ui, health);
                    show_status_cell(ui, health);

                    // 启用/禁用展示
                    if r.disabled {
                        ui.colored_label(
                            theme::semantic_danger(ui.style().visuals.dark_mode),
                            "已禁用",
                        );
                    } else {
                        ui.colored_label(
                            theme::semantic_success(ui.style().visuals.dark_mode),
                            "启用",
                        );
                    }

                    // 操作：toggle
                    let label = if r.disabled { "启用" } else { "禁用" };
                    if ui.small_button(label).clicked() {
                        to_toggle = Some(r.id);
                    }
                    ui.end_row();
                }
            });
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
}

fn show_latency_cell(ui: &mut egui::Ui, h: Option<&SourceHealth>) {
    match h {
        None => {
            ui.label(egui::RichText::new("-").weak());
        }
        Some(h) if h.error.is_none() => {
            // 有结果且无 error：彩色标记延迟
            let dark = ui.style().visuals.dark_mode;
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
            ui.colored_label(color, format!("{} ms", h.delay_ms));
        }
        Some(_) => {
            // 有结果但失败
            ui.colored_label(
                theme::semantic_danger(ui.style().visuals.dark_mode),
                "超时/不通",
            );
        }
    }
}

fn show_status_cell(ui: &mut egui::Ui, h: Option<&SourceHealth>) {
    match h {
        None => {
            ui.label(egui::RichText::new("-").weak());
        }
        Some(h) => {
            let dark = ui.style().visuals.dark_mode;
            if let Some(code) = h.http_status {
                let color = if (200..400).contains(&code) {
                    theme::semantic_success(dark)
                } else {
                    theme::semantic_warn(dark)
                };
                ui.colored_label(color, format!("HTTP {code}"));
            } else if let Some(e) = &h.error {
                ui.colored_label(theme::semantic_danger(dark), truncate(e, 32));
            } else {
                ui.label("-");
            }
        }
    }
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
