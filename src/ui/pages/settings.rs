//! 设置页。这是阶段 1 唯一**可真实写回 config.ini** 的页面。

use crate::app::SoNovelApp;
use crate::config::{save_config, ExportFormat, LangType};
use crate::ui::theme;

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    ui.heading("设置");
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(format!(
            "修改后点击『保存』将写回: {}",
            app.paths.config_file.display()
        ))
        .small()
        .weak(),
    );
    ui.add_space(8.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        section_download(ui, app);
        ui.add_space(8.0);
        section_source(ui, app);
        ui.add_space(8.0);
        section_crawl(ui, app);
        ui.add_space(8.0);
        section_proxy(ui, app);
        ui.add_space(8.0);
        section_cookie(ui, app);
        ui.add_space(8.0);
        section_global(ui, app);
        ui.add_space(8.0);
        section_web(ui, app);
        ui.add_space(16.0);

        ui.horizontal(|ui| {
            if theme::button(ui, "保存到 config.ini").clicked() {
                match save_config(&app.paths.config_file, &app.draft_config) {
                    Ok(_) => {
                        app.config = app.draft_config.clone();
                        app.show_toast("已保存到 config.ini");
                    }
                    Err(e) => {
                        app.show_toast(format!("保存失败: {e}"));
                    }
                }
            }
            if theme::button(ui, "重置为已保存值").clicked() {
                app.draft_config = app.config.clone();
                app.show_toast("已重置");
            }
        });
    });
}

fn section_download(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.strong("[download]");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("下载目录:");
                ui.add(
                    egui::TextEdit::singleline(&mut app.draft_config.download_path)
                        .desired_width(360.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label("默认格式:");
                let mut fmt = app.draft_config.ext_name;
                egui::ComboBox::from_id_salt("ext_name")
                    .selected_text(fmt.as_lower())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut fmt, ExportFormat::Epub, "epub");
                        ui.selectable_value(&mut fmt, ExportFormat::Txt, "txt");
                        ui.selectable_value(&mut fmt, ExportFormat::Html, "html");
                        ui.add_enabled(
                            false,
                            egui::Button::selectable(false, "pdf (阶段一不支持)"),
                        );
                    });
                app.draft_config.ext_name = fmt;
            });
            ui.horizontal(|ui| {
                ui.label("TXT 编码:");
                ui.add(
                    egui::TextEdit::singleline(&mut app.draft_config.txt_encoding)
                        .desired_width(120.0),
                );
                ui.label(
                    egui::RichText::new("（如需兼容旧设备可填 GBK）")
                        .small()
                        .weak(),
                );
            });
            ui.checkbox(
                &mut app.draft_config.preserve_chapter_cache,
                "保留章节缓存目录",
            );
            ui.checkbox(&mut app.draft_config.enable_progressbar, "启用下载进度条");
        });
}

fn section_source(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.strong("[source]");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("语言:");
                let mut lang = app.draft_config.language;
                egui::ComboBox::from_id_salt("lang")
                    .selected_text(lang.as_str())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut lang, LangType::ZhCn, "zh_CN");
                        ui.selectable_value(&mut lang, LangType::ZhTw, "zh_TW");
                        ui.selectable_value(&mut lang, LangType::ZhHant, "zh_Hant");
                    });
                app.draft_config.language = lang;
            });

            ui.horizontal(|ui| {
                ui.label("激活规则:");
                ui.add(
                    egui::TextEdit::singleline(&mut app.draft_config.active_rules)
                        .desired_width(220.0),
                );
                ui.label(
                    egui::RichText::new("文件名（如 main.json）或绝对路径")
                        .small()
                        .weak(),
                );
            });

            ui.horizontal(|ui| {
                ui.label("搜索条数上限:");
                let mut v = app.draft_config.search_limit.unwrap_or(-1);
                ui.add(egui::DragValue::new(&mut v).range(-1..=10000));
                app.draft_config.search_limit = if v < 0 { None } else { Some(v) };
                ui.label(egui::RichText::new("(-1 表示不限)").small().weak());
            });

            ui.checkbox(
                &mut app.draft_config.search_filter,
                "过滤低相似度并排序搜索结果",
            );
        });
}

fn section_crawl(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.strong("[crawl]");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("并发上限:");
                let mut c = app.draft_config.concurrency.unwrap_or(-1);
                ui.add(egui::DragValue::new(&mut c).range(-1..=100));
                app.draft_config.concurrency = if c < 0 { None } else { Some(c) };
                ui.label(
                    egui::RichText::new("(-1 = 自动: min(50, 章节数))")
                        .small()
                        .weak(),
                );
            });

            ui.horizontal(|ui| {
                ui.label("最小间隔(ms):");
                ui.add(egui::DragValue::new(&mut app.draft_config.min_interval).range(0..=60_000));
                ui.label("最大间隔(ms):");
                ui.add(egui::DragValue::new(&mut app.draft_config.max_interval).range(0..=60_000));
            });

            ui.checkbox(&mut app.draft_config.enable_retry, "启用失败重试");

            ui.horizontal(|ui| {
                ui.label("最大重试次数:");
                ui.add(egui::DragValue::new(&mut app.draft_config.max_retries).range(0..=20));
            });
            ui.horizontal(|ui| {
                ui.label("重试最小间隔(ms):");
                ui.add(
                    egui::DragValue::new(&mut app.draft_config.retry_min_interval)
                        .range(0..=60_000),
                );
                ui.label("重试最大间隔(ms):");
                ui.add(
                    egui::DragValue::new(&mut app.draft_config.retry_max_interval)
                        .range(0..=60_000),
                );
            });
        });
}

fn section_proxy(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.strong("[proxy]");
            ui.add_space(4.0);
            ui.checkbox(&mut app.draft_config.proxy_enabled, "启用 HTTP 代理");
            ui.horizontal(|ui| {
                ui.label("Host:");
                ui.add(
                    egui::TextEdit::singleline(&mut app.draft_config.proxy_host)
                        .desired_width(220.0),
                );
                ui.label("Port:");
                ui.add(egui::DragValue::new(&mut app.draft_config.proxy_port).range(1..=65535));
            });
        });
}

fn section_cookie(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.strong("[cookie]");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("起点 cookie:");
                ui.add(
                    egui::TextEdit::singleline(&mut app.draft_config.qidian_cookie)
                        .desired_width(420.0)
                        .hint_text("w_tsfp=..."),
                );
            });
        });
}

fn section_global(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.strong("[global]");
            ui.add_space(4.0);
            ui.checkbox(&mut app.draft_config.auto_update, "启动时检查更新");
            ui.horizontal(|ui| {
                ui.label("GitHub 代理:");
                ui.add(
                    egui::TextEdit::singleline(&mut app.draft_config.gh_proxy).desired_width(360.0),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Cloudflare bypass URL:");
                ui.add(
                    egui::TextEdit::singleline(&mut app.draft_config.cf_bypass)
                        .desired_width(360.0),
                );
            });
        });
}

fn section_web(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.strong("[web]（旧 Java WebUI 兼容字段，Rust 版不启用 web 服务）");
            ui.add_space(4.0);
            ui.checkbox(&mut app.draft_config.web_enabled, "Java 版 WebUI 启用标志");
            ui.horizontal(|ui| {
                ui.label("Port:");
                ui.add(egui::DragValue::new(&mut app.draft_config.web_port).range(1..=65535));
            });
        });
}
