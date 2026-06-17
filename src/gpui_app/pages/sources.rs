//! Sources 页面：书源管理（导入 / 启用禁用 / 健康检查 / 删除）。
//!
//! 布局完全对齐 `library.rs`（参考 task 27 ~ task 21 沉淀的模式）：
//! - PageHeader：标题 + 副标题 + 右侧 actions（添加 / 测速）
//! - Toolbar：filter Input + status Select（全部 / 启用 / 禁用）
//! - 统计行 + 探测进度（保留 —— 信息密度高，独立一行比塞进 subtitle 好）
//! - 错误提示横幅
//! - gpui-component `List` + delegate 虚拟滚动
//! - `Pagination` 页脚（每页 30 条）
//!
//! 删除走 `WindowExt::open_dialog` 二次确认。添加后 `model.add_sources_from_file` 内部已
//! 刷新内存 rules，下次 render 拿到新数据。

use std::collections::HashMap;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, Window,
};
use gpui_component::StyledExt;
use gpui_component::{
    button::{Button, ButtonVariant, ButtonVariants as _},
    dialog::{Dialog, DialogButtonProps},
    h_flex,
    input::{Input, InputEvent, InputState},
    list::{List, ListDelegate, ListItem, ListState},
    spinner::Spinner,
    switch::Switch,
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable, Icon, IconName, IndexPath, Selectable, Sizable,
    WindowExt,
};

use crate::app::{AppModel, SourcesFilterStatus};
use crate::crawler::health::SourceHealth;
use crate::gpui_app::components::{truncate, EmptyState, PageHeader, Pagination, StatusBadge};
use crate::gpui_app::i18n::{ts, ts_fmt};
use crate::models::Rule;

/// 每页条数 —— 跟 library.rs 保持一致（同一个 `Pagination` 组件）。
const PAGE_SIZE: usize = 30;

/// Sources 页面 entity。
pub struct SourcesPage {
    #[allow(dead_code)]
    model: Entity<AppModel>,

    /// 名字 / URL 关键字过滤 Input。struct 字段持有避免 click / focus 丢失
    /// （同 library filter_input / theme_state 模式）。
    ///
    /// **placeholder 必须在 InputState 上** —— gpui-component 0.5.1 的 `Input` element
    /// **没有** `.placeholder(...)` 方法，placeholder 只能从 `InputState.placeholder` 读
    /// （`element.rs:952-958` paint 时 `let placeholder = self.placeholder.clone()` 字段）。
    /// 所以 placeholder **一定**在 State 上 —— 这是 gpui-component 的 API 限制，
    /// 不能完全"避免 State 持有翻译"。但用 `last_seen_placeholder` sentinel 在 render
    /// 里**实时刷新**，切语言后下一帧自动更新。
    filter_input: Entity<InputState>,

    /// gpui-component 虚拟列表。
    list_state: Entity<ListState<SourcesDelegate>>,

    /// 当前 0-based 页码。UI-only，每次路径或过滤变化时重置为 0。
    current_page: usize,

    /// 实时 i18n sentinel：上次 render 时 `Sources.filter.placeholder` 的翻译结果。
    /// 切语言 → `ts()` 返回新值 → render 里检测到不一致 → `set_placeholder` 刷新。
    last_seen_placeholder: SharedString,
}

impl SourcesPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 1. 名字 / URL 过滤 Input。placeholder 在 state 上设初值（gpui-component API
        // 限制，element 层无 placeholder 字段），后续 render 里用 sentinel 检测切语言。
        let initial_placeholder = ts("Sources.filter.placeholder");
        let filter_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(initial_placeholder.clone()));
        cx.subscribe_in(&filter_input, window, |this, _state, ev, _window, cx| {
            if matches!(ev, InputEvent::Change) {
                let v = this.filter_input.read(cx).value();
                this.model.update(cx, |m, _cx| {
                    m.sources_state.filter_text = v.to_string();
                });
                // 关键字变了 → 跳回第 1 页（避免卡在已不存在的页码上）。
                this.current_page = 0;
                cx.notify();
            }
        })
        .detach();

        // 2. 状态过滤 —— 不走 SelectState（SelectState 持有 options 翻译字段，
        // 切语言会失效）。改用**自定义按钮组**：3 个 Button，`selected` 状态从
        // `model.sources_state.filter_status` 读，label 在 render 里现取 `ts(...)`。
        // 切语言自动同步。

        // 3. List + Delegate。
        let page_handle = cx.entity().clone();
        let delegate = SourcesDelegate::new(page_handle);
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx));

        Self {
            model,
            filter_input,
            list_state,
            current_page: 0,
            last_seen_placeholder: initial_placeholder,
        }
    }

    /// 点"删除"按钮 → 弹 Dialog 二次确认。
    fn prompt_delete(&mut self, source_id: i32, window: &mut Window, cx: &mut App) {
        let model = self.model.clone();
        let model_id = model.entity_id();

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            let model_for_ok = model.clone();
            let model_id_for_ok = model_id;
            let source_id_for_ok = source_id;

            dialog
                .title(ts("Sources.delete_dialog.title"))
                .child(div().child(ts_fmt(
                    "Sources.delete_dialog.message",
                    &[("source_id", &source_id_for_ok.to_string())],
                )))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(ts("Sources.delete_dialog.confirm"))
                        .cancel_text(ts("Sources.delete_dialog.cancel"))
                        .ok_variant(ButtonVariant::Danger),
                )
                .confirm()
                .on_ok(move |_ev: &ClickEvent, _window, cx| {
                    model_for_ok.update(cx, |m, _cx| {
                        m.delete_source(source_id_for_ok);
                    });
                    cx.notify(model_id_for_ok);
                    true
                })
        });
    }

    /// 调 `rfd` 文件选择器选 JSON 文件，调 `add_sources_from_file`。
    ///
    /// 用 `rfd::AsyncFileDialog` —— 内部走 `tokio::task::spawn_blocking`，
    /// dialog 在 tokio 专门的 blocking thread pool 上跑，正确初始化 COM
    /// apartment + message pump。
    ///
    /// **别用同步 `rfd::FileDialog::pick_file()` 丢 `cx.background_executor().spawn`
    /// 上** —— Windows 下 `IFileOpenDialog::Show()` 需要 STA + message pump，
    /// tokio worker thread 都没有，`Show()` 静默失败立即返回 None 且 dialog
    /// 不显示。
    fn pick_and_add(&mut self, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let page_handle = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            // rfd 弹原生 OS 文件选择器 —— 三个标签（对话框标题 + 两个 filter 名字）
            // 都走 `ts()` 翻译，跟 app 其他用户可见文本保持一致。
            // `.as_ref()` 把 `SharedString` → `&str`（rfd 0.15 的 `add_filter` / `set_title`
            // 签名是 `&str`，不接受 owned `String` / `SharedString`）。
            let file = rfd::AsyncFileDialog::new()
                .add_filter(
                    ts("Sources.add_source.filter_json").as_ref(),
                    &["json", "json5"],
                )
                .add_filter(ts("Sources.add_source.filter_all").as_ref(), &["*"])
                .set_title(ts("Sources.add_source.dialog_title").as_ref())
                .pick_file()
                .await;
            if let Some(file) = file {
                let path = file.path().to_path_buf();
                let _ = page_handle.update(async_cx, |_page, cx| {
                    model.update(cx, |m, _cx| {
                        m.add_sources_from_file(&path);
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn run_health_check(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| m.spawn_health_check());
        cx.notify();
    }

    /// 设置状态过滤（全部 / 启用 / 禁用）。跳回第 1 页（同 library ext_filter 行为）。
    fn set_status_filter(&mut self, new_status: SourcesFilterStatus, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| {
            m.sources_state.filter_status = new_status;
        });
        self.current_page = 0;
        cx.notify();
    }
}

// 需要 `InputEvent` 和 `InputState` —— 已经在顶部 use cluster 里导入。

impl Render for SourcesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 实时 i18n 同步（仅 placeholder —— gpui-component 0.5.1 API 限制必须存在 State）。
        //
        // 状态过滤不走 sentinel（已经用 button group，label 现取 `ts(...)`）。
        //
        // `set_placeholder` 内部 `cx.notify()` 只通知 InputState 重新 render。
        // 但 `Input` 元素是在 SourcesPage render 时构造的，LibraryPage 不重
        // render，Input 元素就不重画。这里额外 `cx.notify()` 强制 SourcesPage 重
        // render，触发 Input 重构造 → 读取 InputState 的最新 placeholder 渲染。
        let new_placeholder = ts("Sources.filter.placeholder");
        if self.last_seen_placeholder != new_placeholder {
            self.last_seen_placeholder = new_placeholder.clone();
            self.filter_input.update(cx, |state, cx| {
                state.set_placeholder(new_placeholder, window, cx);
            });
            // 强制 SourcesPage 重 render，新 placeholder 立刻可见。
            cx.notify();
        }

        let model = self.model.read(cx);
        let total_rules = model.rules.len();
        let disabled = model.rules.iter().filter(|r| r.disabled).count();
        let enabled = total_rules - disabled;

        // "可用"取上次 health-check 的结果
        let available_after_check =
            if !model.sources_state.health.is_empty() && !model.sources_state.running {
                Some(
                    model
                        .sources_state
                        .health
                        .values()
                        .filter(|h| {
                            h.error.is_none()
                                && matches!(h.http_status, Some(s) if (200..400).contains(&s))
                        })
                        .count(),
                )
            } else {
                None
            };

        let rule_load_error = model.rule_load_error.clone();
        let running = model.sources_state.running;
        let received = model.sources_state.received;
        let expected = model.sources_state.expected;
        let health = model.sources_state.health.clone();
        let all_rules = model.rules.clone();
        let _ = model;

        // 过滤后取当前页切片 —— 跟 library.rs 同模式（global 序号 + 切片 + 推给 delegate）。
        let filtered = self.model.read(cx).sources_state.filtered_rules(&all_rules);
        let total = filtered.len();
        let page_count = total.div_ceil(PAGE_SIZE);
        if page_count > 0 && self.current_page >= page_count {
            self.current_page = page_count - 1;
        }
        let start = self.current_page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(total);
        let page_items: Vec<(usize, Rule)> = if total == 0 {
            Vec::new()
        } else {
            filtered[start..end]
                .iter()
                .enumerate()
                .map(|(local_ix, r)| (start + local_ix, r.clone()))
                .collect()
        };

        // 推给 delegate（包括 health map，让 row 渲染时拿到健康状态）。
        let health_for_delegate = health.clone();
        let page_handle = cx.entity().clone();
        self.list_state.update(cx, |state, _cx| {
            let d = state.delegate_mut();
            d.page_items = page_items;
            d.health = health_for_delegate;
            d.page_handle = page_handle;
        });

        v_flex()
            .size_full()
            .p_6()
            .gap_3()
            // ---- PageHeader：标题 + 副标题 + 右侧 actions ----
            .child(
                PageHeader::new(ts("Sources.page_title"))
                    .subtitle(ts("Sources.page_subtitle"))
                    .action(
                        Button::new("add-source")
                            .icon(Icon::new(IconName::Plus))
                            .label(ts("Sources.action.add"))
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.pick_and_add(cx);
                            })),
                    )
                    .action(
                        Button::new("health-check")
                            .icon(Icon::new(IconName::Loader))
                            .label(ts("Sources.action.health_check"))
                            .loading(running)
                            .disabled(running || total_rules == 0)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.run_health_check(cx);
                            })),
                    ),
            )
            // ---- toolbar: 名字过滤 + 状态过滤 ----
            //
            // **状态过滤**用 3 个 Button 而不是 SelectState —— 原因：
            // SelectState 把 options 翻译字段冻在 state 里，切语言不会自动更新。
            // Button 组在 render 里现取 `ts(...)`，永远跟当前 locale 同步。状态
            // 用 `selected` style 标记，存的是 enum 不带翻译。
            .child({
                let current_status = self.model.read(cx).sources_state.filter_status;
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        // 名字过滤：placeholder 在 InputState 上（API 限制 —— `Input`
                        // element 没有 `.placeholder(...)` 方法）。实时 i18n 由
                        // render 顶部的 sentinel 检测 + `set_placeholder` 刷新。
                        Input::new(&self.filter_input).w(px(280.0)).prefix(
                            Icon::new(IconName::Search)
                                .small()
                                .text_color(cx.theme().muted_foreground),
                        ),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Button::new("status-all")
                                    .small()
                                    .ghost()
                                    .selected(current_status == SourcesFilterStatus::All)
                                    .label(ts("Sources.status.all"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.set_status_filter(SourcesFilterStatus::All, cx);
                                    })),
                            )
                            .child(
                                Button::new("status-enabled")
                                    .small()
                                    .ghost()
                                    .selected(current_status == SourcesFilterStatus::Enabled)
                                    .label(ts("Sources.status.enabled"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.set_status_filter(SourcesFilterStatus::Enabled, cx);
                                    })),
                            )
                            .child(
                                Button::new("status-disabled")
                                    .small()
                                    .ghost()
                                    .selected(current_status == SourcesFilterStatus::Disabled)
                                    .label(ts("Sources.status.disabled"))
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.set_status_filter(SourcesFilterStatus::Disabled, cx);
                                    })),
                            ),
                    )
            })
            // ---- 统计 + 进度 ----
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Tag::secondary()
                            .small()
                            .child(format!("{} {}", ts("Sources.stat.total"), total_rules)),
                    )
                    .when(enabled > 0, |this| {
                        this.child(
                            Tag::success()
                                .small()
                                .child(format!("{} {}", ts("Sources.stat.enabled"), enabled)),
                        )
                    })
                    .when(disabled > 0, |this| {
                        this.child(
                            Tag::warning()
                                .small()
                                .child(format!("{} {}", ts("Sources.stat.disabled"), disabled)),
                        )
                    })
                    .when_some(available_after_check, |this, n| {
                        this.child(
                            Tag::info()
                                .small()
                                .child(format!("{} {}", ts("Sources.stat.available"), n)),
                        )
                    })
                    .when(running, |this| {
                        this.child(Spinner::new().small()).child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{}/{} {}",
                                    received,
                                    expected,
                                    ts("Sources.health.progress")
                                )),
                        )
                    }),
            )
            // ---- 错误提示 ----
            .when_some(rule_load_error, |this, err| {
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .child(format!("{}: {err}", ts("Sources.error.load_failed"))),
                )
            })
            // ---- list / 空态 ----
            .child(if total == 0 {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::Globe, ts("Sources.empty.title"))
                            .subtitle(ts("Sources.empty.subtitle")),
                    )
                    .into_any_element()
            } else {
                // List 容器：跟 library.rs 同款（border + .px(12).py(4) +
                // List::new().size_full()），让选中边框不被滚动条遮挡。
                div()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .child(
                        List::new(&self.list_state)
                            .px(px(12.))
                            .py(px(4.))
                            .size_full(),
                    )
                    .into_any_element()
            })
            // ---- 分页页脚 ----
            // 永远渲染 —— Sources 通常 < 30 条，page_count=1，prev/next disabled，
            // 单个数字按钮 "1" 高亮，给用户"完整列表已展示"的视觉锚点。
            .child(Pagination::new(
                self.current_page,
                page_count,
                cx.listener(|this, &new_page, _window, _cx| {
                    this.current_page = new_page;
                    _cx.notify();
                }),
            ))
    }
}

// 删除走 Dialog 二次确认（点「删除」→ 弹 dialog → 确认才真删）—— 跟 library.rs
// `prompt_delete` 完全对齐：SourcesPage 持有 `prompt_delete(source_id, window, cx)`
// 方法，row 的删除按钮直接转发到 page。
//
// 设计要点（参考 `crates/story/src/stories/list_story.rs:88-93` 的 CompanyListItem）：
// - 选中态交给 ListItem：`.selected(...)` 内置 `list_active` bg + `list_active_border`
// - 圆角交给 ListItem：`.rounded(cx.theme().radius)` 让选中边框自动 round
// - 不要 `.hover(|this| this.bg(list_hover))` / `.border_b_1()`：ListItem paint 已提供
pub struct SourcesDelegate {
    /// 当前页要展示的条目，每条带"全局序号"（在完整 filtered 列表里的 0-based 位置）。
    /// 跨分页连续：page 0 → 0..29，page 1 → 30..59，等等。显示时 +1 变 1-based 给人看。
    page_items: Vec<(usize, Rule)>,
    /// source_id → 探测结果（AppModel.sources_state.health 的快照）。
    /// 每次 render 推过来 —— row 渲染时按 `rule.id` 查找。
    health: HashMap<i32, SourceHealth>,
    /// 当前选中项（List 内置 hover / selected 样式管理）。
    selected_index: Option<IndexPath>,
    /// 拿 SourcesPage handle 用于删除按钮 → `prompt_delete` 转发。
    page_handle: Entity<SourcesPage>,
}

impl SourcesDelegate {
    fn new(page_handle: Entity<SourcesPage>) -> Self {
        Self {
            page_items: Vec::new(),
            health: HashMap::new(),
            selected_index: None,
            page_handle,
        }
    }
}

impl ListDelegate for SourcesDelegate {
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
        let (global_index, rule) = self.page_items.get(ix.row)?.clone();
        let health_status = self.health.get(&rule.id).cloned();
        Some(
            ListItem::new(ix)
                .selected(Some(ix) == self.selected_index)
                .rounded(cx.theme().radius)
                .mb(px(4.))
                .child(render_source_row(
                    global_index,
                    &rule,
                    health_status.as_ref(),
                    self.page_handle.clone(),
                    &mut *cx,
                )),
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

/// 渲染一条书源行（5 列：序号 / name + lang tag / url flex_1 / Switch / StatusBadge / Delete）。
///
/// 跟 library.rs::render_row 同模式：
/// - 序号列 48px，右对齐 muted
/// - 书名 + 语言 tag 列 flex_1 撑满剩余（短名也撑满，tag 紧贴书名右侧）
/// - URL 列自然宽度（不够 truncate）
/// - Switch 80px
/// - StatusBadge 90px
/// - Delete 按钮 100px
///
/// 宽度总和（不含 flex_1 的 book/url）：48 + 80 + 90 + 100 + 4×gap ≈ 360 + gap。
/// 1200px 窗口下 book + url 拿到 ~800px，足够显示大多数 URL。
fn render_source_row(
    index: usize,
    rule: &Rule,
    health: Option<&SourceHealth>,
    page: Entity<SourcesPage>,
    cx: &mut App,
) -> impl IntoElement {
    let name = truncate(&rule.name, 30);
    let lang_display = if rule.language.is_empty() {
        SharedString::from("--")
    } else {
        SharedString::from(rule.language.to_uppercase())
    };
    // 是否需要代理
    let need_proxy = rule.need_proxy;

    h_flex()
        .px_2()
        .py_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .items_center()
        // ---- 序号列 ----
        .child(
            div()
                .w(px(48.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("#{}", index + 1)),
        )
        // ---- 书名 + 语言 tag ----
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.))
                .items_center()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .overflow_x_hidden()
                        .text_sm()
                        .text_color(cx.theme().foreground)
                        .child(div().whitespace_nowrap().text_ellipsis().child(name))
                        .child(
                            div()
                                .h_flex()
                                .items_center()
                                .gap_1()
                                .child(Tag::secondary().small().child(lang_display))
                                .when(need_proxy, |d| {
                                    d.child(Tag::secondary().small().child(ts("Sources.tag.proxy")))
                                }),
                        ),
                ),
        )
        // ---- URL ----
        .child(
            div()
                .w(px(250.))
                .overflow_x_hidden()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(truncate(&rule.url, 60)),
                ),
        )
        // ---- 健康状态 Badge ----
        .child(div().w(px(150.)).justify_end().child({
            let badge_kind = health_status_kind(health);
            let label = health_status_label(health);
            StatusBadge::new(badge_kind, label)
        }))
        // ---- 启用开关 ----
        .child({
            let page_for_switch = page.clone();
            let rule_id = rule.id;
            Switch::new(("src-switch", index as u64))
                .checked(!rule.disabled)
                .on_click(move |checked, _window, cx| {
                    let want_disabled = !*checked;
                    page_for_switch.update(cx, |p, cx| {
                        p.model.update(cx, |m, _cx| {
                            // 只在 model 当前状态与 UI 期望不一致时才 toggle（避免重复触发）。
                            if m.rules.iter().find(|r| r.id == rule_id).map(|r| r.disabled)
                                != Some(want_disabled)
                            {
                                m.toggle_source_disabled(rule_id);
                            }
                        });
                    });
                })
        })
        // ---- 删除按钮（点一次弹 Dialog 二次确认 —— 跟 library.rs `prompt_delete` 同模式）----
        .child({
            let page_for_del = page.clone();
            let rule_id = rule.id;
            Button::new(("src-del", index as u64))
                .small()
                .danger()
                .icon(Icon::new(IconName::Delete))
                .label(ts("Sources.action.delete"))
                .on_click(move |_, window, cx| {
                    page_for_del.update(cx, |p, cx| {
                        p.prompt_delete(rule_id, window, cx);
                    });
                })
        })
}

/// 健康状态 → 语义色枚举。
fn health_status_kind(h: Option<&SourceHealth>) -> crate::gpui_app::components::StatusKind {
    use crate::gpui_app::components::StatusKind as K;
    match h {
        None => K::Neutral,
        Some(h) if h.error.is_some() => K::Error,
        Some(h) => match h.http_status {
            Some(s) if (200..300).contains(&s) => K::Success,
            Some(s) if (300..400).contains(&s) => K::Info,
            Some(_) => K::Warning,
            None => K::Warning,
        },
    }
}

/// 健康状态 → 显示文本。
fn health_status_label(h: Option<&SourceHealth>) -> String {
    match h {
        None => ts("Sources.health.not_tested").to_string(),
        Some(h) if h.error.is_some() => ts("Sources.health.error").to_string(),
        Some(h) => match h.http_status {
            // HTTP 200 / HTTP 404 等 —— `ts_fmt` 替换 {status} 占位符（不能直接
            // `format!` 拼字符串，否则切语言后占位符翻译也跟着拼，顺序会乱）。
            Some(s) => {
                ts_fmt("Sources.health.http_status", &[("status", &s.to_string())]).to_string()
            }
            // 源错误但没 HTTP 响应（DNS 失败 / 超时 等）—— 调试输出太长塞不进 StatusBadge，
            // 用一句"网络错误"代替。原来的 `format!("{:?}", h.error)` 会把 anyhow 内部
            // chain 全部展开，超长且对用户没意义。
            None => ts("Sources.health.network_error").to_string(),
        },
    }
}
