//! 设置页。
//!
//! 视觉风格：iOS 系统设置 — 同一类设置一个分组卡片，分组内多设置项用细分割线隔开。
//! 每行：左侧上下结构（标题 + 副标题）+ 右侧输入控件 / 开关。
//!
//! 数据流：
//! - UI 直接改 `app.config`（不再维护 `draft_config` 副本）
//! - 任一控件 `.changed()` 设 `app.settings_dirty = true`
//! - `show()` 末尾统一 `app.persist_settings()` 写盘
//! - 写盘失败会在 toast 报错但**不回滚** dirty，下次还会再试

use crate::app::SoNovelApp;
use crate::config::ExportFormat;
use crate::config::LangType;
use crate::design_system::{button, input, settings, theme_picker, toggle};
use egui::Ui;
use crate::material_icons::icons as mi;

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    let ctx = ui.ctx().clone();

    egui::ScrollArea::vertical().show(ui, |ui| {
        // ---- 外观 ----
        card(ui, "外观", |ui| {
            separator(ui);
            let mut changed = false;
            settings::settings_row(ui, "主题", None, |ui| {
                changed = theme_picker::theme_segmented_control(ui, &mut app.config.theme);
            });
            if changed {
                ctx.set_theme(app.config.theme.to_theme_preference());
                app.settings_dirty = true;
            }
        });

        // ---- 下载 ----
        card(ui, "下载", |ui| {
            if toggle_row(ui, "保留章节缓存目录",
                Some("关闭后下次重新下载会全章节再爬"),
                &mut app.config.preserve_chapter_cache) {
                app.settings_dirty = true;
            }
            if toggle_row(ui, "启用下载进度条",
                Some("终端/CLI 模式下生效"),
                &mut app.config.enable_progressbar) {
                app.settings_dirty = true;
            }
            // 下载目录：副标题实时显示当前路径（让用户一眼看到生效值），
            // 右侧是「选择...」按钮 → rfd 弹原生文件夹选择对话框。
            // 副标题先拷到 String，释放对 app 的借用；否则闭包里再 &mut app 冲突。
            let current = app.config.download_path.clone();
            separator(ui);
            let mut changed = false;
            settings::settings_row(ui, "下载目录", Some(&current), |ui| {
                if button::inline_icon(ui, "选择", mi::ICON_FOLDER_OPEN) {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("选择下载目录")
                        .pick_folder()
                    {
                        app.config.download_path = path.to_string_lossy().into_owned();
                        changed = true;
                    }
                }
            });
            if changed {
                app.settings_dirty = true;
            }
            if combo_row(ui, "默认格式",
                Some("txt 编码见下一项"),
                "ext_name", 130.0,
                &mut app.config.ext_name, ExportFormat::as_lower, |ui, cur| {
                    ui.selectable_value(cur, ExportFormat::Epub, "epub");
                    ui.selectable_value(cur, ExportFormat::Txt, "txt");
                    ui.selectable_value(cur, ExportFormat::Html, "html");
                    ui.add_enabled(false, egui::Button::selectable(false, "pdf (阶段一不支持)"));
                }) {
                app.settings_dirty = true;
            }
            // TXT 编码：常见中文编码下拉。`txt_encoding` 是自由字符串，下拉里的
            // 选项都是 &'static str 字符串字面量。比对时大小写敏感（编码名大写
            // 是惯例），找不到匹配就显示"自定义"——既不强制覆盖用户手填的怪值，
            // 也让用户在下拉里能"恢复"到合法选项之一。
            separator(ui);
            const ENCODINGS: &[&str] = &[
                "UTF-8", "GBK", "GB18030", "Big5", "BIG5HKSCS", "UTF-16LE", "UTF-16BE",
            ];
            let current = app.config.txt_encoding.clone();
            let mut changed = false;
            settings::settings_row(ui, "TXT 编码",
                Some("TXT 导出的字符编码；如需兼容旧设备选 GBK"),
                |ui| {
                    let matched = ENCODINGS.iter().find(|e| **e == current).copied();
                    let selected = matched.unwrap_or("自定义");
                    input::rounded_combo(ui, "txt_encoding", selected, 130.0, input::ROW_HEIGHT, |ui| {
                        for enc in ENCODINGS {
                            let is_cur = *enc == current;
                            if ui.selectable_label(is_cur, *enc).clicked() {
                                app.config.txt_encoding = (*enc).to_string();
                                changed = true;
                            }
                        }
                        // 如果当前值不在预设列表中，"自定义"行高亮并可点击清空（不删字段）
                        let custom_current = matched.is_none();
                        if ui.selectable_label(custom_current, "自定义").clicked() {
                            // 用户重新选"自定义"——保留原值，仅作为显示，不动字段。
                        }
                    });
                },
            );
            if changed {
                app.settings_dirty = true;
            }
        });

        // ---- 书源 ----
        card(ui, "书源", |ui| {
            if combo_row(ui, "界面语言", None, "lang", 130.0,
                &mut app.config.language, LangType::as_str, |ui, cur| {
                    ui.selectable_value(cur, LangType::ZhCn, "zh_CN 简体");
                    ui.selectable_value(cur, LangType::ZhTw, "zh_TW 台湾");
                    ui.selectable_value(cur, LangType::ZhHant, "zh_Hant 通用繁体");
                }) {
                app.settings_dirty = true;
            }
            if drag_row_opt_i32(ui, "搜索条数上限",
                Some("每源最多返回条数；-1 表示不限"),
                -1, 10_000,
                &mut app.config.search_limit, -1) {
                app.settings_dirty = true;
            }
            if toggle_row(ui, "过滤低相似度并排序搜索结果", None,
                &mut app.config.search_filter) {
                app.settings_dirty = true;
            }
        });

        // ---- 抓取 ----
        card(ui, "抓取", |ui| {
            if drag_row_opt_i32(ui, "并发上限",
                Some("-1 = 自动：min(50, 章节数)"),
                -1, 100,
                &mut app.config.concurrency, -1) {
                app.settings_dirty = true;
            }
            if range_row(ui, "请求间隔 (ms)",
                Some("min ≤ max；抓每章前会等 [min..max] 内的随机 ms"),
                &mut app.config.min_interval, &mut app.config.max_interval) {
                app.settings_dirty = true;
            }
            if toggle_row(ui, "启用失败重试",
                Some("章节下载失败时按下方间隔重试，达到上限后视为放弃"),
                &mut app.config.enable_retry) {
                app.settings_dirty = true;
            }
            if drag_row_int(ui, "最大重试次数", None, 0u32, 20u32,
                &mut app.config.max_retries) {
                app.settings_dirty = true;
            }
            if range_row(ui, "重试间隔 (ms)",
                Some("min ≤ max；两次重试之间等 [min..max] 内的随机 ms"),
                &mut app.config.retry_min_interval, &mut app.config.retry_max_interval) {
                app.settings_dirty = true;
            }
        });

        // ---- 代理 ----
        card(ui, "代理", |ui| {
            if toggle_row(ui, "启用 HTTP 代理",
                Some("所有出站请求都走此代理（书源 fetch + 测速）"),
                &mut app.config.proxy_enabled) {
                app.settings_dirty = true;
            }
            if text_row(ui, "代理 Host", None,
                &mut app.config.proxy_host, 220.0, None) {
                app.settings_dirty = true;
            }
            if drag_row_int(ui, "代理 Port", None, 1u16, 65_535u16,
                &mut app.config.proxy_port) {
                app.settings_dirty = true;
            }
        });

        // ---- Cookie ----
        card(ui, "Cookie", |ui| {
            if text_row(ui, "起点 Cookie",
                Some("起点中文网 / 起点海外站点需要；其它书源可忽略"),
                &mut app.config.qidian_cookie, 360.0, Some("w_tsfp=...")) {
                app.settings_dirty = true;
            }
        });

        // ---- 网络 / 全局 ----
        card(ui, "网络", |ui| {
            if text_row(ui, "GitHub 代理",
                Some("用于加速 release / raw 资源下载；留空走默认"),
                &mut app.config.gh_proxy, 360.0, None) {
                app.settings_dirty = true;
            }
            if text_row(ui, "Cloudflare bypass URL",
                Some("本地或远端 sarperavci/CloudflareBypass 服务的 base URL"),
                &mut app.config.cf_bypass, 360.0, None) {
                app.settings_dirty = true;
            }
        });

        // ---- 关于 ----
        card(ui, "关于", |ui| {
            separator(ui);
            settings::settings_row(ui, "版本",
                Some("So Novel — Rust + egui 桌面客户端"),
                |ui| {
                    ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).strong());
                },
            );

            separator(ui);
            settings::settings_row(ui, "检查更新", None, |ui| {
                if app.update_state.checking {
                    ui.spinner();
                    ui.add_space(4.0);
                    ui.label("检查中…");
                } else {
                    // 有新版本时多一个"查看"按钮
                    if let Some(latest) = &app.update_state.latest_version {
                        let current = env!("CARGO_PKG_VERSION");
                        if latest.trim_start_matches('v') != current {
                            if button::inline(ui, "查看") {
                                ui.ctx().open_url(egui::OpenUrl::new_tab("https://github.com/AhJxs/so-novel-rs/releases/latest"));
                            }
                            ui.add_space(4.0);
                        }
                    }
                    if button::inline_icon(ui, "检查", mi::ICON_REFRESH) {
                        app.spawn_update_check();
                    }
                }
            });

            separator(ui);
            settings::settings_row(ui, "项目主页", None, |ui| {
                ui.hyperlink_to("GitHub", "https://github.com/AhJxs/so-novel-rs");
            });
        });
    });

    // 任何控件 changed 之后这里自动落盘
    app.persist_settings();
}

// ---------- 行布局 helpers ----------
//
// 这些是卡片内"一行的渲染"封装：左边标题 + 副标题，右边一个具体控件。
// 各 helper 内部统一处理 dirty 标记，调用方不用关心。

/// 一张设置卡片（iOS 风格）：圆角描边 frame + 标题 + 多个用分隔线隔开的行。
///
/// 卡片左上角标题字号 / 字重与行内主标题一致（14.5pt strong），用
/// `semantic_muted` 色降级为"分组标签"语义，不与行内主标题抢权重。
fn card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.add_space(2.0);
    egui::Frame::group(ui.style())
        .fill(ui.style().visuals.faint_bg_color)
        .stroke(egui::Stroke::new(1.0, ui.style().visuals.widgets.noninteractive.bg_stroke.color))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new(title)
                        .size(14.5)
                        .strong()
                        .color(ui.style().visuals.weak_text_color()),
                );
            });
            ui.add_space(6.0);
            body(ui);
        });
    ui.add_space(10.0);
}

/// 行顶分隔线（卡片内多行间用）。
fn separator(ui: &mut Ui) {
    ui.add(egui::Separator::default().spacing(0.0).horizontal());
}

/// iOS 风格 toggle 行：右控件是 toggle_switch。
///
/// helper 只管布局 + 控件，**不**接管 dirty 标记 —— Rust 借检查不允许 helper
/// 内部同时持有 `&mut SoNovelApp` 和 `&mut app.config.field`。
/// 调用方拿返回的 `changed()` 自己设 `app.settings_dirty = true`。
fn toggle_row(
    ui: &mut Ui,
    title: &str,
    subtitle: Option<&str>,
    on: &mut bool,
) -> bool {
    separator(ui);
    let mut changed = false;
    settings::settings_row(ui, title, subtitle, |ui| {
        changed = toggle::toggle_switch(ui, on).changed();
    });
    changed
}

/// TextEdit 行（单行字符串，圆角边框样式）。返回 `changed()`。
fn text_row(
    ui: &mut Ui,
    title: &str,
    subtitle: Option<&str>,
    target: &mut String,
    width: f32,
    hint: Option<&str>,
) -> bool {
    separator(ui);
    let mut changed = false;
    settings::settings_row(ui, title, subtitle, |ui| {
        changed = input::rounded_text_input(ui, target, width, input::ROW_HEIGHT, hint).changed();
    });
    changed
}

/// ComboBox 行。返回 `*target` 旧值比对结果（combo 改动由闭包内 `selectable_value`
/// 直接写 target，egui 不会自动设 changed()）。
#[allow(clippy::too_many_arguments)]
fn combo_row<T: Copy + PartialEq>(
    ui: &mut Ui,
    title: &str,
    subtitle: Option<&str>,
    id_salt: &str,
    width: f32,
    target: &mut T,
    value_to_str: impl Fn(T) -> &'static str,
    fill_options: impl FnOnce(&mut Ui, &mut T),
) -> bool {
    let before = *target;
    separator(ui);
    settings::settings_row(ui, title, subtitle, |ui| {
        let selected = value_to_str(*target).to_string();
        input::rounded_combo(ui, id_salt, selected, width, input::ROW_HEIGHT, |ui| {
            fill_options(ui, target);
        });
    });
    *target != before
}

/// DragValue 行（Option<i32>，None 时回退到 -1 等 sentinel，圆角边框样式）。
fn drag_row_opt_i32(
    ui: &mut Ui,
    title: &str,
    subtitle: Option<&str>,
    lo: i32,
    hi: i32,
    target: &mut Option<i32>,
    none_sentinel: i32,
) -> bool {
    let before = *target;
    separator(ui);
    let mut changed = false;
    settings::settings_row(ui, title, subtitle, |ui| {
        let mut v = target.unwrap_or(none_sentinel);
        if input::rounded_drag_value(ui, &mut v, lo..=hi, 80.0, input::ROW_HEIGHT).changed() {
            *target = if v == none_sentinel { None } else { Some(v) };
            changed = true;
        }
    });
    changed || *target != before
}

/// DragValue 行（任意整数类型 `T: egui::emath::Numeric`，圆角边框样式）。
fn drag_row_int<T: egui::emath::Numeric>(
    ui: &mut Ui,
    title: &str,
    subtitle: Option<&str>,
    lo: T,
    hi: T,
    target: &mut T,
) -> bool {
    separator(ui);
    let mut changed = false;
    settings::settings_row(ui, title, subtitle, |ui| {
        if input::rounded_drag_value(ui, target, lo..=hi, 80.0, input::ROW_HEIGHT).changed() {
            changed = true;
        }
    });
    changed
}

/// "min / max" 双 DragValue 行（抓取间隔、重试间隔共用，圆角边框样式）。
fn range_row(
    ui: &mut Ui,
    title: &str,
    subtitle: Option<&str>,
    min: &mut u32,
    max: &mut u32,
) -> bool {
    separator(ui);
    let mut changed = false;
    settings::settings_row(ui, title, subtitle, |ui| {
        ui.horizontal_centered(|ui| {
            ui.label("min");
            let r1 = input::rounded_drag_value(ui, min, 0..=60_000, 70.0, input::ROW_HEIGHT);
            ui.label("max");
            let r2 = input::rounded_drag_value(ui, max, 0..=60_000, 70.0, input::ROW_HEIGHT);
            if r1.changed() || r2.changed() {
                changed = true;
            }
        });
    });
    changed
}
