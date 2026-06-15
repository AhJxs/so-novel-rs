//! 本地书库页。阶段 4b：扫描 `download_path` 列出已生成的电子书文件，
//! 提供搜索过滤、打开文件、显示位置、删除（带二次确认）。
//!
//! 不递归子目录 — `download_path` 根下放合并产物，章节缓存目录是另一层
//! `<书名>(<作者>) EXT/`，按用途清晰分开。

use std::path::PathBuf;

use crate::app::{LibraryEntry, SoNovelApp};
use crate::design_system::{button, chip, color, input};
use crate::util::system::{open_path, reveal_in_folder};
use crate::util::time::format_unix_local_u64;
use crate::material_icons::icons as mi;

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

    show_toolbar(ui, app);
    ui.add_space(8.0);

    if let Some(err) = &app.library.last_error {
        ui.colored_label(
            color::semantic_danger(ui.style().visuals.dark_mode),
            format!("⚠ {err}"),
        );
        ui.add_space(4.0);
    }

    show_table(ui, app);
}

fn show_toolbar(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    ui.horizontal(|ui| {
        // 1. 文件名过滤输入框（与搜索页关键词框同款）
        const INPUT_W: f32 = 280.0;
        let (_resp, _enter) = input::icon_text_input(
            ui,
            &mut app.library.filter_text,
            "按文件名过滤",
            mi::ICON_FILTER,
            INPUT_W,
            input::INPUT_HEIGHT,
        );

        ui.add_space(6.0);

        // 2. 格式过滤下拉（与搜索页书源下拉同款）
        let mut current = app
            .library
            .filter_ext
            .clone()
            .unwrap_or_else(|| "全部".to_string());
        input::rounded_combo(ui, "library_ext_filter", current.clone(), 110.0, input::INPUT_HEIGHT, |ui| {
            for opt in ["全部", "epub", "txt", "zip", "html", "pdf"] {
                ui.selectable_value(&mut current, opt.to_string(), opt);
            }
        });
        app.library.filter_ext = if current == "全部" {
            None
        } else {
            Some(current)
        };

        ui.add_space(6.0);

        // 3. 刷新按钮（亮蓝主按钮，与搜索按钮同款）
        let refresh_label = format!("{} 刷新", mi::ICON_REFRESH.codepoint);
        if button::primary_button(ui, &refresh_label, true) {
            app.refresh_library();
        }
    });
}

fn show_table(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    if app.library.entries.is_empty() {
        // 还没扫到任何书：与下载任务页空态同款（图标 + 主副文案）。
        chip::empty_state(
            ui,
            mi::ICON_LIBRARY_BOOKS,
            "本地书库为空",
            "下载完成的书会显示在这里",
        );
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

    // 过滤后无结果：同款空态，引导清空过滤条件。
    if visible.is_empty() {
        chip::empty_state(
            ui,
            mi::ICON_SEARCH_OFF,
            "没有匹配的书",
            "试试清空关键词或换一种格式",
        );
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("library_list")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            // 卡片宽度在循环外算一次（避免 ScrollArea 跨帧反馈造成"逐张变宽"），
            // 与 search 页 result_card 同样的处理。
            let card_width = ui.available_width();
            for (idx, e) in visible.iter().enumerate() {
                let is_pending = pending_delete.as_ref() == Some(&e.path);
                let action = entry_card(ui, idx, e, is_pending, card_width);
                match action {
                    EntryAction::None => {}
                    EntryAction::Open => to_open = Some(e.path.clone()),
                    EntryAction::Reveal => to_reveal = Some(e.path.clone()),
                    EntryAction::ConfirmDelete => to_confirm_delete = Some(e.path.clone()),
                    EntryAction::Delete => to_delete = Some(e.path.clone()),
                    EntryAction::CancelDelete => cancel_pending_delete = true,
                }
                ui.add_space(6.0);
            }
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

/// 卡片点击产生的动作（统一收尾在 show_table 末尾），避免循环里多重借 app。
#[derive(Debug, Clone, Copy)]
enum EntryAction {
    None,
    Open,
    Reveal,
    ConfirmDelete,
    Delete,
    CancelDelete,
}

/// 单本书卡片。视觉与「搜索下载」页 result_card 一致：
/// - 行高 32px、圆角 8px、淡描边、hover 浅底
/// - 左侧：序号 + 书名（强 label）+ · + 格式 + · + 大小 + · + 修改时间
/// - 右侧：打开 / 位置 / 删除（pending 时切换为「确认删除 / 取消」）
fn entry_card(
    ui: &mut egui::Ui,
    idx: usize,
    e: &LibraryEntry,
    pending_delete: bool,
    card_width: f32,
) -> EntryAction {
    let visuals = ui.style().visuals.clone();
    let dark_mode = visuals.dark_mode;

    let hover_fill = if dark_mode {
        egui::Color32::from_white_alpha(10)
    } else {
        egui::Color32::from_black_alpha(8)
    };

    let card_inner_width = (card_width - 28.0).max(0.0);

    // hover 反馈：与 search 页同款 — 借 memory 记上一帧，1 帧延迟人感不到。
    let card_id = egui::Id::new(("library_card", idx));
    let was_hovered = ui
        .ctx()
        .memory(|m| m.data.get_temp::<bool>(card_id).unwrap_or(false));

    let frame_fill = if was_hovered {
        hover_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    let frame_stroke = egui::Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color);

    let mut action = EntryAction::None;

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

                        // 两行布局：
                        // - 第一行：序号 + 书名（强）           + 按钮组（贴右）
                        // - 第二行：           大小 · 时间       （缩进对齐到书名起点，弱小字）
                        //
                        // 单行版本里"大小 / 时间"夹在书名右边，每行因书名长度不同
                        // 而起始 x 飘移，视觉上很乱。挪到第二行后所有卡片元数据
                        // 起点一致，自然对齐。
                        //
                        // 整体 horizontal：左 vertical（两行内容）+ 右 rtl 按钮组。
                        // rtl 默认 Align::Center，按钮垂直居中于左侧 vertical 整体高度。
                        ui.horizontal(|ui| {
                            // ---- 左：书名行 + 元数据行 ----
                            ui.vertical(|ui| {
                                // 第一行：序号 + 书名
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(format!("#{}", idx + 1)));
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(truncate(&e.file_name, 50))
                                            .strong()
                                            .size(14.5),
                                    );
                                });

                                ui.add_space(2.0);

                                // 第二行：大小 · 时间。缩进 24px 让起点与书名大致对齐
                                // （序号 #N 宽度 + space(8) 视觉等价）。所有卡片同样的
                                // 缩进 → 视觉对齐成一列。
                                ui.horizontal(|ui| {
                                    ui.add_space(24.0);
                                    ui.label(
                                        egui::RichText::new(format_unix_local_u64(
                                            e.modified_unix_secs,
                                        ))
                                        .small()
                                        .weak(),
                                    );
                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new("·").small().weak());
                                    ui.add_space(8.0);
                                    ui.label(
                                        egui::RichText::new(format_size(e.size_bytes))
                                            .small()
                                            .weak(),
                                    );
                                });
                            });

                            // ---- 右：按钮组（垂直居中于整张卡片）----
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if pending_delete {
                                        // 二次确认：警示色"确认删除"+ 普通"取消"
                                        // rtl 下添加顺序：先取消 → 再确认删除 → 视觉上"确认删除"在最右
                                        if button::inline_danger_icon(ui, "确认删除", mi::ICON_DELETE_FOREVER) {
                                            action = EntryAction::Delete;
                                        }
                                        ui.add_space(6.0);
                                        if button::inline_icon(ui, "取消", mi::ICON_CANCEL) {
                                            action = EntryAction::CancelDelete;
                                        }
                                    } else {
                                        if button::inline_danger_icon(ui, "删除", mi::ICON_DELETE) {
                                            action = EntryAction::ConfirmDelete;
                                        }
                                        ui.add_space(6.0);
                                        if button::inline_icon(ui, "位置", mi::ICON_FOLDER_OPEN) {
                                            action = EntryAction::Reveal;
                                        }
                                        ui.add_space(6.0);
                                        if button::inline_icon(ui, "打开", mi::ICON_OPEN_IN_NEW) {
                                            action = EntryAction::Open;
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
}
