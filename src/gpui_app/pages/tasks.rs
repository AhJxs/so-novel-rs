//! Tasks 页面：下载任务管理（进度 / 取消 / 重试 / 打开 / 位置 / 删除单条）。
//!
//! 布局（参考 library.rs / sources.rs / search.rs 的统一模式）：
//! - PageHeader：title + subtitle（新描述，无统计数字；无 action —— 去掉"清除已完成"）。
//! - 过滤按钮组：「全部 / 运行中 / 已完成 / 失败 / 已取消」，各带数量后缀，
//!   `.small().ghost().selected(bool)` 标记当前过滤（跟 sources.rs 状态过滤同款）。
//! - 结果列表：`gpui-component::List` + `TasksDelegate`（虚拟滚动）。
//!   每条任务卡片含书名 / 元信息 / 状态徽章 / 进度条 / 失败折叠 / 动作按钮。
//!   已结束任务（完成 / 失败 / 已取消）显示「删除」按钮 → 弹 confirm Dialog 二次确认
//!   （复用 library.rs prompt_delete 模式）→ `AppModel::delete_task`。
//!
//! i18n 文本 + 按钮组过滤 + List 虚拟滚动。

use gpui::{
    App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, IconName, IndexPath, Selectable, Sizable, WindowExt,
    accordion::Accordion,
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::{Dialog, DialogButtonProps},
    h_flex,
    list::{List, ListDelegate, ListItem, ListState},
    progress::Progress,
    v_flex,
};

use crate::app::AppModel;
use crate::gpui_app::components::{
    compute_page_window, EmptyState, PageHeader, Pagination, StatusBadge, StatusKind, truncate,
};
use crate::gpui_app::i18n::{ts, ts_fmt};
use crate::models::{Book, SearchResult};
use crate::util::system::{open_path, reveal_in_folder};
use std::path::PathBuf;

/// `DownloadTask` 含 `mpsc::Receiver` / `CancelToken` 不可 Clone。
/// UI 渲染时复制必要字段为 `TaskSummary`，避开 Clone 限制。
#[derive(Clone)]
#[allow(dead_code)]
struct TaskSummary {
    id: u64,
    /// 全局序号（跨分页连续，0-based，显示时 +1）。render 切片时填入。
    index: usize,
    origin: SearchResult,
    started_at_unix: i64,
    finished_at_unix: Option<i64>,
    book_meta: Option<Book>,
    total_chapters: usize,
    completed: u32,
    failed: u32,
    last_chapter_title: String,
    /// 跟 `DownloadTask::finished` 同型 —— 成功 = Ok(path)；结束原因见 `FinishedReason`。
    finished: Option<Result<PathBuf, crate::db::tasks::FinishedReason>>,
    failures: Vec<(u32, String, String)>,
    cancelling: bool,
}

impl TaskSummary {
    fn is_running(&self) -> bool {
        self.finished.is_none()
    }
    fn is_failed(&self) -> bool {
        // is_failed 仅看 finished：Err(Failed) 才是真正的失败。
        // is_cancelled 由独立函数判断（UserCancelled / AppRestarted 归为 cancelled）。
        !self.is_cancelled() && !self.is_running() && self.is_finished_with_err_failed()
    }
    fn is_cancelled(&self) -> bool {
        matches!(
            self.finished.as_ref(),
            Some(Err(reason)) if reason.is_cancelled()
        )
    }
    /// 已结束且 reason 是 `Failed`（不是 cancelled 也不是运行中）。
    fn is_finished_with_err_failed(&self) -> bool {
        matches!(
            self.finished.as_ref(),
            Some(Err(crate::db::tasks::FinishedReason::Failed { .. }))
        )
    }
    fn book_name(&self) -> &str {
        self.book_meta
            .as_ref()
            .map(|b| b.book_name.as_str())
            .unwrap_or(self.origin.book_name.as_str())
    }
}

/// 过滤种类 —— 按下载状态分组，`All` 不限。顺序固定 = 按钮组顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum TaskFilter {
    #[default]
    All,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl TaskFilter {
    /// 全部过滤，顺序与按钮组 index 对齐。
    const ALL: [TaskFilter; 5] = [
        TaskFilter::All,
        TaskFilter::Running,
        TaskFilter::Completed,
        TaskFilter::Failed,
        TaskFilter::Cancelled,
    ];

    /// label 的 i18n key（不含数量后缀）。
    fn label_key(self) -> &'static str {
        match self {
            TaskFilter::All => "Tasks.tab.all",
            TaskFilter::Running => "Tasks.tab.running",
            TaskFilter::Completed => "Tasks.tab.completed",
            TaskFilter::Failed => "Tasks.tab.failed",
            TaskFilter::Cancelled => "Tasks.tab.cancelled",
        }
    }

    /// 空态 title 的 i18n key。
    fn empty_title_key(self) -> &'static str {
        match self {
            TaskFilter::All => "Tasks.empty.all.title",
            TaskFilter::Running => "Tasks.empty.running.title",
            TaskFilter::Completed => "Tasks.empty.completed.title",
            TaskFilter::Failed => "Tasks.empty.failed.title",
            TaskFilter::Cancelled => "Tasks.empty.cancelled.title",
        }
    }

    /// 空态 subtitle 的 i18n key。
    fn empty_subtitle_key(self) -> &'static str {
        match self {
            TaskFilter::All => "Tasks.empty.all.subtitle",
            TaskFilter::Running => "Tasks.empty.running.subtitle",
            TaskFilter::Completed => "Tasks.empty.completed.subtitle",
            TaskFilter::Failed => "Tasks.empty.failed.subtitle",
            TaskFilter::Cancelled => "Tasks.empty.cancelled.subtitle",
        }
    }
}

/// Tasks 页面 entity。
pub struct TasksPage {
    model: Entity<AppModel>,
    /// 当前过滤。UI-only，切按钮时更新 + cx.notify。
    filter: TaskFilter,
    /// gpui-component 虚拟列表 + 自定义 Delegate。必须在 `new()` 里建一次并缓存。
    list_state: Entity<ListState<TasksDelegate>>,
    /// 当前 0-based 页码。UI-only，每次过滤变化时重置为 0。
    current_page: usize,
}

impl TasksPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let page_handle = cx.entity().clone();
        let delegate = TasksDelegate::new(page_handle);
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx));
        Self {
            model,
            filter: TaskFilter::default(),
            list_state,
            current_page: 0,
        }
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

    /// 点删除按钮 → 弹 confirm Dialog 二次确认。跟 library.rs `prompt_delete` 同模式。
    fn prompt_delete(&self, task_id: u64, book_name: String, window: &mut Window, cx: &mut App) {
        let model = self.model.clone();
        let model_id = model.entity_id();
        // 书名兜底：空时用 i18n fallback。
        let name: String = if book_name.trim().is_empty() {
            ts("Tasks.fallback_unknown_book").to_string()
        } else {
            book_name
        };

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            // builder 是 Fn（每帧重调）—— on_ok 也要能多次调，用引用捕获 + clone 避 FnOnce。
            let model_for_ok = model.clone();
            let name_for_ok = name.clone();
            let model_id_for_ok = model_id;

            dialog
                .title(ts("Tasks.delete_dialog.title"))
                .child(div().child(ts_fmt(
                    "Tasks.delete_dialog.message",
                    &[("book_name", &name_for_ok)],
                )))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(ts("Tasks.delete_dialog.confirm_button"))
                        .cancel_text(ts("Tasks.delete_dialog.cancel_button"))
                        .ok_variant(ButtonVariant::Danger),
                )
                .confirm()
                .on_ok(move |_ev: &ClickEvent, _window, cx| {
                    model_for_ok.update(cx, |m, _cx| {
                        m.delete_task(task_id);
                    });
                    cx.notify(model_id_for_ok);
                    true // 关闭 dialog
                })
        });
    }

    /// 切过滤 —— 跳回第 1 页（跟 library.rs / sources.rs 同款）。
    fn set_filter(&mut self, f: TaskFilter, cx: &mut Context<Self>) {
        if self.filter != f {
            self.filter = f;
            self.current_page = 0;
            cx.notify();
        }
    }
}

impl Render for TasksPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // ---- 1. 统计各状态数量（按钮 label 后缀 + 过滤）----
        let (n_all, n_running, n_completed, n_failed, n_cancelled) = {
            let m = self.model.read(cx);
            let all = m.tasks.len();
            let running = m.tasks.iter().filter(|t| t.is_running()).count();
            let completed = m
                .tasks
                .iter()
                .filter(|t| matches!(t.finished, Some(Ok(_))))
                .count();
            let failed = m.tasks.iter().filter(|t| t.is_failed()).count();
            let cancelled = m.tasks.iter().filter(|t| t.is_cancelled()).count();
            (all, running, completed, failed, cancelled)
        };
        let counts = [n_all, n_running, n_completed, n_failed, n_cancelled];

        // ---- 2. 按当前过滤筛选 + 排序 ----
        // 排序：运行中在前；同组按时间倒序（finished_at，无则 started_at）。
        let filter = self.filter;
        let mut indices: Vec<usize> = {
            let m = self.model.read(cx);
            (0..m.tasks.len())
                .filter(|&i| task_matches_filter(&m.tasks[i], filter))
                .collect()
        };
        {
            let m = self.model.read(cx);
            indices.sort_by(|&a, &b| {
                let ta = &m.tasks[a];
                let tb = &m.tasks[b];
                let key =
                    |t: &crate::app::DownloadTask| t.finished_at_unix.unwrap_or(t.started_at_unix);
                tb.is_running()
                    .cmp(&ta.is_running())
                    .then_with(|| key(tb).cmp(&key(ta)))
            });
        }

        // ---- 3. 复制 TaskSummary（避开 DownloadTask 不可 Clone）推给 delegate ----
        // index = 在过滤+排序后列表里的 0-based 位置（跨分页连续，显示时 +1）。
        let summaries: Vec<TaskSummary> = {
            let m = self.model.read(cx);
            indices
                .iter()
                .enumerate()
                .map(|(index, &i)| {
                    let t = &m.tasks[i];
                    TaskSummary {
                        id: t.id,
                        index,
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
        let total = summaries.len();

        // ---- 4. 分页切片 + 兜底（过滤后 current_page 越界 → 回卷）----
        let w = compute_page_window(total, &mut self.current_page);
        let page_items: Vec<TaskSummary> = if !w.is_empty() {
            summaries[w.start..w.end].to_vec()
        } else {
            Vec::new()
        };
        // 推给 delegate，List 渲染时读到。
        self.list_state.update(cx, |state, _cx| {
            state.delegate_mut().page_items = page_items;
        });

        // ---- 5. 渲染 ----
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            // Header：title + 新描述 subtitle，**无** action。
            .child(PageHeader::new(ts("Tasks.page_title")).subtitle(ts("Tasks.subtitle")))
            // 过滤按钮组：「全部 / 运行中 / 已完成 / 失败 / 已取消」，各带数量。
            // 跟 sources.rs 状态过滤同款：.small().ghost().selected(bool)。
            .child(h_flex().gap_1().items_center().children(
                TaskFilter::ALL.iter().enumerate().map(|(i, f)| {
                    let f = *f;
                    Button::new(("task-filter", i as u64))
                        .small()
                        .ghost()
                        .selected(self.filter == f)
                        .label(format!("{} {}", ts(f.label_key()), counts[i]))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.set_filter(f, cx);
                        }))
                }),
            ))
            // 列表 / 空态
            .child(if total == 0 {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::Inbox, ts(filter.empty_title_key()))
                            .subtitle(ts(filter.empty_subtitle_key())),
                    )
                    .into_any_element()
            } else {
                // List 容器：跟 library.rs / sources.rs 同款（border + .px(12).py(4) +
                // List::new().size_full()），让选中边框不被滚动条遮挡。
                div()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .child(List::new(&self.list_state).p(px(12.)).size_full())
                    .into_any_element()
            })
            // 分页页脚（仅在列表非空时渲染 —— 空态不显示，避免无意义的"第 1 页 / 共 0 条"）。
            .when(total > 0, |this| {
                this.child(Pagination::new(
                    self.current_page,
                    w.page_count,
                    cx.listener(|this, &new_page, _window, _cx| {
                        this.current_page = new_page;
                        _cx.notify();
                    }),
                ))
            })
    }
}

/// 任务是否属于该过滤。
fn task_matches_filter(t: &crate::app::DownloadTask, f: TaskFilter) -> bool {
    match f {
        TaskFilter::All => true,
        TaskFilter::Running => t.is_running(),
        TaskFilter::Completed => matches!(t.finished, Some(Ok(_))),
        TaskFilter::Failed => matches!(t.finished.as_ref(), Some(Err(_))) && !task_is_cancelled(t),
        TaskFilter::Cancelled => task_is_cancelled(t),
    }
}

fn task_is_cancelled(t: &crate::app::DownloadTask) -> bool {
    matches!(
        t.finished.as_ref(),
        Some(Err(reason)) if reason.is_cancelled()
    )
}

// ============================================================================
// Delegate + row renderer
// ============================================================================

/// `gpui-component::List` 的 delegate —— 把当前过滤下的 `TaskSummary` 列表渲染成行。
///
/// 完全对齐 `sources.rs::SourcesDelegate` / `search.rs::SearchDelegate` 模式：
/// - `page_items` 由 `TasksPage::render` 在每帧 render 前写入；`render_item` 直接取。
/// - 持有 `Entity<TasksPage>` handle 以便动作按钮 → page 转发（cancel / retry / prompt_delete）。
/// - 选中态交给 `ListItem::selected(...)` + `set_selected_index` 配对管理。
struct TasksDelegate {
    /// 当前过滤下要展示的任务。
    page_items: Vec<TaskSummary>,
    /// 当前选中项。`None` = 未选中。
    selected_index: Option<IndexPath>,
    /// 拿 TasksPage handle 用于动作按钮 → 转发回 page。
    page_handle: Entity<TasksPage>,
}

impl TasksDelegate {
    fn new(page_handle: Entity<TasksPage>) -> Self {
        Self {
            page_items: Vec::new(),
            selected_index: None,
            page_handle,
        }
    }
}

impl ListDelegate for TasksDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.page_items.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let task = self.page_items.get(ix.row)?.clone();
        Some(
            ListItem::new(ix)
                .selected(Some(ix) == self.selected_index)
                .rounded(cx.theme().radius)
                .mb(px(4.))
                .child(render_task_row(task, self.page_handle.clone(), &mut *cx)),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected_index = ix;
        cx.notify();
    }
}

/// 把 unix 秒格式化成 `YYYY-MM-DD HH:MM`（本地时区）。0 / 失败走 i18n fallback。
///
/// 跟 library.rs `format_unix_secs` 同款实现（Rfc3339 截前 16 字符 + T 换空格）。
fn format_started_time(secs: i64) -> String {
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;
    if secs <= 0 {
        return ts("Tasks.card.meta.time_unknown").to_string();
    }
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(secs) else {
        return ts("Tasks.card.meta.time_unknown").to_string();
    };
    let local =
        dt.to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC));
    local
        .format(&Rfc3339)
        .ok()
        .map(|s| s[..16].replace('T', " "))
        .unwrap_or_else(|| ts("Tasks.card.meta.time_unknown").to_string())
}

/// 渲染一条任务行（卡片式：序号 / 标题行 / 进度条 / 失败折叠 / 动作按钮）。
fn render_task_row(task: TaskSummary, page: Entity<TasksPage>, cx: &mut App) -> impl IntoElement {
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
    let started_display = format_started_time(task.started_at_unix);

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
    let status_label: SharedString = ts(status_key);

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
            None => ts("Tasks.fallback_unknown_author"),
        }
    };
    // 书源名：直接用结果自带的 source_name（数据，不译），空走 fallback。
    let source_name_display: SharedString = if task.origin.source_name.trim().is_empty() {
        ts("Tasks.fallback_unknown_book")
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
                .font_weight(gpui::FontWeight::SEMIBOLD)
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
                                            ts("Tasks.card.meta.started"),
                                            started_display
                                        )),
                                ),
                        )
                        .child(Progress::new().value(progress_pct).w_full())
                        // 失败章节折叠
                        .when(!task.failures.is_empty(), |this| {
                            let fail_count = task.failures.len();
                            this.child(Accordion::new(("task-fails", task.id)).item(|item| {
                                item.title(ts_fmt(
                                    "Tasks.card.failures",
                                    &[("n", &fail_count.to_string())],
                                ))
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .p_2()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .children(task.failures.iter().take(20).map(
                                            |(idx, title, reason)| {
                                                div().gap_1().child(
                                                    div()
                                                        .font_weight(gpui::FontWeight::MEDIUM)
                                                        .child(format!(
                                                            "{} · {}",
                                                            ts_fmt(
                                                                "Tasks.card.failure_chapter",
                                                                &[
                                                                    ("idx", &idx.to_string()),
                                                                    ("title", title)
                                                                ]
                                                            ),
                                                            ts_fmt(
                                                                "Tasks.card.failure_reason",
                                                                &[("reason", reason)]
                                                            ),
                                                        )),
                                                )
                                            },
                                        )),
                                )
                            }))
                        })
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
                                        ts("Tasks.card.action.cancelling")
                                    } else {
                                        ts("Tasks.card.action.cancel")
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
                                            .label(ts("Tasks.card.action.retry"))
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
                                            .small()
                                            .outline()
                                            .icon(Icon::new(IconName::ExternalLink))
                                            .label(ts("Tasks.card.action.open"))
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
                                            .label(ts("Tasks.card.action.reveal"))
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
                                            .label(ts("Tasks.card.action.delete"))
                                            .on_click(move |_, window, cx| {
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
