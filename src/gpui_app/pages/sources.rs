//! Sources 页面：书源管理（导入 / 启用禁用 / 健康检查 / 删除）。
//!
//! 行为完全对应旧 `crate::ui::pages::sources::show`：
//! - 顶部统计：总数 / 启用 / 禁用 / 上次检测可用（用 `Badge`）。
//! - 右侧按钮：测速（`spawn_health_check`）、添加（`rfd` 文件选择器）。
//! - 列表：每条书源一行，包含：name + url + 语言 + 启用开关 + 健康状态 + 删除。
//! - 删除走 `WindowExt::open_dialog` 二次确认。
//! - 添加后立即刷新内存 rules（`model.add_sources_from_file` 内部已做）。

use gpui::{
    div, prelude::FluentBuilder as _, px, App, ClickEvent, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, Styled, Window,
};
use gpui_component::{
    badge::Badge,
    button::{Button, ButtonVariant, ButtonVariants},
    dialog::{Dialog, DialogButtonProps},
    h_flex, spinner::Spinner, switch::Switch, v_flex,
    ActiveTheme as _, Disableable, Icon, IconName, Sizable, WindowExt,
};

use crate::app::AppModel;
use crate::gpui_app::components::{truncate, EmptyState, PageHeader, StatusBadge};
use crate::models::Rule;

/// Sources 页面 entity。
pub struct SourcesPage {
    model: Entity<AppModel>,
}

impl SourcesPage {
    pub fn new(model: Entity<AppModel>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self { model }
    }

    #[allow(dead_code)]
    fn toggle(&mut self, source_id: i32, cx: &mut Context<Self>) {
        // 当前 Switch on_click 直接调 model.toggle_source_disabled，不走本方法。
        // 留作未来需要"确认后再 toggle"或 UI 状态机时用。
        self.model.update(cx, |m, _cx| {
            m.toggle_source_disabled(source_id);
        });
        cx.notify();
    }

    fn prompt_delete(&mut self, source_id: i32, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let model_id = model.entity_id();

        window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
            let model_for_ok = model.clone();
            let model_id_for_ok = model_id;
            let source_id_for_ok = source_id;
            dialog
                .title("确认删除书源")
                .child(div().child(format!(
                    "确定要删除书源 #{source_id} 吗？此操作无法撤销。"
                )))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("删除")
                        .cancel_text("取消")
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
    /// 不显示。详见 memory `rfd-windows-async-file-dialog-only.md`。
    fn pick_and_add(&mut self, cx: &mut Context<Self>) {
        let model = self.model.clone();
        let page_handle = cx.entity().downgrade();
        // Context::spawn 签名：fn(WeakEntity<Self>, &mut AsyncApp) -> R + 'static
        cx.spawn(async move |_weak, async_cx| {
            let file = rfd::AsyncFileDialog::new()
                .add_filter("JSON 规则文件", &["json", "json5"])
                .add_filter("所有文件", &["*"])
                .set_title("选择书源 JSON 文件")
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
}

impl Render for SourcesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let total = model.rules.len();
        let disabled = model.rules.iter().filter(|r| r.disabled).count();
        let enabled = total - disabled;

        // "可用"取上次 health-check 的结果
        let available_after_check = if !model.sources_state.health.is_empty()
            && !model.sources_state.running
        {
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
        let pending_delete = model.sources_state.pending_delete;
        let rules: Vec<Rule> = model.rules.clone();
        let _ = model;

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            // PageHeader
            .child(
                PageHeader::new("书源管理")
                    .subtitle("启用/禁用、连通性检测、JSON 导入")
                    .action(
                        Button::new("add-source")
                            .icon(Icon::new(IconName::Plus))
                            .label("添加")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.pick_and_add(cx);
                            })),
                    )
                    .action(
                        Button::new("health-check")
                            .icon(Icon::new(IconName::Loader))
                            .label("测速")
                            .disabled(running || total == 0)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.run_health_check(cx);
                            })),
                    ),
            )
            // 统计行 + 进度
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(stat_badge("总数", total, cx))
                    .when(enabled > 0, |this| {
                        this.child(stat_badge("启用", enabled, cx))
                    })
                    .when(disabled > 0, |this| {
                        this.child(stat_badge("禁用", disabled, cx))
                    })
                    .when_some(available_after_check, |this, n| {
                        this.child(stat_badge("可用", n, cx))
                    })
                    .when(running, |this| {
                        this.child(Spinner::new().small())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{}/{} 已返回", received, expected)),
                            )
                    }),
            )
            // 错误提示
            .when_some(rule_load_error, |this, err| {
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .child(format!("规则加载失败: {err}")),
                )
            })
            // 列表 / 空态
            .child(if rules.is_empty() {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::Globe, "暂未导入任何书源")
                            .subtitle("点击右上角「添加」选择 JSON 文件。"),
                    )
                    .into_any_element()
            } else {
                v_flex()
                    .flex_1()
                    .size_full()
                    .overflow_hidden()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .children(rules.iter().enumerate().map(|(idx, rule)| {
                        render_source_row(idx, rule, &health, pending_delete, cx.entity(), cx)
                    }))
                    .into_any_element()
            })
    }
}

/// 渲染一条书源行：name / url / lang / Switch / 健康状态 / 删除按钮。
fn render_source_row(
    index: usize,
    rule: &Rule,
    health: &std::collections::HashMap<i32, crate::crawler::health::SourceHealth>,
    pending_delete: Option<i32>,
    page: Entity<SourcesPage>,
    cx: &mut App,
) -> impl IntoElement {
    let health_status = health.get(&rule.id).cloned();
    let row_id = ("src-row", index as u64);
    let name = truncate(&rule.name, 30);
    let url = truncate(rule.url.as_str(), 60);

    h_flex()
        .id(row_id)
        .px_3()
        .py_2()
        .gap_3()
        .items_center()
        .border_b_1()
        .border_color(cx.theme().border)
        .hover(|this| this.bg(cx.theme().list_hover))
        // name + id
        .child(
            v_flex()
                .w(px(220.0))
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
                        .child(format!("#{}{}", rule.id, optional_str(&rule.language, " []"))),
                ),
        )
        // url
        .child(
            div()
                .flex_1()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(url),
        )
        // 启用开关
        .child({
            let page_for_switch = page.clone();
            let rule_id = rule.id;
            Switch::new(("src-switch", index as u64))
                .checked(!rule.disabled)
                .on_click(move |checked, _window, cx| {
                    let want_disabled = !*checked;
                    // 简化：只信任 !rule.disabled 切换；如不一致，sync model
                    page_for_switch.update(cx, |p, cx| {
                        // 通过 model 反向更新
                        p.model.update(cx, |m, _cx| {
                            if m.rules.iter().find(|r| r.id == rule_id).map(|r| r.disabled)
                                != Some(want_disabled)
                            {
                                m.toggle_source_disabled(rule_id);
                            }
                        });
                    });
                })
        })
        // 健康状态徽章
        .child({
            let badge_kind = health_status_kind(health_status.as_ref());
            let label = health_status_label(health_status.as_ref());
            StatusBadge::new(badge_kind, label)
        })
        // 删除
        .child({
            let page_for_del = page.clone();
            let rule_id = rule.id;
            Button::new(("src-del", index as u64))
                .xsmall()
                .ghost()
                .danger()
                .icon(Icon::new(IconName::Delete))
                .label(if pending_delete == Some(rule_id) {
                    "确认?"
                } else {
                    "删除"
                })
                .on_click(move |_, window, cx| {
                    page_for_del.update(cx, |p, cx| {
                        p.prompt_delete(rule_id, window, cx);
                    });
                })
        })
}

/// 健康状态 → 语义色枚举。
fn health_status_kind(
    h: Option<&crate::crawler::health::SourceHealth>,
) -> crate::gpui_app::components::StatusKind {
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
fn health_status_label(h: Option<&crate::crawler::health::SourceHealth>) -> String {
    match h {
        None => "未测".to_string(),
        Some(h) if h.error.is_some() => "错误".to_string(),
        Some(h) => match h.http_status {
            Some(s) => format!("HTTP {}", s),
            None => format!("{:?}", h.error),
        },
    }
}

/// 统计 Badge — 复用 `Badge` + 主题色。
fn stat_badge(label: &'static str, value: usize, cx: &mut App) -> impl IntoElement {
    h_flex()
        .gap_1()
        .items_center()
        .px_2()
        .py_1()
        .rounded_md()
        .bg(cx.theme().sidebar)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(label)
        .child(Badge::new().child(format!("{value}")).color(cx.theme().primary))
}

fn optional_str(s: &str, prefix: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!(" {}{}", prefix, s)
    }
}
