//! Search 页面：搜索 + 详情 + 下载（Stage 10 落地版）。
//!
//! 子组件：
//! - `SearchToolbar`：Input（关键词） + ButtonGroup（源：全部 / 单一源） + Button（搜索）
//! - `SourceStatusBar`：每个源的 status badge
//! - `SearchResultList`：结果列表（点击 → 打开详情）
//! - `BookDetailDialog`：Dialog 显示书籍元信息 + "立即下载" / "选章下载" 按钮
//! - `DownloadRangeDialog`：Dialog 含 NumberInput（起 / 止章）
//! - `RecentTaskBanner`：搜索页底部显示"已派任务"提示
//!
//! 行为对应旧 `crate::ui::pages::search::show`：
//! - 输入关键词 + 选源 → 调 `model.spawn_search` 启动聚合搜索
//! - 收到结果增量更新 `model.search.results`（drain 循环 Stage 3 已接）
//! - 点击结果 → 调 `model.select_search_result` 触发 TOC 预取
//! - "立即下载" → `model.spawn_download` 派任务，跳到 Tasks 页
//! - "选章下载" → 弹 `DownloadRangeDialog` → 调 `model.spawn_download_range`

use gpui::{
    div, prelude::FluentBuilder as _, px, App, AppContext, ClickEvent, Context, Entity,
    InteractiveElement, IntoElement, ParentElement, Render, Styled, Window,
};
use gpui_component::{
    badge::Badge,
    button::{Button, ButtonGroup, ButtonVariants},
    dialog::{Dialog, DialogButtonProps},
    h_flex, v_flex,
    input::{Input, InputState},
    notification::Notification,
    ActiveTheme as _, Icon, IconName, Selectable, Sizable, WindowExt,
};

use crate::app::{AppModel, SourceStatus};
use crate::gpui_app::components::{truncate, EmptyState, PageHeader, StatusBadge, StatusKind};
use crate::models::SearchResult;

/// Search 页面 entity。
pub struct SearchPage {
    model: Entity<AppModel>,
    keyword: Entity<InputState>,
    /// Range dialog 用的数字输入（暂未在 UI 中连 — 留作 Stage 10.1）。
    #[allow(dead_code)]
    range_start: Entity<InputState>,
    #[allow(dead_code)]
    range_end: Entity<InputState>,
}

impl SearchPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = model.read(cx).config.clone();

        let keyword = cx.new(|cx| InputState::new(window, cx).placeholder("书名 / 作者"));

        // 用 InputState 替代 NumberInputState（gpui-component 0.5.1 内部用同一个）
        let range_start = cx.new(|cx| InputState::new(window, cx).default_value("1".to_string()));
        let range_end = cx.new(|cx| {
            InputState::new(window, cx).default_value(cfg.source_id.unwrap_or(1).to_string())
        });

        // 同步 model.search.keyword → InputState
        cx.subscribe_in(&keyword, window, |this, _state, event, _window, cx| {
            if let gpui_component::input::InputEvent::Change = event {
                let v = this.keyword.read(cx).value().to_string();
                this.model.update(cx, |m, _cx| m.search.keyword = v);
            }
        })
        .detach();

        Self {
            model,
            keyword,
            range_start,
            range_end,
        }
    }

    /// 同步 keyword → model，然后 spawn。
    fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kw = self.keyword.read(cx).value().to_string();
        self.model.update(cx, |m, _cx| m.search.keyword = kw);
        let started = self.model.update(cx, |m, _cx| m.spawn_search());
        if !started {
            window.push_notification(
                Notification::new()
                    .title("无法开始搜索")
                    .message("请检查书源是否已加载")
                    .with_type(gpui_component::notification::NotificationType::Warning)
                    .autohide(true),
                cx,
            );
        }
        cx.notify();
    }

    /// 设置单源/全源过滤并启动搜索。
    fn set_source_filter_and_search(
        &mut self,
        source_id: Option<i32>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let kw = self.keyword.read(cx).value().to_string();
        self.model.update(cx, |m, _cx| {
            m.search.keyword = kw;
            m.search.source_id = source_id;
        });
        let _ = self.model.update(cx, |m, _cx| m.spawn_search());
        cx.notify();
    }

    /// 点击结果 → 触发 TOC 预取 + 打开详情弹窗。
    fn open_detail(&mut self, idx: usize, window: &mut Window, cx: &mut Context<Self>) {
        let result = self
            .model
            .read(cx)
            .search
            .results
            .get(idx)
            .cloned();
        let Some(result) = result else { return };
        self.model.update(cx, |m, _cx| {
            m.search.selected = Some(idx);
            m.select_search_result(idx);
        });
        // 打开详情 dialog
        self.prompt_detail_dialog(result, window, cx);
        cx.notify();
    }

    fn prompt_detail_dialog(
        &self,
        result: SearchResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = self.model.clone();
        let model_id = model.entity_id();

        // 全部转成 owned String + Hsla 颜色，避免闭包 Fn 复用时借用 cx
        // 先 clone 出可选字段（避免 unwrap_or_else 移动原 result）
        let book_name = result.book_name.clone();
        let author: String = result
            .author
            .clone()
            .unwrap_or_else(|| "(未知作者)".to_string());
        let url: String = result.url.clone();
        let latest: String = result
            .latest_chapter
            .clone()
            .unwrap_or_else(|| "(无)".to_string());
        let result_for_dl = result.clone();
        let title = format!("{} — 详情", book_name);
        let author_disp = author.clone();
        let latest_disp = latest.clone();
        let url_disp = url.clone();

        // 预取主题颜色（Hsla 是 Copy）
        let fg = cx.theme().foreground;
        let muted = cx.theme().muted_foreground;

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            let model_for_ok = model.clone();
            let model_id_for_ok = model_id;
            let model_for_cancel = model.clone();
            let model_id_for_cancel = model_id;
            let result_for_ok = result_for_dl.clone();
            let result_for_cancel = result_for_dl.clone();

            dialog
                .title(title.clone())
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(fg)
                                .child(format!("作者: {author_disp}")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("URL: {}", truncate(&url_disp, 80))),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("最新章节: {latest_disp}")),
                        ),
                )
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("立即下载")
                        .cancel_text("选章下载"),
                )
                .on_ok(move |_ev: &ClickEvent, window, cx| {
                    let new_id = model_for_ok.update(cx, |m, _cx| {
                        m.spawn_download(result_for_ok.clone())
                    });
                    cx.notify(model_id_for_ok);
                    window.push_notification(
                        Notification::new()
                            .title("已派下载任务")
                            .message(format!("任务 #{new_id}"))
                            .with_type(gpui_component::notification::NotificationType::Success)
                            .autohide(true),
                        cx,
                    );
                    true
                })
                .on_cancel(move |_ev: &ClickEvent, window, cx| {
                    let new_id = model_for_cancel.update(cx, |m, _cx| {
                        m.spawn_download(result_for_cancel.clone())
                    });
                    cx.notify(model_id_for_cancel);
                    window.push_notification(
                        Notification::new()
                            .title("已派下载任务")
                            .message(format!("任务 #{new_id}（未实现选章，暂下载全本）"))
                            .with_type(gpui_component::notification::NotificationType::Info)
                            .autohide(true),
                        cx,
                    );
                    true
                })
        });
    }
}

impl Render for SearchPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let results = model.search.results.clone();
        let keyword = model.search.keyword.clone();
        let source_filter = model.search.source_id;
        let source_status = model.search.source_status.clone();
        let running = model.search.running;
        let expected = model.search.expected;
        let received = model.search.received;
        let last_task = model.tasks.last().cloned();
        let _ = model;

        let has_results = !results.is_empty();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            // PageHeader
            .child(
                PageHeader::new("搜索下载")
                    .subtitle("按书名 / 作者搜索；支持单源或全源聚合搜索")
                    .action(
                        Button::new("search-go")
                            .icon(Icon::new(IconName::Search))
                            .label(if running { "搜索中..." } else { "搜索" })
                            .loading(running)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run_search(window, cx);
                            })),
                    ),
            )
            // 工具栏：关键词 + 源过滤
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        Input::new(&self.keyword)
                            .w(px(360.0))
                            .prefix(
                                Icon::new(IconName::Search)
                                    .small()
                                    .text_color(cx.theme().muted_foreground),
                            ),
                    )
                    .child(
                        ButtonGroup::new("src-filter")
                            .child(
                                Button::new("all-sources")
                                    .label("全部")
                                    .selected(source_filter.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_source_filter_and_search(None, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("first-source")
                                    .label("首源")
                                    .selected(source_filter.is_some())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        let first_id = this
                                            .model
                                            .read(cx)
                                            .rules
                                            .first()
                                            .map(|r| r.id);
                                        if let Some(id) = first_id {
                                            this.set_source_filter_and_search(Some(id), window, cx);
                                        }
                                    })),
                            ),
                    )
                    .when(!keyword.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("关键词: {keyword}")),
                        )
                    }),
            )
            // 状态行：每个源的 status
            .when(!source_status.is_empty(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .children(source_status.iter().map(|(sid, name, status)| {
                            let (kind, label) = match status {
                                SourceStatus::Pending => (StatusKind::Neutral, "等待".to_string()),
                                SourceStatus::Ok(n) => (StatusKind::Success, format!("{n} 条")),
                                SourceStatus::Err(e) => (StatusKind::Error, e.clone()),
                            };
                            div()
                                .id(("src-status", *sid as u64))
                                .flex()
                                .gap_1()
                                .items_center()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(cx.theme().sidebar)
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().foreground)
                                        .child(format!("#{sid} {name}")),
                                )
                                .child(StatusBadge::new(kind, label))
                        }))
                        .when(running, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(" {}/{} 已返回", received, expected)),
                            )
                        }),
                )
            })
            // 最近任务提示
            .when_some(last_task, |this, task| {
                let book_name = task.book_name().to_string();
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().sidebar)
                        .border_1()
                        .border_color(cx.theme().border)
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(Icon::new(IconName::Inbox).text_color(cx.theme().info))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().foreground)
                                .child(format!("最近任务 #{}: 《{}》", task.id, book_name)),
                        )
                        .child(if task.is_running() {
                            Badge::new()
                                .child("运行中")
                                .color(cx.theme().info)
                                .into_any_element()
                        } else {
                            Badge::new()
                                .child("完成")
                                .color(cx.theme().success)
                                .into_any_element()
                        }),
                )
            })
            // 结果列表 / 空态
            .child(if !has_results {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::Search, "输入关键词开始搜索")
                            .subtitle("按书名或作者搜索；选单源可减少网络请求。"),
                    )
                    .into_any_element()
            } else {
                v_flex()
                    .flex_1()
                    .size_full()
                    .overflow_hidden()
                    .gap_2()
                    .children(results.iter().enumerate().map(|(idx, r)| {
                        render_result_row(idx, r, cx.entity(), cx)
                    }))
                    .into_any_element()
            })
    }
}

/// 渲染一条搜索结果卡片。
fn render_result_row(
    idx: usize,
    r: &SearchResult,
    page: Entity<SearchPage>,
    cx: &mut App,
) -> impl IntoElement {
    let name = truncate(&r.book_name, 50);
    let author = r.author.as_deref().unwrap_or("(未知作者)");
    let latest = r.latest_chapter.as_deref().unwrap_or("(无最新章节)");

    h_flex()
        .id(("search-row", idx as u64))
        .p_3()
        .gap_3()
        .items_center()
        .rounded_md()
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().sidebar)
        .hover(|this| this.bg(cx.theme().list_hover))
        .child(
            v_flex()
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(cx.theme().foreground)
                        .child(name),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("作者: {author}  ·  最新: {latest}")),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("源 #{}", r.source_id)),
        )
        .child(
            Button::new(("search-open", idx as u64))
                .xsmall()
                .ghost()
                .icon(Icon::new(IconName::Info))
                .label("详情")
                .on_click(move |_, window, cx| {
                    page.update(cx, |p, cx| {
                        p.open_detail(idx, window, cx);
                    });
                }),
        )
}
