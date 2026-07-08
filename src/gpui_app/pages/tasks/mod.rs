//! Tasks 页面：下载任务管理（进度 / 取消 / 重试 / 打开 / 位置 / 删除单条）。
//!
//! 布局（参考 library.rs / sources.rs / search.rs 的统一模式）：
//! - PageHeader：title + subtitle（新描述，无统计数字；无 action —— 去掉"清除已完成"）。
//! - 过滤按钮组：「全部 / 运行中 / 已完成 / 失败 / 已取消」，各带数量后缀，
//!   `.small().ghost().selected(bool)` 标记当前过滤（跟 sources.rs 状态过滤同款）。
//! - 结果列表：`gpui-component::List` + `TasksDelegate`（虚拟滚动）。
//!   每条任务卡片含书名 / 元信息 / 状态徽章 / 进度条 / 失败折叠 / 动作按钮。
//!   已结束任务（完成 / 失败 / 已取消）显示「删除」按钮 → 弹 confirm Dialog 二次确认
//!   （复用 library.rs `prompt_delete` 模式）→ `AppModel::delete_task`。
//!
//! 子模块：
//! - `summary` — `TaskSummary`（避开 `DownloadTask` 不可 Clone）+ `TaskFilter` + 过滤/排序 helper
//! - `toolbar` — 5-Button 状态过滤组
//! - `delegate` — `TasksDelegate` + `ListDelegate` impl
//! - `row` — 单条任务行渲染（卡片式）
//!
//! i18n 文本 + 按钮组过滤 + List 虚拟滚动。

mod delegate;
mod row;
mod summary;
mod toolbar;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render, Styled,
    Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, WindowExt,
    button::ButtonVariant,
    dialog::{Dialog, DialogButtonProps},
    list::{List, ListState},
    scroll::ScrollableElement as _,
    v_flex,
};

use crate::app::AppModel;
use crate::gpui_app::components::{EmptyState, PageHeader, Pagination, compute_page_window};
use crate::i18n::{ts, ts_fmt};

use self::delegate::TasksDelegate;
pub use self::summary::TaskSummary;
use self::summary::{TaskFilter, build_summaries, count_by_status, filter_and_sort_indices};

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
            if let Some(t) = m.tasks.iter_mut().find(|t| t.id == task_id) {
                if let Some(cancel) = t.cancel.take() {
                    cancel.cancel();
                    t.cancelling = true;
                }
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

    /// 点删除按钮 → 弹 confirm Dialog 二次确认。跟 library.rs `prompt_delete` 同模式。
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

    /// 点「失败明细」按钮 → 弹只读 Dialog 列出失败章节 + 原因。
    ///
    /// 不再用行内 `Accordion`：`List` 要求所有行等高 + `overflow_hidden`
    /// （gpui-component `list.rs`），Accordion 展开撑高会被裁掉。把可变高度内容
    /// 移出虚拟列表行，放进 Dialog（`.alert()` 单 OK 按钮 + 可滚动列表）。
    pub(super) fn show_failures(
        failures: Vec<(u32, String, String)>,
        book_name: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        // 书名兜底：空时用 i18n fallback。
        let name: String = if book_name.trim().is_empty() {
            ts("Tasks.fallback_unknown_book").to_string()
        } else {
            book_name
        };

        window.open_dialog(cx, move |dialog: Dialog, _window, cx| {
            // builder 是 Fn（每帧重调）—— 捕获用引用 / clone，不能 FnOnce。
            let name_for_title = name.clone();
            let failures_for_list = failures.clone();
            // 跟书籍详情 Dialog 同款样式：宽 640px + 不调 `.alert()`/`.confirm()`，
            // 保留默认 `close_button: true` 的右上角 X 关闭按钮 + overlay 点击关闭 + Esc。
            dialog
                .title(ts_fmt(
                    "Tasks.failures_dialog.title",
                    &[("book_name", &name_for_title)],
                ))
                .w(px(640.))
                // 失败章节可能很多 —— 限高 + 纵向滚动，避免 Dialog 撑出屏幕。
                // `overflow_y_scrollbar` 是 terminal builder（返回 `Scrollable<Div>`），
                // 必须放在链尾；详见 search/detail_dialog.rs 同款用法。
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
                                        ts_fmt(
                                            "Tasks.card.failure_chapter",
                                            &[("idx", &idx.to_string()), ("title", title)],
                                        ),
                                        ts_fmt(
                                            "Tasks.card.failure_reason",
                                            &[("reason", reason)],
                                        ),
                                    )))
                                }),
                        )
                        .overflow_y_scrollbar(),
                )
        });
    }

    /// 切过滤 —— 跳回第 1 页（跟 library.rs / sources.rs 同款）。
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
        // ---- 1. 统计各状态数量（按钮 label 后缀 + 过滤）----
        let counts = count_by_status(self.model.read(cx));

        // ---- 2. 按当前过滤筛选 + 排序 ----
        let indices = filter_and_sort_indices(self.model.read(cx), self.filter);

        // ---- 3. 复制 TaskSummary（避开 DownloadTask 不可 Clone）推给 delegate ----
        let summaries = build_summaries(self.model.read(cx), &indices);
        let total = summaries.len();

        // TODO：接入 list_cache。当前每帧 3 个 &AppModel 借用的 helper
        // 单独跑（count_by_status / filter_and_sort_indices /
        // build_summaries），结果没共享。Tasks 任务数少（通常 < 100），
        // TaskSummary 已是"已重"克隆，list_cache 收益小（最多 1ms → 0）；
        // 改造需把这 3 个 helper 改成"拿 &mut AppModel 一站式算"，改动
        // 风险 vs 收益不划算，故暂不接入。详见 git history 'list_cache
        // 接入' commit 后续。

        // ---- 4. 分页切片 + 兜底（过滤后 current_page 越界 → 回卷）----
        let w = compute_page_window(total, &mut self.current_page);
        let page_items: Vec<TaskSummary> = if w.is_empty() {
            Vec::new()
        } else {
            summaries[w.start..w.end].to_vec()
        };
        // 推给 delegate，List 渲染时读到。
        self.list_state.update(cx, |state, _cx| {
            state.delegate_mut().page_items = page_items;
        });

        let filter = self.filter;

        // ---- 5. 渲染 ----
        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            // Header：title + 新描述 subtitle，**无** action。
            .child(PageHeader::new(ts("Tasks.page_title")).subtitle(ts("Tasks.subtitle")))
            // 过滤按钮组：「全部 / 运行中 / 已完成 / 失败 / 已取消」，各带数量。
            // 跟 sources.rs 状态过滤同款：.small().ghost().selected(bool)。
            .child(toolbar::filter_buttons(self.filter, counts, cx))
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
                    cx.listener(|this, &new_page, _window, cx| {
                        this.current_page = new_page;
                        cx.notify();
                    }),
                ))
            })
    }
}
