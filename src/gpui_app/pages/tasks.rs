//! Tasks 页面：下载任务管理（进度 / 取消 / 重试 / 打开 / 位置 / 清除已完成）。
//!
//! 行为对应旧 `crate::ui::pages::tasks::show`：
//! - 顶部 summary：总数 / 运行中 / 已完成 / 失败。
//! - 列表：每条任务卡片，含书名 / 来源 / 进度条 / 状态徽章 / 动作按钮。
//! - 失败章节明细用 `Accordion` 折叠。
//! - 完成任务的"打开文件" / "打开所在目录"按钮。
//! - 失败 / 取消任务的"重新下载"按钮。
//! - 顶部"清除已完成"按钮。

use gpui::{
    div, prelude::FluentBuilder as _, App, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, Styled, Window,
};
use gpui_component::{
    accordion::Accordion,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
    progress::Progress,
    ActiveTheme as _, Disableable, Icon, IconName, Sizable,
};

use crate::app::AppModel;
use crate::models::{Book, SearchResult};
use std::path::PathBuf;

/// `DownloadTask` 含 `mpsc::Receiver` / `CancelToken` 不可 Clone。
/// UI 渲染时复制必要字段为 `TaskSummary`，避开 Clone 限制。
#[allow(dead_code)]
struct TaskSummary {
    id: u64,
    origin: SearchResult,
    started_at_unix: i64,
    finished_at_unix: Option<i64>,
    book_meta: Option<Book>,
    total_chapters: usize,
    completed: u32,
    failed: u32,
    last_chapter_title: String,
    finished: Option<Result<PathBuf, String>>,
    failures: Vec<(u32, String, String)>,
    cancelling: bool,
}

impl TaskSummary {
    fn is_running(&self) -> bool {
        self.finished.is_none()
    }
    fn is_failed(&self) -> bool {
        matches!(self.finished.as_ref(), Some(Err(_))) && !self.is_cancelled()
    }
    fn is_cancelled(&self) -> bool {
        matches!(
            self.finished.as_ref(),
            Some(Err(reason)) if reason == "用户已取消" || reason == "应用重启时中断"
        )
    }
    fn book_name(&self) -> &str {
        self.book_meta
            .as_ref()
            .map(|b| b.book_name.as_str())
            .unwrap_or(self.origin.book_name.as_str())
    }
}
use crate::gpui_app::components::{
    truncate, EmptyState, PageHeader, StatusBadge, StatusKind,
};
use crate::util::system::{open_path, reveal_in_folder};

/// Tasks 页面 entity。
pub struct TasksPage {
    model: Entity<AppModel>,
}

impl TasksPage {
    pub fn new(model: Entity<AppModel>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { model }
    }

    fn cancel(&mut self, task_id: u64, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| {
            if let Some(t) = m.tasks.iter_mut().find(|t| t.id == task_id) {
                if let Some(cancel) = t.cancel.take() {
                    cancel.cancel();
                    t.cancelling = true;
                }
            }
        });
        cx.notify();
    }

    fn retry(&mut self, task_id: u64, cx: &mut Context<Self>) {
        // 重新下载 = 重新派一个新任务（保留原始 SearchResult）。
        let origin = self
            .model
            .read(cx)
            .tasks
            .iter()
            .find(|t| t.id == task_id)
            .map(|t| t.origin.clone());
        if let Some(origin) = origin {
            self.model.update(cx, |m, _cx| {
                m.spawn_download(origin);
            });
            cx.notify();
        }
    }

    fn clear_finished(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| m.clear_finished_tasks());
        cx.notify();
    }
}

impl Render for TasksPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let total = model.tasks.len();
        let running = model.tasks.iter().filter(|t| t.is_running()).count();
        let completed = model
            .tasks
            .iter()
            .filter(|t| matches!(t.finished, Some(Ok(_))))
            .count();
        let failed = model.tasks.iter().filter(|t| t.is_failed()).count();
        let cancelled = model.tasks.iter().filter(|t| t.is_cancelled()).count();
        // 按"未结束在前 + 同状态按 started_at 倒序"排序（不需 clone — 用 ref 即可）
        let mut indices: Vec<usize> = (0..model.tasks.len()).collect();
        indices.sort_by(|&a, &b| {
            let ta = &model.tasks[a];
            let tb = &model.tasks[b];
            match (ta.is_running(), tb.is_running()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => tb.started_at_unix.cmp(&ta.started_at_unix),
            }
        });
        let _ = model;

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(
                PageHeader::new("下载任务")
                    .subtitle(format!(
                        "{total} 个任务 · {running} 运行中 · {completed} 已完成 · {failed} 失败 · {cancelled} 已取消"
                    ))
                    .action(
                        Button::new("clear-finished")
                            .icon(Icon::new(IconName::Delete))
                            .label("清除已完成")
                            .disabled(completed + failed + cancelled == 0)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.clear_finished(cx);
                            })),
                    ),
            )
            .child(if indices.is_empty() {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::Inbox, "暂无下载任务")
                            .subtitle("从「搜索下载」页面下载一本书后，任务会出现在这里。"),
                    )
                    .into_any_element()
            } else {
                // 把必要字段 (id, origin, total, completed, failed, finished, failures, cancelling,
                // cancelling) 复制成本地 Vec<TaskSummary> — 不需要 clone 整个 DownloadTask
                // (其 mpsc::Receiver / CancelToken 不可 Clone)。
                let summaries: Vec<TaskSummary> = {
                    let m = self.model.read(cx);
                    indices
                        .iter()
                        .map(|&i| {
                            let t = &m.tasks[i];
                            TaskSummary {
                                id: t.id,
                                origin: t.origin.clone(),
                                started_at_unix: t.started_at_unix,
                                finished_at_unix: t.finished_at_unix,
                                book_meta: t.book_meta.clone(),
                                total_chapters: t.total_chapters,
                                completed: t.completed,
                                failed: t.failed,
                                last_chapter_title: t.last_chapter_title.clone(),
                                finished: t.finished.clone(),
                                failures: t.failures.clone(),
                                cancelling: t.cancelling,
                            }
                        })
                        .collect()
                };
                v_flex()
                    .flex_1()
                    .size_full()
                    .overflow_hidden()
                    .gap_3()
                    .children(summaries.iter().map(|s| render_task_card(s, cx.entity(), cx)))
                    .into_any_element()
            })
    }
}

/// 渲染一条任务卡片。
fn render_task_card(
    task: &TaskSummary,
    page: Entity<TasksPage>,
    cx: &mut App,
) -> impl IntoElement {
    let running = task.is_running();
    let succeeded = matches!(task.finished, Some(Ok(_)));
    let failed = task.is_failed();
    let cancelled = task.is_cancelled();

    let title = truncate(task.book_name(), 50);
    let source_id = task.origin.source_id;
    let total = task.total_chapters;
    let completed = task.completed;
    let failed_count = task.failed;

    let progress_pct = if total > 0 {
        (completed as f32 / total as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let status = if running {
        StatusKind::Info
    } else if succeeded {
        StatusKind::Success
    } else if failed {
        StatusKind::Error
    } else if cancelled {
        StatusKind::Neutral
    } else {
        StatusKind::Info
    };
    let status_label = if running {
        "运行中".to_string()
    } else if succeeded {
        "已完成".to_string()
    } else if failed {
        "失败".to_string()
    } else if cancelled {
        "已取消".to_string()
    } else {
        "未知".to_string()
    };

    // 输出路径（成功才有）
    let output_path = match &task.finished {
        Some(Ok(p)) => Some(p.clone()),
        _ => None,
    };

    v_flex()
        .id(("task-card", task.id))
        .p_4()
        .gap_2()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().sidebar)
        // 标题行
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(cx.theme().foreground)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("源 #{source_id} · {completed}/{total} 章 · 失败 {failed_count}")),
                        ),
                )
                .child(StatusBadge::new(status, status_label)),
        )
        // 进度条
        .when(running, |this| {
            this.child(
                Progress::new()
                    .value(progress_pct)
                    .w_full(),
            )
        })
        // 失败章节折叠
        .when(!task.failures.is_empty(), |this| {
            let fail_count = task.failures.len();
            this.child(
                Accordion::new(("task-fails", task.id))
                    .item(|item| {
                        item.title(format!("失败章节明细 ({fail_count})"))
                            .child(
                                v_flex()
                                    .gap_1()
                                    .p_2()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .children(task.failures.iter().take(20).map(|(idx, title, reason)| {
                                        div()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .child(format!("第 {idx} 章 — {title}")),
                                            )
                                            .child(div().child(format!("原因: {reason}")))
                                    })),
                            )
                    }),
            )
        })
        // 动作按钮行
        .child(
            h_flex()
                .gap_2()
                .justify_end()
                // 取消（仅运行中）
                .when(running, |this| {
                    let page_for_cancel = page.clone();
                    let task_id = task.id;
                    let cancelling = task.cancelling;
                    this.child(
                        Button::new(("task-cancel", task_id))
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::Close))
                            .label(if cancelling {
                                "正在取消..."
                            } else {
                                "取消"
                            })
                            .disabled(cancelling)
                            .on_click(move |_, _window, cx| {
                                page_for_cancel.update(cx, |p, cx| {
                                    p.cancel(task_id, cx);
                                });
                            }),
                    )
                })
                // 重试（仅失败 / 取消）
                .when(failed || cancelled, |this| {
                    let page_for_retry = page.clone();
                    let task_id = task.id;
                    this.child(
                        Button::new(("task-retry", task_id))
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::Loader))
                            .label("重试")
                            .on_click(move |_, _window, cx| {
                                page_for_retry.update(cx, |p, cx| {
                                    p.retry(task_id, cx);
                                });
                            }),
                    )
                })
                // 打开 / 位置（仅成功）
                .when_some(output_path.clone(), |this, path| {
                    let path_open = path.clone();
                    let path_reveal = path.clone();
                    let task_id = task.id;
                    this.child(
                        Button::new(("task-open", task_id))
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::ExternalLink))
                            .label("打开")
                            .on_click(move |_, _window, _cx| {
                                if let Err(e) = open_path(&path_open) {
                                    tracing::warn!("open_path failed: {e:#}");
                                }
                            }),
                    )
                    .child(
                        Button::new(("task-reveal", task_id))
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::Folder))
                            .label("位置")
                            .on_click(move |_, _window, _cx| {
                                if let Err(e) = reveal_in_folder(&path_reveal) {
                                    tracing::warn!("reveal_in_folder failed: {e:#}");
                                }
                            }),
                    )
                }),
        )
}
