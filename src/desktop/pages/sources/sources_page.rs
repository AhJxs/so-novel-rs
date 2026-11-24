use std::sync::Arc;

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::list::{List, ListState};
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Disableable, Icon, IconName, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    div, px,
};
use rust_i18n::t;

use super::delegate::SourcesDelegate;
use crate::db::list_rule_files;
use crate::desktop::components::{EmptyState, PageHeader, Pagination, compute_page_window};
use crate::desktop::model::{AppModel, SourcesFilterStatus};
use crate::models::Rule;

/// Sources 页面 entity。
pub struct SourcesPage {
    pub(super) model: Entity<AppModel>,

    /// 名字 / URL 关键字过滤 Input。struct 字段持有避免 click / focus 丢失
    filter_input: Entity<InputState>,

    /// 选择活跃书源文件的下拉框。
    rule_file_select: Entity<SelectState<SearchableVec<String>>>,

    /// gpui-kit 组件库的虚拟列表。
    list_state: Entity<ListState<SourcesDelegate>>,

    /// 当前 0-based 页码。UI-only，每次路径或过滤变化时重置为 0。
    current_page: usize,

    /// 实时 i18n sentinel：上次 render 时 `Sources.filter.placeholder` 的翻译结果。
    /// 切语言 → `t!()` 返回新值 → render 里检测到不一致 → `set_placeholder` 刷新。
    last_seen_placeholder: SharedString,

    /// 上次同步给下拉框的规则文件名快照。`None` = 尚未同步。
    ///
    /// 不能每帧重新扫目录：`list_rule_files` 会 `read_dir` + 逐项 `is_file()`，而
    /// `set_items` / `set_selected_index` 内部都会 `cx.notify()`，写在 render 里会形成
    /// "通知 → 再排一帧 → 再读盘" 的自激循环，还会把下拉框每帧滚回选中项。
    last_rule_files: Option<Vec<String>>,

    /// 上次同步时的活跃规则文件名。活动文件可能已不在目录里（被删），单独比对，
    /// 保证"切换活跃文件"即使文件列表没变也能更新选中项。
    last_active_file: Option<String>,

    /// 上次观察到健康检查是否在跑。只在 `true → false` 那次通知里刷新规则文件，
    /// 既覆盖"检查完成后 drain"这个事件点，又不需要在 render 里读盘或轮询。
    last_health_running: bool,
}

impl SourcesPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 1. 名字 / URL 过滤 Input。placeholder 只能设在 state 上（gpui-kit API 限制），
        // 后续 render 里用 sentinel 检测切语言。
        let initial_placeholder: SharedString = t!("Sources.filter.placeholder").into();
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

        // 2. 活跃书源文件下拉框。items 首次为空，render 第一次跑时从 `rules_dir` 重建
        // （用户手动增删规则文件后也靠这条路径刷新）。
        let items: SearchableVec<String> = Vec::<String>::new().into();
        let rule_file_select =
            cx.new(|cx| SelectState::new(items, None, window, cx).searchable(false));
        let model_for_select = model.clone();
        cx.subscribe_in(
            &rule_file_select,
            window,
            move |this, _state, ev, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = ev {
                    let filename = value.clone();
                    model_for_select.update(cx, |m, _cx| {
                        m.switch_active_file(&filename);
                    });
                    // 切换后重算选中项（文件可能已删除，`position` 返回 None 时保留旧选中）。
                    this.sync_rule_files(window, cx);
                    cx.notify();
                }
            },
        )
        .detach();

        // 3. List + Delegate。
        let page_handle = cx.entity();
        let delegate = SourcesDelegate::new(page_handle);
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx));

        // 4. 健康检查完成的唯一信号来自 model 通知：`drain` 改完 `running` 后
        //    `ctx.notify()` 模型，这里在 `true → false` 那次重扫一次规则目录。
        //    用 `observe_in` 而不是 render 检测，是因为它带 `&mut Window`，刷新
        //    下拉框不必退回渲染期副作用。
        cx.observe_in(&model, window, |this, model, window, cx| {
            let running = model.read(cx).sources_state.running;
            if this.last_health_running && !running {
                this.sync_rule_files(window, cx);
            }
            this.last_health_running = running;
        })
        .detach();

        let mut page = Self {
            model,
            filter_input,
            rule_file_select,
            list_state,
            current_page: 0,
            last_seen_placeholder: initial_placeholder,
            last_rule_files: None,
            last_active_file: None,
            last_health_running: false,
        };
        // 构造期是首个"能保证用户一进页面就看到正确选项"的非渲染点。
        page.sync_rule_files(window, cx);
        page
    }

    /// 把 `rules_dir` 下的规则文件名同步进下拉框，快照没变就完全跳过。
    ///
    /// 只在真正会改变规则目录 / 活跃文件的事件点调用（构造、导入、健康检查完成、
    /// 切换活跃文件）。`set_items` / `set_selected_index` 内都会 `cx.notify()`，
    /// 放在 render 里会"通知 → 再排一帧 → 再读盘"自激，并把用户滚动位置拽回选中项。
    fn sync_rule_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (rules_dir, active_file) = {
            let model = self.model.read(cx);
            (
                model.paths.rules_dir.clone(),
                model.sources_config.active_file.clone(),
            )
        };
        let files = list_rule_files(&rules_dir);
        let files_changed = self.last_rule_files.as_deref() != Some(files.as_slice());
        let active_changed = self.last_active_file.as_deref() != Some(active_file.as_str());
        if !files_changed && !active_changed {
            return;
        }
        self.last_rule_files = Some(files.clone());
        self.last_active_file = Some(active_file.clone());

        let items: SearchableVec<String> = files.into();
        let pos = <SearchableVec<String> as gpui_kit::component::select::SelectDelegate>::position(
            &items,
            &active_file,
        );
        self.rule_file_select.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            if let Some(p) = pos {
                state.set_selected_index(Some(p), window, cx);
            }
        });
    }

    /// 调 `rfd` 文件选择器选 JSON 文件，调 `add_sources_from_file`。
    ///
    /// 导入会新写入文件或重载活跃规则，因此完成后要刷新下拉框；异步回调里没有
    /// `&mut Window`，用 `spawn_in` 拿 `AsyncWindowContext`，在 `update_in` 里
    /// 一并完成 model 写入与下拉刷新（`update_in` 会提供 window）。
    fn pick_and_add(window: &Window, cx: &Context<Self>) {
        let page_handle = cx.entity().downgrade();
        cx.spawn_in(window, async move |_weak, async_cx| {
            // 三个标签（对话框标题 + 两个 filter 名字）都走 `t!()` 翻译。
            // `.as_ref()` 把 `SharedString` → `&str`（rfd 的 `add_filter` / `set_title` 要 `&str`）。
            let file = rfd::AsyncFileDialog::new()
                .add_filter(
                    t!("Sources.add_source.filter_json").as_ref(),
                    &["json", "json5"],
                )
                .add_filter(t!("Sources.add_source.filter_all").as_ref(), &["*"])
                .set_title(t!("Sources.add_source.dialog_title").as_ref())
                .pick_file()
                .await;
            if let Some(file) = file {
                let path = file.path().to_path_buf();
                let _ = page_handle.update_in(async_cx, |page, window, cx| {
                    page.model.update(cx, |m, _cx| {
                        m.add_sources_from_file(&path);
                    });
                    page.sync_rule_files(window, cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn run_health_check(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| m.spawn_health_check());
        // 检查可能在同一轮 drain 里全部完成（还没通知过 running=true）。先把当前
        // 状态记进快照，observer 才能在之后那次通知里识别 true → false。
        self.last_health_running = self.model.read(cx).sources_state.running;
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
        // 实时 i18n 同步（仅 placeholder）。`set_placeholder` 只通知 InputState 重 render，
        // 而 `Input` 元素是 SourcesPage render 时构造的，故这里额外 `cx.notify()` 强制重 render。
        let new_placeholder: SharedString = t!("Sources.filter.placeholder").into();
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
        let _ = model;

        let filtered = self
            .model
            .read(cx)
            .sources_state
            .filtered_rule_indices(&all_rules);
        let total = filtered.len();
        let w = compute_page_window(total, &mut self.current_page);
        // 过滤只产出下标；只有当前页的项才做 `Arc` 浅拷贝给 delegate。
        let page_items: Vec<(usize, Arc<Rule>)> = if total == 0 {
            Vec::new()
        } else {
            filtered[w.start..w.end]
                .iter()
                .enumerate()
                .map(|(local_ix, &i)| (w.start + local_ix, Arc::clone(&all_rules[i])))
                .collect()
        };

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
                PageHeader::new(t!("Sources.page_title").into())
                    .subtitle(t!("Sources.page_subtitle").into())
                    .action(
                        Button::new("add-source")
                            .icon(Icon::new(IconName::Plus))
                            .label(t!("Sources.action.add"))
                            .on_click(cx.listener(|_this, _, window, cx| {
                                Self::pick_and_add(window, cx);
                            })),
                    )
                    .action(
                        Button::new("health-check")
                            .icon(Icon::new(IconName::Loader))
                            .label(t!("Sources.action.health_check"))
                            .loading(running)
                            .disabled(running || total_rules == 0)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.run_health_check(cx);
                            })),
                    ),
            )
            // ---- toolbar: 名字过滤 + 活跃书源文件选择 + 状态过滤 ----
            // 状态过滤用 3 个 Button 而不是 SelectState：后者把 options 的翻译字段冻在 state 里，
            // 切语言不更新；Button 组在 render 里现取 `t!(...)`。
            .child(super::toolbar::render(
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
                        t!("Sources.stat.total"),
                        total_rules
                    )))
                    .when(enabled > 0, |this| {
                        this.child(Tag::success().small().child(format!(
                            "{} {}",
                            t!("Sources.stat.enabled"),
                            enabled
                        )))
                    })
                    .when(disabled > 0, |this| {
                        this.child(Tag::warning().small().child(format!(
                            "{} {}",
                            t!("Sources.stat.disabled"),
                            disabled
                        )))
                    })
                    .when_some(available_after_check, |this, n| {
                        this.child(Tag::info().small().child(format!(
                            "{} {}",
                            t!("Sources.stat.available"),
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
                                    t!("Sources.health.progress")
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
                        .child(format!("{}: {err}", t!("Sources.error.load_failed"))),
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
                        EmptyState::new(IconName::Globe, t!("Sources.empty.title").into())
                            .subtitle(t!("Sources.empty.subtitle").into()),
                    )
                    .into_any_element()
            } else {
                // List 容器（border + padding + size_full）。
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
            // ---- 分页页脚（`Pagination` 自己判可见性，不足一页不渲染）----
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
