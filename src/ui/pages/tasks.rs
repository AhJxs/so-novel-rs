//! 下载任务页。阶段 4b 打磨：
//! - 完成任务的"打开文件" / "打开所在目录"按钮；
//! - 失败 / 取消任务的"重新下载"按钮；
//! - 顶部"清除已完成"按钮。

use std::path::PathBuf;

use crate::app::{DownloadTask, SoNovelApp};
use crate::models::SearchResult;
use crate::ui::theme;

use crate::util::system::{open_path, reveal_in_folder};

pub fn show(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    ui.heading("下载任务");
    ui.add_space(4.0);
    show_summary_bar(ui, app);
    ui.add_space(8.0);

    if app.tasks.is_empty() {
        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::same(12))
            .show(ui, |ui| {
                ui.label("暂无下载任务。去 [搜索下载] 选一本书试试。");
            });
        return;
    }

    // 收集子动作（避免渲染循环里多重借 app）
    let mut to_open: Option<PathBuf> = None;
    let mut to_reveal: Option<PathBuf> = None;
    let mut to_redownload: Option<Box<SearchResult>> = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        let len = app.tasks.len();
        // 倒序：最新任务在最上面
        for i in (0..len).rev() {
            let action = show_one_task(ui, &mut app.tasks[i]);
            match action {
                TaskAction::None => {}
                TaskAction::Open(p) => to_open = Some(p),
                TaskAction::Reveal(p) => to_reveal = Some(p),
                TaskAction::Redownload(r) => to_redownload = Some(r),
            }
            ui.add_space(6.0);
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

fn show_summary_bar(ui: &mut egui::Ui, app: &mut SoNovelApp) {
    let total = app.tasks.len();
    let running = app.tasks.iter().filter(|t| t.is_running()).count();
    let done = app
        .tasks
        .iter()
        .filter(|t| matches!(&t.finished, Some(Ok(_))))
        .count();
    let failed = app
        .tasks
        .iter()
        .filter(|t| matches!(&t.finished, Some(Err(_))))
        .count();

    ui.horizontal(|ui| {
        let dark = ui.style().visuals.dark_mode;
        ui.label(format!("共 {total} 个任务"));
        if running > 0 {
            ui.colored_label(theme::semantic_info(dark), format!("⏳ 进行中 {running}"));
        }
        if done > 0 {
            ui.colored_label(theme::semantic_success(dark), format!("✓ 完成 {done}"));
        }
        if failed > 0 {
            ui.colored_label(theme::semantic_warn(dark), format!("⚠ 失败/取消 {failed}"));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let any_finished = done + failed > 0;
            if ui
                .add_enabled(any_finished, egui::Button::new("清除已完成"))
                .clicked()
            {
                app.tasks.retain(|t| t.is_running());
            }
        });
    });
}

enum TaskAction {
    None,
    Open(PathBuf),
    Reveal(PathBuf),
    /// SearchResult 较大（含若干 Option<String>）；Box 起来避免大 enum 警告。
    Redownload(Box<SearchResult>),
}

fn show_one_task(ui: &mut egui::Ui, task: &mut DownloadTask) -> TaskAction {
    let mut action = TaskAction::None;

    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(format!("#{} {}", task.id, task.book_name()));
                let author = task
                    .book_meta
                    .as_ref()
                    .map(|b| b.author.as_str())
                    .or(task.origin.author.as_deref())
                    .unwrap_or("-");
                ui.label(format!("作者：{author}"));
                ui.separator();
                ui.label(format!(
                    "来源：{}#{}",
                    task.origin.source_name, task.origin.source_id
                ));

                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| match &task.finished {
                        Some(Ok(p)) => {
                            ui.colored_label(egui::Color32::from_rgb(80, 170, 110), "✓ 完成");
                            if theme::small_button(ui, "位置").clicked() {
                                action = TaskAction::Reveal(p.clone());
                            }
                            if theme::small_button(ui, "打开").clicked() {
                                action = TaskAction::Open(p.clone());
                            }
                        }
                        Some(Err(reason)) => {
                            ui.colored_label(
                                theme::semantic_warn(ui.style().visuals.dark_mode),
                                format!("⚠ {reason}"),
                            );
                            if theme::small_button(ui, "重试").clicked() {
                                action = TaskAction::Redownload(Box::new(task.origin.clone()));
                            }
                        }
                        None => {
                            if theme::button(ui, "取消").clicked() {
                                task.cancel.cancel();
                            }
                        }
                    },
                );
            });

            ui.add_space(4.0);
            // 进度条
            let total = task.total_chapters.max(1);
            let progress = task.completed as f32 / total as f32;
            let progress_label = if task.total_chapters == 0 && task.is_running() {
                "解析详情中…".to_string()
            } else {
                format!(
                    "{} / {} 章（失败 {}）",
                    task.completed, task.total_chapters, task.failed
                )
            };
            ui.add(
                egui::ProgressBar::new(progress.min(1.0))
                    .text(progress_label)
                    .desired_width(ui.available_width()),
            );
            if !task.last_chapter_title.is_empty() {
                ui.label(
                    egui::RichText::new(format!("最近章节：{}", task.last_chapter_title))
                        .small()
                        .weak(),
                );
            }
            // 完成时显示输出路径
            if let Some(Ok(p)) = &task.finished {
                ui.label(
                    egui::RichText::new(format!("输出: {}", p.display()))
                        .small()
                        .weak(),
                );
            }

            // 失败章节明细（折叠）
            if !task.failures.is_empty() {
                ui.add_space(4.0);
                let header = format!("失败章节（{}）", task.failures.len());
                egui::CollapsingHeader::new(header)
                    .id_salt(("task_failures", task.id))
                    .show(ui, |ui| {
                        for (idx, title, reason) in &task.failures {
                            ui.label(format!("第 {idx} 章 《{title}》 — {reason}"));
                        }
                    });
            }
        });

    action
}
