//! Sources 页面：书源管理（导入 / 启用禁用 / 健康检查）。

mod delegate;
mod row;
mod toolbar;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    div, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, IconName, Sizable,
    button::Button,
    h_flex,
    input::{InputEvent, InputState},
    list::{List, ListState},
    select::{SearchableVec, SelectEvent, SelectState},
    spinner::Spinner,
    tag::Tag,
    v_flex,
};

use crate::app::{AppModel, SourcesFilterStatus};
use crate::db::list_rule_files;
use crate::gpui_app::components::{EmptyState, PageHeader, Pagination, compute_page_window};
use crate::i18n::ts;
use crate::models::Rule;

use self::delegate::SourcesDelegate;

/// Sources 页面 entity。
pub struct SourcesPage {
    model: Entity<AppModel>,

    /// 名字 / URL 关键字过滤 Input。struct 字段持有避免 click / focus 丢失
    filter_input: Entity<InputState>,

    /// 选择活跃书源文件的下拉框。
    rule_file_select: Entity<SelectState<SearchableVec<String>>>,

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

        // 2. 选择活跃书源文件的下拉框。items 首次为空：render 第一次跑时会从
        // `rules_dir` 重建。这条路径处理用户手动添加/删除规则文件后的刷新。
        let items: SearchableVec<String> = Vec::<String>::new().into();
        let rule_file_select =
            cx.new(|cx| SelectState::new(items, None, window, cx).searchable(false));
        let model_for_select = model.clone();
        cx.subscribe_in(
            &rule_file_select,
            window,
            move |_this, _state, ev, _w, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    let filename = value.clone();
                    model_for_select.update(cx, |m, _cx| {
                        m.switch_active_file(&filename);
                    });
                    cx.notify();
                }
            },
        )
        .detach();

        // 3. List + Delegate。
        let page_handle = cx.entity();
        let delegate = SourcesDelegate::new(page_handle);
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx));

        Self {
            model,
            filter_input,
            rule_file_select,
            list_state,
            current_page: 0,
            last_seen_placeholder: initial_placeholder,
        }
    }

    /// 调 `rfd` 文件选择器选 JSON 文件，调 `add_sources_from_file`。
    fn pick_and_add(&self, cx: &Context<Self>) {
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

    fn run_health_check(&self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| m.spawn_health_check());
        cx.notify();
    }

    /// 设置状态过滤（全部 / 启用 / 禁用）。跳回第 1 页（同 library `ext_filter` 行为）。
    pub(super) fn set_status_filter(
        &mut self,
        new_status: SourcesFilterStatus,
        cx: &mut Context<Self>,
    ) {
        self.model.update(cx, |m, _cx| {
            m.sources_state.filter_status = new_status;
        });
        self.current_page = 0;
        cx.notify();
    }
}

impl Render for SourcesPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 实时 i18n 同步（仅 placeholder —— gpui-component 0.5.1 API 限制必须存在 State）。
        //
        // 状态过滤不走 sentinel（已经用 button group，label 现取 `ts(...)`）。
        //
        // `set_placeholder` 内部 `cx.notify()` 只通知 InputState 重新 render。
        // 但 `Input` 元素是在 SourcesPage render 时构造的，SourcesPage 不重
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
        let current_status = model.sources_state.filter_status;
        let active_file = model.sources_config.active_file.clone();
        let rules_dir = model.paths.rules_dir.clone();
        let _ = model;

        // 同步活跃书源文件下拉框选项。
        // 每次 render 都重新读取 rules_dir（用户可能手动添加/删除了文件）。
        let rule_files = list_rule_files(&rules_dir);
        let items: SearchableVec<String> = rule_files.into();
        let sel = active_file;
        let pos = <SearchableVec<String> as gpui_component::select::SelectDelegate>::position(
            &items, &sel,
        );
        self.rule_file_select.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            if let Some(p) = pos {
                state.set_selected_index(Some(p), window, cx);
            }
        });

        // 过滤后取当前页切片 —— 跟 library.rs 同模式（global 序号 + 切片 + 推给 delegate）。
        let filtered = self.model.read(cx).sources_state.filtered_rules(&all_rules);
        let total = filtered.len();
        let w = compute_page_window(total, &mut self.current_page);
        let page_items: Vec<(usize, Rule)> = if total == 0 {
            Vec::new()
        } else {
            filtered[w.start..w.end]
                .iter()
                .enumerate()
                .map(|(local_ix, r)| (w.start + local_ix, r.clone()))
                .collect()
        };

        // 推给 delegate（包括 health map，让 row 渲染时拿到健康状态）。
        let health_for_delegate = health;
        let page_handle = cx.entity();
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
            // ---- toolbar: 名字过滤 + 活跃书源文件选择 + 状态过滤 ----
            //
            // **状态过滤**用 3 个 Button 而不是 SelectState —— 原因：
            // SelectState 把 options 翻译字段冻在 state 里，切语言不会自动更新。
            // Button 组在 render 里现取 `ts(...)`，永远跟当前 locale 同步。状态
            // 用 `selected` style 标记，存的是 enum 不带翻译。
            .child(toolbar::render(
                &self.filter_input,
                &self.rule_file_select,
                current_status,
                cx,
            ))
            // ---- 统计 + 进度 ----
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(Tag::secondary().small().child(format!(
                        "{} {}",
                        ts("Sources.stat.total"),
                        total_rules
                    )))
                    .when(enabled > 0, |this| {
                        this.child(Tag::success().small().child(format!(
                            "{} {}",
                            ts("Sources.stat.enabled"),
                            enabled
                        )))
                    })
                    .when(disabled > 0, |this| {
                        this.child(Tag::warning().small().child(format!(
                            "{} {}",
                            ts("Sources.stat.disabled"),
                            disabled
                        )))
                    })
                    .when_some(available_after_check, |this, n| {
                        this.child(Tag::info().small().child(format!(
                            "{} {}",
                            ts("Sources.stat.available"),
                            n
                        )))
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
                    .child(List::new(&self.list_state).p(px(12.)).size_full())
                    .into_any_element()
            })
            // ---- 分页页脚（仅在列表非空时渲染 —— 空态不显示，避免无意义的"第 1 页 / 共 0 条"）----
            // Sources 通常 < 30 条，page_count=1，prev/next disabled，
            // 单个数字按钮 "1" 高亮，给用户"完整列表已展示"的视觉锚点。
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
