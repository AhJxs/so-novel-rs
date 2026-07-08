//! 单条任务行渲染（卡片式：序号 / 标题行 / 进度条 / 失败折叠 / 动作按钮）。

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Entity, FontWeight, IntoElement, ParentElement, SharedString, Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, IconName, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    progress::Progress,
    v_flex,
};

use crate::gpui_app::components::{StatusBadge, StatusKind, truncate};
use crate::i18n::{ts_cached, ts_fmt};
use crate::utils::system::{open_path, reveal_in_folder};

use super::TasksPage;
use super::summary::TaskSummary;

/// 渲染一条任务行（卡片式：序号 / 标题行 / 进度条 / 失败折叠 / 动作按钮）。
pub(super) fn render(task: TaskSummary, page: Entity<TasksPage>, cx: &mut App) -> impl IntoElement {
    let running = task.is_running();
    let succeeded = matches!(task.finished, Some(Ok(_)));
    let failed = task.is_failed();
    let cancelled = task.is_cancelled();

    let title = truncate(task.book_name(), 50);

    let total = task.total_chapters;
    let completed = task.completed;
    let failed_count = task.failed;
    // 全局序号（1-based，跨分页连续）+ 开始时间。
    let seq = task.index + 1;
    let started_display = crate::utils::formatting::format_local_unix_secs(
        task.started_at_unix,
        "Tasks.card.meta.time_unknown",
        "Tasks.card.meta.time_unknown",
        "Tasks.card.meta.time_unknown",
    );

    let progress_pct = if total > 0 {
        (completed as f32 / total as f32).clamp(0.0, 1.0) * 100.0
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
    let status_key = if running {
        "Tasks.card.status.running"
    } else if succeeded {
        "Tasks.card.status.completed"
    } else if failed {
        "Tasks.card.status.failed"
    } else if cancelled {
        "Tasks.card.status.cancelled"
    } else {
        "Tasks.card.status.unknown"
    };
    let status_label: SharedString = ts_cached(status_key);

    // 作者：优先详情拉的 book_meta.author（完整），否则 origin.author；空走 fallback。
    let author_display: SharedString = {
        let from_book = task.book_meta.as_ref().map(|b| b.author.as_str());
        let raw = from_book.filter(|s| !s.trim().is_empty()).or_else(|| {
            task.origin
                .author
                .as_deref()
                .filter(|s| !s.trim().is_empty())
        });
        match raw {
            Some(s) => SharedString::from(truncate(s, 30).to_string()),
            None => ts_cached("Tasks.fallback_unknown_author"),
        }
    };
    // 书源名：直接用结果自带的 source_name（数据，不译），空走 fallback。
    let source_name_display: SharedString = if task.origin.source_name.trim().is_empty() {
        ts_cached("Tasks.fallback_unknown_book")
    } else {
        SharedString::from(truncate(&task.origin.source_name, 20).to_string())
    };

    // 书名行：书名 · 作者 · 书源名（作者 / 书源名 muted、xs，紧贴书名右侧）。
    let title_line = h_flex()
        .gap_2()
        .items_baseline()
        .child(
            div()
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().foreground)
                .child(title),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{} · {}", author_display, source_name_display)),
        );

    // 进度条上的章节信息：N/M 章 · 失败 n（走 ts_fmt 占位符，避免切语言乱序）。
    // 失败数为 0 时不显示「失败 n」段。
    let chapters_text = if failed_count > 0 {
        format!(
            "{} · {}",
            ts_fmt(
                "Tasks.card.meta.chapters",
                &[
                    ("completed", &completed.to_string()),
                    ("total", &total.to_string())
                ]
            ),
            ts_fmt(
                "Tasks.card.meta.failed",
                &[("n", &failed_count.to_string())]
            ),
        )
    } else {
        ts_fmt(
            "Tasks.card.meta.chapters",
            &[
                ("completed", &completed.to_string()),
                ("total", &total.to_string()),
            ],
        )
        .to_string()
    };

    // 输出路径（成功才有）
    let output_path = match &task.finished {
        Some(Ok(p)) => Some(p.clone()),
        _ => None,
    };

    // 删除按钮要 clone 的：task id + 书名 + page。
    let task_id = task.id;
    let book_name_for_delete = task.book_name().to_string();
    let page_for_delete = page.clone();

    h_flex()
        .px_2()
        .py_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .items_start()
        // ---- 序号列（跨分页连续：1-based，48px 装 "#100"）----
        .child(
            div()
                .w(px(48.0))
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("#{seq}")),
        )
        // 右侧主体：标题行 / 进度条 / 失败折叠 / 动作按钮
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap_2()
                // 标题行：书名 · 作者 · 书源名（左）+ 状态徽章（右）
                .child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(title_line)
                        .child(StatusBadge::new(status, status_label)),
                )
                // 进度条 + 章节信息：进度条始终显示（已完成 100% / 失败·取消保留当前进度），
                // 章节信息（N/M 章 · 失败 n）在左、开始时间在右（行内两端对齐），
                // 进度条在上方整行下方。
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .justify_between()
                                .gap_2()
                                .items_baseline()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(chapters_text),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "{} {}",
                                            ts_cached("Tasks.card.meta.started"),
                                            started_display
                                        )),
                                ),
                        )
                        .child(
                            Progress::new()
                                .value(progress_pct)
                                .w_full()
                                .bg(cx.theme().success),
                        )
                        // 失败章节不再用行内 Accordion 折叠 —— `List` 要求所有行等高 +
                        // `overflow_hidden`（见 gpui-component list.rs），Accordion 展开撑高
                        // 会被裁掉。改成动作按钮区的「失败明细」按钮 → 弹 Dialog 只读列表
                        // （`TasksPage::show_failures`），把可变高度内容移出虚拟列表行。
                        // 动作按钮行
                        .child(
                            h_flex()
                                .pt_1()
                                .gap_2()
                                .justify_end()
                                // 取消（仅运行中）
                                .when(running, |this| {
                                    let page_for_cancel = page.clone();
                                    let task_id = task.id;
                                    let cancelling = task.cancelling;
                                    let label = if cancelling {
                                        ts_cached("Tasks.card.action.cancelling")
                                    } else {
                                        ts_cached("Tasks.card.action.cancel")
                                    };
                                    this.child(
                                        Button::new(("task-cancel", task_id))
                                            .small()
                                            .outline()
                                            .icon(Icon::new(IconName::Close))
                                            .label(label)
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
                                            .small()
                                            .outline()
                                            .icon(Icon::new(IconName::Loader))
                                            .label(ts_cached("Tasks.card.action.retry"))
                                            .on_click(move |_, _window, cx| {
                                                page_for_retry.update(cx, |p, cx| {
                                                    p.retry(task_id, cx);
                                                });
                                            }),
                                    )
                                })
                                // 失败明细（有失败章节时）→ 弹只读 Dialog 列出失败章节 + 原因。
                                // 不再用行内 Accordion（List 等高 + overflow_hidden 撑不开）。
                                .when(!task.failures.is_empty(), |this| {
                                    let page_for_fails = page.clone();
                                    let task_id = task.id;
                                    let failures = task.failures.clone();
                                    let book_name = task.book_name().to_string();
                                    let fail_count = failures.len();
                                    this.child(
                                        Button::new(("task-fails", task_id))
                                            .small()
                                            .outline()
                                            .icon(Icon::new(IconName::TriangleAlert))
                                            .label(ts_fmt(
                                                "Tasks.card.action.failures",
                                                &[("n", &fail_count.to_string())],
                                            ))
                                            .on_click(move |_, window: &mut Window, cx| {
                                                // Fn handler：每次点击重新 clone 给 update 闭包。
                                                let failures = failures.clone();
                                                let book_name = book_name.clone();
                                                page_for_fails.update(cx, |p, cx| {
                                                    p.show_failures(failures, book_name, window, cx);
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
                                            .small()
                                            .outline()
                                            .icon(Icon::new(IconName::ExternalLink))
                                            .label(ts_cached("Tasks.card.action.open"))
                                            .on_click(move |_, _window, _cx| {
                                                if let Err(e) = open_path(&path_open) {
                                                    tracing::warn!("open_path failed: {e:#}");
                                                }
                                            }),
                                    )
                                    .child(
                                        Button::new(("task-reveal", task_id))
                                            .small()
                                            .outline()
                                            .icon(Icon::new(IconName::Folder))
                                            .label(ts_cached("Tasks.card.action.reveal"))
                                            .on_click(move |_, _window, _cx| {
                                                if let Err(e) = reveal_in_folder(&path_reveal) {
                                                    tracing::warn!(
                                                        "reveal_in_folder failed: {e:#}"
                                                    );
                                                }
                                            }),
                                    )
                                })
                                // 删除（仅已结束：完成 / 失败 / 已取消）—— 弹 confirm Dialog 二次确认，
                                // 跟 library.rs prompt_delete 同模式。
                                .when(!running, |this| {
                                    this.child(
                                        Button::new(("task-delete", task_id))
                                            .small()
                                            .danger()
                                            .icon(Icon::new(IconName::Delete))
                                            .label(ts_cached("Tasks.card.action.delete"))
                                            .on_click(move |_, window: &mut Window, cx| {
                                                // prompt_delete 要 &self + &mut Window + &mut App：
                                                // page.update 闭包内 cx 是 Context<TasksPage>（无 window），
                                                // 所以 window 从 on_click 自带的 &mut Window 传入。
                                                page_for_delete.update(cx, |p, cx| {
                                                    p.prompt_delete(
                                                        task_id,
                                                        book_name_for_delete.clone(),
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }),
                                    )
                                }),
                        ),
                ),
        )
}
