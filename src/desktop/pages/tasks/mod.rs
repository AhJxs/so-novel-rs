//! Tasks 页面：下载任务管理（进度 / 取消 / 重试 / 打开 / 位置 / 删除单条）。
//!
//! `PageHeader` + 5 个带计数后缀的过滤 Button + `List` 虚拟滚动列表；已结束的任务可删除
//! （弹 confirm Dialog → `AppModel::delete_task`）。
//! 子模块：`summary`（`TaskSummary` + 过滤/排序 helper）、`toolbar`、`delegate`、`row`。

mod delegate;
mod row;
mod summary;
mod toolbar;

use gpui_kit::component::{
    ActiveTheme as _, IconName, WindowExt,
    button::ButtonVariant,
    dialog::{AlertDialog, Dialog},
    list::{List, ListState},
    scroll::ScrollableElement as _,
    v_flex,
};
use gpui_kit::{
    App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, div, px,
};

use crate::desktop::components::{EmptyState, PageHeader, Pagination, compute_page_window};
use crate::desktop::model::AppModel;
use crate::desktop::model::tasks::DeleteTaskResult;
use rust_i18n::t;

use self::delegate::TasksDelegate;
pub use self::summary::TaskSummary;
use self::summary::{TaskFilter, build_summaries, count_by_status, filter_and_sort_indices};

pub struct TasksPage {
    model: Entity<AppModel>,
    /// 当前过滤。UI-only，切按钮时更新 + cx.notify。
    filter: TaskFilter,
    /// gpui-kit 的虚拟列表 + 自定义 Delegate，必须在 `new()` 里建一次并缓存。
    list_state: Entity<ListState<TasksDelegate>>,
    /// 当前 0-based 页码。UI-only，每次过滤变化时重置为 0。
    current_page: usize,
}

impl TasksPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let page_handle = cx.entity();
        let delegate = TasksDelegate::new(page_handle);
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx));
        Self {
            model,
            filter: TaskFilter::default(),
            list_state,
            current_page: 0,
        }
    }

    pub(super) fn cancel(&self, task_id: u64, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| {
            if let Some(t) = m.tasks.iter_mut().find(|t| t.id == task_id)
                && let Some(cancel) = t.cancel.take()
            {
                cancel.cancel();
                t.cancelling = true;
            }
        });
        cx.notify();
    }

    pub(super) fn retry(&self, task_id: u64, cx: &mut Context<Self>) {
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

    /// 点删除按钮 → 弹 confirm Dialog 二次确认。
    pub(super) fn prompt_delete(
        &self,
        task_id: u64,
        book_name: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let model = self.model.clone();
        let model_id = model.entity_id();
        // 书名兜底：空时用 i18n fallback。
        let name: String = if book_name.trim().is_empty() {
            t!("Tasks.fallback_unknown_book").to_string()
        } else {
            book_name
        };

        window.open_alert_dialog(cx, move |alert: AlertDialog, _window, _cx| {
            // dialog builder 每帧重调（Fn）—— on_ok 用引用捕获 + clone 避 FnOnce。
            let model_for_ok = model.clone();
            let name_for_ok = name.clone();
            let model_id_for_ok = model_id;

            alert
                .title(t!("Tasks.delete_dialog.title"))
                .description(t!("Tasks.delete_dialog.message", book_name = &name_for_ok))
                // 顶层 dialog builder 负责单个按钮（见 `library/mod.rs` 同处注释）。
                .ok_text(t!("Tasks.delete_dialog.confirm_button"))
                .cancel_text(t!("Tasks.delete_dialog.cancel_button"))
                .ok_variant(ButtonVariant::Danger)
                .confirm()
                .on_ok(move |_ev: &ClickEvent, _window, cx| {
                    model_for_ok.update(cx, |m, _cx| match m.delete_task(task_id) {
                        DeleteTaskResult::Deleted => m.push_success(t!("Toasts.delete_task_ok", book_name = &name_for_ok)),
                        DeleteTaskResult::StillRunning => m.push_warning(t!("Toasts.delete_task_still_running", book_name = &name_for_ok)),
                        DeleteTaskResult::Missing => {
                            m.push_warning(t!("Toasts.delete_task_missing"));
                        }
                    });
                    cx.notify(model_id_for_ok);
                    true // 关闭 dialog
                })
        });
    }

    /// 点「失败明细」按钮 → 弹只读 Dialog 列出失败章节 + 原因。
    /// 不用行内 `Accordion`：`List` 要求所有行等高 + `overflow_hidden`，展开撑高会被裁掉，
    /// 可变高度内容必须放进 Dialog。
    pub(super) fn show_failures(
        failures: Vec<(u32, String, String)>,
        book_name: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 书名兜底：空时用 i18n fallback。
        let name: String = if book_name.trim().is_empty() {
            t!("Tasks.fallback_unknown_book").to_string()
        } else {
            book_name
        };

        window.open_dialog(cx, move |dialog: Dialog, _window, cx| {
            // dialog builder 每帧重调（Fn）—— 捕获用引用 / clone，不能 FnOnce。
            let name_for_title = name.clone();
            let failures_for_list = failures.clone();
            // 宽 640px + 不调 `.alert()`/`.confirm()`，保留默认 close_button + overlay / Esc 关闭。
            dialog
                .title(t!("Tasks.failures_dialog.title", book_name = &name_for_title))
                .w(px(640.))
                // 失败章节可能很多 —— 限高 + 纵向滚动。`overflow_y_scrollbar` 是 terminal
                // builder（返回 `Scrollable<Div>`），必须放链尾。
                .child(
                    v_flex()
                        .max_h(px(400.))
                        .gap_1()
                        .py_1()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .children(
                            failures_for_list
                                .iter()
                                .take(50)
                                .map(|(idx, title, reason)| {
                                    div().gap_1().child(div().child(format!(
                                        "{} · {}",
                                        t!("Tasks.card.failure_chapter", idx = &idx.to_string(), title = title),
                                        t!("Tasks.card.failure_reason", reason = reason),
                                    )))
                                }),
                        )
                        .overflow_y_scrollbar(),
                )
        });
    }

    /// 切过滤 —— 跳回第 1 页。
    pub(super) fn set_filter(&mut self, f: TaskFilter, cx: &mut Context<Self>) {
        if self.filter != f {
            self.filter = f;
            self.current_page = 0;
            cx.notify();
        }
    }
}

impl Render for TasksPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // ---- 1. 统计各状态数量（按钮 label 后缀）----
        let counts = count_by_status(self.model.read(cx));

        // ---- 2. 按当前过滤筛选 + 排序 ----
        let indices = filter_and_sort_indices(self.model.read(cx), self.filter);

        let summaries = build_summaries(self.model.read(cx), &indices);
        let total = summaries.len();

        // TODO：接入 list_cache。当前 3 个 helper 各自跑、结果不共享；Tasks 数量少
        //（通常 < 100），改造风险 vs 收益不划算，暂不接入。

        // ---- 4. 分页切片 + 兜底（过滤后 current_page 越界 → 回卷）----
        let w = compute_page_window(total, &mut self.current_page);
        let page_items: Vec<TaskSummary> = if w.is_empty() {
            Vec::new()
        } else {
            summaries[w.start..w.end].to_vec()
        };
        self.list_state.update(cx, |state, _cx| {
            state.delegate_mut().page_items = page_items;
        });

        let filter = self.filter;

        // ---- 5. 渲染 ----
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            // Header：title + subtitle，**无** action。
            .child(PageHeader::new(t!("Tasks.page_title")).subtitle(t!("Tasks.subtitle")))
            // 过滤按钮组：「全部 / 运行中 / 已完成 / 失败 / 已取消」，各带数量。
            .child(toolbar::filter_buttons(self.filter, counts, cx))
            // 列表 / 空态
            .child(if total == 0 {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::Inbox, t!(filter.empty_title_key()))
                            .subtitle(t!(filter.empty_subtitle_key())),
                    )
                    .into_any_element()
            } else {
                // List 容器（border + padding + size_full），让选中边框不被滚动条遮挡。
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
            // 分页页脚（`Pagination` 自己判可见性，不足一页不渲染）。
            .child(Pagination::new(
                self.current_page,
                w.page_count,
                cx.listener(|this, &new_page, _window, cx| {
                    this.current_page = new_page;
                    cx.notify();
                }),
            ))
    }
}
