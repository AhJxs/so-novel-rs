//! 本地书库页。阶段 4b：扫描 `download_path` 列出已生成的电子书文件，
//! 提供搜索过滤、打开文件、显示位置、删除（带二次确认）。
//!
//! 不递归子目录 — `download_path` 根下放合并产物，章节缓存目录是另一层
//! `<书名>(<作者>) EXT/`，按用途清晰分开。

use std::path::PathBuf;

use crate::app::{LibraryEntry, SoNovelApp};
use crate::ui::theme;
use crate::util::system::{open_path, reveal_in_folder};

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    // 首次进入或下载目录变化时自动扫描。
    let need_initial = app.library.scanned_dir.is_none()
        || app
            .library
            .scanned_dir
            .as_deref()
            .map(|d| d.as_os_str() != app.config.download_path.as_str())
            .unwrap_or(false);
    if need_initial {
        app.refresh_library();
    }

    ui.heading("本地书库");
    ui.add_space(4.0);
    show_toolbar(ui, app);
    ui.add_space(8.0);

    if let Some(err) = &app.library.last_error {
        ui.colored_label(
            theme::semantic_danger(ui.style().visuals.dark_mode),
            format!("⚠ {err}"),
        );
        ui.add_space(4.0);
    }

    show_table(ui, app);
}

fn show_toolbar(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    let dir_display = app
        .library
        .scanned_dir
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| app.config.download_path.clone());

    ui.horizontal(|ui| {
        ui.label("下载目录:");
        ui.add(
            egui::Label::new(
                egui::RichText::new(&dir_display)
                    .small()
                    .color(theme::semantic_info(ui.style().visuals.dark_mode)),
            )
            .truncate(),
        );
    });

    ui.add_space(4.0);

    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(&mut app.library.filter_text)
            .hint_text("按文件名过滤")
            .desired_width(280.0);
        ui.add(edit);

        // 格式过滤
        let mut current = app
            .library
            .filter_ext
            .clone()
            .unwrap_or_else(|| "全部".to_string());
        egui::ComboBox::from_id_salt("library_ext_filter")
            .selected_text(&current)
            .width(110.0)
            .show_ui(ui, |ui| {
                for opt in ["全部", "epub", "txt", "zip", "html", "pdf"] {
                    ui.selectable_value(&mut current, opt.to_string(), opt);
                }
            });
        app.library.filter_ext = if current == "全部" {
            None
        } else {
            Some(current)
        };

        if theme::button(ui, "🔄 刷新").clicked() {
            app.refresh_library();
        }

        if theme::button(ui, "打开下载目录").clicked() {
            if let Some(dir) = &app.library.scanned_dir {
                if let Err(e) = open_path(dir) {
                    app.library.last_error = Some(format!("打开目录失败: {e}"));
                }
            }
        }
    });
}

fn show_table(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    if app.library.entries.is_empty() {
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.label("还没有下载完成的书。去 [搜索下载] 试试。");
            });
        return;
    }

    let filter_text = app.library.filter_text.trim().to_lowercase();
    let filter_ext = app.library.filter_ext.clone();

    // 收集要执行的动作（避免在循环中可变借 app）
    let mut to_open: Option<PathBuf> = None;
    let mut to_reveal: Option<PathBuf> = None;
    let mut to_delete: Option<PathBuf> = None;
    let mut to_confirm_delete: Option<PathBuf> = None;
    let mut cancel_pending_delete = false;

    let pending_delete = app.library.pending_delete.clone();

    let visible: Vec<&LibraryEntry> = app
        .library
        .entries
        .iter()
        .filter(|e| {
            if let Some(ext) = &filter_ext {
                if !e.ext.eq_ignore_ascii_case(ext) {
                    return false;
                }
            }
            if !filter_text.is_empty() && !e.file_name.to_lowercase().contains(&filter_text) {
                return false;
            }
            true
        })
        .collect();

    ui.label(format!(
        "共 {} 本（过滤后 {} 本）",
        app.library.entries.len(),
        visible.len()
    ));
    ui.add_space(4.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("library_grid")
            .striped(true)
            .min_col_width(60.0)
            .show(ui, |ui| {
                ui.strong("文件");
                ui.strong("格式");
                ui.strong("大小");
                ui.strong("修改时间");
                ui.strong("操作");
                ui.end_row();

                for e in &visible {
                    ui.label(truncate(&e.file_name, 50));
                    ui.label(&e.ext);
                    ui.label(format_size(e.size_bytes));
                    ui.label(format_unix_time(e.modified_unix_secs));

                    ui.horizontal(|ui| {
                        if theme::small_button(ui, "打开").clicked() {
                            to_open = Some(e.path.clone());
                        }
                        if theme::small_button(ui, "位置").clicked() {
                            to_reveal = Some(e.path.clone());
                        }

                        if pending_delete.as_ref() == Some(&e.path) {
                            // 二次确认：警示色"确认删除"+ 普通"取消"
                            let resp = ui.add(
                                egui::Button::new(
                                    egui::RichText::new("确认删除").color(egui::Color32::WHITE),
                                )
                                .fill(theme::semantic_danger(ui.style().visuals.dark_mode)),
                            );
                            if resp.clicked() {
                                to_delete = Some(e.path.clone());
                            }
                            if theme::small_button(ui, "取消").clicked() {
                                cancel_pending_delete = true;
                            }
                        } else if theme::small_button(ui, "删除").clicked() {
                            to_confirm_delete = Some(e.path.clone());
                        }
                    });
                    ui.end_row();
                }
            });
    });

    // 收尾动作
    if let Some(p) = to_open {
        if let Err(e) = open_path(&p) {
            app.library.last_error = Some(format!("打开失败: {e}"));
        }
    }
    if let Some(p) = to_reveal {
        if let Err(e) = reveal_in_folder(&p) {
            app.library.last_error = Some(format!("显示位置失败: {e}"));
        }
    }
    if let Some(p) = to_confirm_delete {
        app.library.pending_delete = Some(p);
    }
    if cancel_pending_delete {
        app.library.pending_delete = None;
    }
    if let Some(p) = to_delete {
        app.delete_library_entry(&p);
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

/// 格式化字节数 → "1.23 MB" 风格。
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// 把 Unix 时间戳格式化为 `YYYY-MM-DD HH:MM`（本地时间近似）。
/// **故意不引 chrono / time crate**：本展示精度到分钟即可，不需要时区数据库。
/// 这里用一个简化的"日历"算法（适用 1970..2099 范围足够）。
fn format_unix_time(secs: u64) -> String {
    if secs == 0 {
        return "-".to_string();
    }
    // 把 UTC 偏移成本地：std 没有现成 API，这里直接显示 UTC 时间并标注。
    // 用户主要靠这个排序与"什么时候下的"概念，分钟级 UTC 已够用。
    let (y, m, d, hh, mm) = unix_to_ymdhm(secs);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC")
}

/// 把秒级 Unix 时间戳分解为 (年, 月, 日, 时, 分)。基于 1970-01-01 起算。
/// 闰年规则覆盖 1900-2100 完全准确。
fn unix_to_ymdhm(secs: u64) -> (u32, u32, u32, u32, u32) {
    let mut s = secs;
    let mm = ((s / 60) % 60) as u32;
    let hh = ((s / 3600) % 24) as u32;
    let mut days = s / 86_400;
    s = days; // re-purpose

    // 自 1970-01-01（周四）起按年累加
    let mut year: u32 = 1970;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if days >= dy {
            days -= dy;
            year += 1;
        } else {
            break;
        }
    }
    let _ = s; // silence unused
    let mdays = month_days(year);
    let mut month = 1u32;
    let mut d = days as u32;
    for (i, dm) in mdays.iter().enumerate() {
        if d < *dm {
            month = (i + 1) as u32;
            break;
        }
        d -= dm;
    }
    let day = d + 1;
    (year, month, day, hh, mm)
}

fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

fn month_days(y: u32) -> [u32; 12] {
    [
        31,
        if is_leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.00 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn unix_to_ymdhm_known_values() {
        // 1970-01-01 00:00:00 UTC = 0
        let (y, m, d, hh, mm) = unix_to_ymdhm(0);
        assert_eq!((y, m, d, hh, mm), (1970, 1, 1, 0, 0));
        // 2026-01-01 00:00:00 UTC（已知 56 年含 14 个闰年）
        let (y, m, d, hh, mm) = unix_to_ymdhm(1_767_225_600);
        assert_eq!((y, m, d, hh, mm), (2026, 1, 1, 0, 0));
        // 2024-02-29 12:34:00 UTC（闰年验证）
        let (y, m, d, hh, mm) = unix_to_ymdhm(1_709_210_040);
        assert_eq!((y, m, d, hh, mm), (2024, 2, 29, 12, 34));
    }

    #[test]
    fn format_unix_time_zero_renders_dash() {
        assert_eq!(format_unix_time(0), "-");
    }

    #[test]
    fn format_unix_time_basic() {
        // 用上面验证过的 2026-01-01 时间戳
        let s = format_unix_time(1_767_225_600);
        assert!(s.starts_with("2026-01-01"), "got {s}");
        assert!(s.ends_with("UTC"));
    }
}
