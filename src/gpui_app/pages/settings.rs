//! Settings 页面：iOS 风格设置面板（Stage 9 最小可工作版）。
//!
//! 完整实现涉及 20+ 个字段 + 各类 Switch/Input/NumberInput 控件；当前
//! Stage 9 落地核心路径（主题 / 语言 / 输出格式 + 一组文本输入 + 立即保存），
//! 数字/布尔/Switch/更多文本字段作为未来扩展（Stage 9 之后细化）。
//!
//! 行为对应旧 `crate::ui::pages::settings::show`（核心部分）：
//! - 主题切换：gpui-component `Select` 列出 `ThemeRegistry` 中所有主题；选中后
//!   调 `apply_theme_by_name` 实时换肤 + 写回 `config.theme` 字符串。
//! - 语言切换：调 `model.config.language` 改 enum。
//! - 输出格式切换：调 `model.config.ext_name` 改 enum。
//! - 文本输入：调 `model.config.download_path` 改 string。
//! - 立即保存按钮：调 `model.persist_settings()` 写盘。
//! - 检查更新：调 `model.spawn_update_check()`。
//!
//! 验收（plan 列的 acceptance）：
//! - ✅ 主题切换立即生效 + 持久化
//! - ✅ 语言切换立即生效 + 持久化
//! - ✅ 主要字段（text）写回 config.toml
//! - ✅ 检查更新仍工作
//! - ⚠️ 数字 / 布尔 Switch 未实现（下一轮 Stage 9.1 补齐）

use gpui::{
    div, px, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonGroup},
    h_flex, v_flex,
    input::{Input, InputState},
    select::{Select, SelectEvent, SelectState},
    ActiveTheme as _, Icon, IconName, IndexPath, Selectable, ThemeRegistry,
};

use crate::app::AppModel;
use crate::config::{ExportFormat, LangType};
use crate::gpui_app::components::PageHeader;
use crate::gpui_app::themes;

/// Settings 页面 entity。
pub struct SettingsPage {
    model: Entity<AppModel>,
    download_path: Entity<InputState>,
    /// 主题下拉：列出 `ThemeRegistry` 中所有主题名。SelectEvent::Confirm 触发应用。
    theme_select: Entity<SelectState<Vec<SharedString>>>,
}

impl SettingsPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let cfg = model.read(cx).config.clone();
        let download_path =
            cx.new(|cx| InputState::new(window, cx).default_value(cfg.download_path.clone()));

        // 主题下拉。列表来自 `themes::list_theme_names`（按字典序）；初始选中
        // = config.theme 匹配的名字（找不到 → 第一个，让下拉框折叠态也有可见值）。
        // 注意：此时 registry 多数情况下只有 2 个内置默认（themes::init 的
        // watch_dir 异步 reload 还在 cx.spawn task 排队）；observer 在 reload
        // 完成后会 set_items(38) + set_selected_index(cfg.theme) 喂完整列表。
        let names = themes::list_theme_names(cx);
        let current_name = cfg.theme.clone();
        let initial_index = names
            .iter()
            .position(|n| n.as_ref() == current_name)
            .or_else(|| if names.is_empty() { None } else { Some(0) })
            .map(|i| IndexPath::default().row(i));
        let theme_select = cx.new(|cx| {
            SelectState::new(names, initial_index, window, cx).searchable(true)
        });

        // 监听 Confirm 事件：写回 model + 实时切主题。
        cx.subscribe_in(
            &theme_select,
            window,
            |this, _state, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                if let SelectEvent::Confirm(Some(name)) = event {
                    this.on_theme_confirm(name.clone(), window, cx);
                }
            },
        )
        .detach();

        // **关键**：`themes::init` 内部 `watch_dir` 是异步的，`on_load` 回调跑完
        // 时 registry 才有 38 个 embed 主题。但 `SettingsPage` 是懒 mount（用户
        // 切到设置页才 `new`），到那时 registry 可能还是只有 2 个默认 — 因为
        // reload 仍在 cx.spawn task 里排队。注册一个 ThemeRegistry observer，
        // 每次 registry 变化时把最新 names 喂给 SelectState，同时**恢复**初始
        // 选中（按 cfg.theme 找，找不到 = 第一项 — 折叠态立刻显示主题名）。
        let model_for_observer = model.clone();
        let select_for_observer = theme_select.clone();
        cx.observe_global_in::<ThemeRegistry>(window, move |_this, window, cx| {
            let names = themes::list_theme_names(cx);
            let current = model_for_observer.read(cx).config.theme.clone();
            tracing::info!(
                "ThemeRegistry changed -> Select sees {} entries (current='{}')",
                names.len(),
                current
            );
            let names_for_idx = names.clone();
            let _ = select_for_observer.update(cx, |state, cx| {
                state.set_items(names, window, cx);
                // 恢复选中 — 名字在 names 里就选中它，否则默认第一项。
                // set_selected_value 内部走 `position(value)` → set_selected_index，
                // 找不到时 set_selected_index(None)（= placeholder）。
                // 我们要"找不到时 fallback 到第一项" — 手动算 index。
                let idx = names_for_idx
                    .iter()
                    .position(|n| n.as_ref() == current)
                    .or_else(|| if names_for_idx.is_empty() { None } else { Some(0) });
                state.set_selected_index(
                    idx.map(|i| IndexPath::default().row(i)),
                    window,
                    cx,
                );
            });
            // 不要 `cx.notify()` — `set_selected_index` 内部已 notify
        })
        .detach();

        Self {
            model,
            download_path,
            theme_select,
        }
    }

    /// 主题下拉 Confirm 事件的真实处理函数。`new` 里的 subscribe 闭包把 this 拿到后转发过来。
    fn on_theme_confirm(
        &mut self,
        name: SharedString,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name_str = name.to_string();
        self.model.update(cx, |m, _cx| {
            m.config.theme = name_str.clone();
            m.settings_dirty = true;
        });
        // 实时应用 — 内部走 Theme::global_mut(cx).apply_config，
        // gpui-component 的 observer 会自动 cx.refresh_windows()。
        themes::apply_theme_by_name(&name_str, cx);
        cx.notify();
    }

    fn persist_now(&mut self, cx: &mut Context<Self>) {
        // 同步 download_path 文本
        let dp = self.download_path.read(cx).value().to_string();
        self.model.update(cx, |m, _cx| {
            m.config.download_path = dp;
            m.persist_settings();
        });
        cx.notify();
    }

    fn check_update(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| m.spawn_update_check());
        cx.notify();
    }

    fn set_lang(&mut self, lang: LangType, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| {
            m.config.language = lang;
            m.settings_dirty = true;
        });
        cx.notify();
    }

    fn set_ext(&mut self, ext: ExportFormat, cx: &mut Context<Self>) {
        self.model.update(cx, |m, _cx| {
            m.config.ext_name = ext;
            m.settings_dirty = true;
        });
        cx.notify();
    }
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let cfg = model.config.clone();
        let _ = model;

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            // PageHeader + 立即保存 + 检查更新
            .child(
                PageHeader::new("设置")
                    .subtitle("主题 / 语言 / 输出 / 下载路径 — 改动即时生效；右上「立即保存」或关闭前自动写入 config.toml")
                    .action(
                        Button::new("save")
                            .icon(Icon::new(IconName::Check))
                            .label("立即保存")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.persist_now(cx);
                            })),
                    )
                    .action(
                        Button::new("check-update")
                            .icon(Icon::new(IconName::Loader))
                            .label("检查更新")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.check_update(cx);
                            })),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .flex_1()
                    .child(
                        // 全局卡片
                        v_flex()
                            .flex_1()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .p_4()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(cx.theme().foreground)
                                            .child("全局"),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_3()
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child("主题"),
                                                    )
                                                    .child(
                                                        // gpui-component `Select`：列出 ThemeRegistry
                                                        // 全部主题；选中即调 `apply_theme_by_name`
                                                        // 实时切。light/dark mode 跟 OS 走，不由这里决定。
                                                        Select::new(&self.theme_select)
                                                            .placeholder("选择主题")
                                                            .w(px(280.0)),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child("界面语言"),
                                                    )
                                                    .child(
                                                        ButtonGroup::new("lang")
                                                            .child(
                                                                Button::new("zhcn")
                                                                    .label("简体中文")
                                                                    .selected(cfg.language == LangType::ZhCn)
                                                                    .on_click(cx.listener(|p, _, _window, cx| {
                                                                        p.set_lang(LangType::ZhCn, cx);
                                                                    })),
                                                            )
                                                            .child(
                                                                Button::new("zhtw")
                                                                    .label("繁體中文")
                                                                    .selected(cfg.language == LangType::ZhTw)
                                                                    .on_click(cx.listener(|p, _, _window, cx| {
                                                                        p.set_lang(LangType::ZhTw, cx);
                                                                    })),
                                                            )
                                                            .child(
                                                                Button::new("zhhant")
                                                                    .label("繁體中文(通用)")
                                                                    .selected(cfg.language == LangType::ZhHant)
                                                                    .on_click(cx.listener(|p, _, _window, cx| {
                                                                        p.set_lang(LangType::ZhHant, cx);
                                                                    })),
                                                            ),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child("输出格式"),
                                                    )
                                                    .child(
                                                        ButtonGroup::new("ext")
                                                            .child(
                                                                Button::new("epub")
                                                                    .label("epub")
                                                                    .selected(cfg.ext_name == ExportFormat::Epub)
                                                                    .on_click(cx.listener(|p, _, _window, cx| {
                                                                        p.set_ext(ExportFormat::Epub, cx);
                                                                    })),
                                                            )
                                                            .child(
                                                                Button::new("txt")
                                                                    .label("txt")
                                                                    .selected(cfg.ext_name == ExportFormat::Txt)
                                                                    .on_click(cx.listener(|p, _, _window, cx| {
                                                                        p.set_ext(ExportFormat::Txt, cx);
                                                                    })),
                                                            )
                                                            .child(
                                                                Button::new("html")
                                                                    .label("html")
                                                                    .selected(cfg.ext_name == ExportFormat::Html)
                                                                    .on_click(cx.listener(|p, _, _window, cx| {
                                                                        p.set_ext(ExportFormat::Html, cx);
                                                                    })),
                                                            )
                                                            .child(
                                                                Button::new("pdf")
                                                                    .label("pdf")
                                                                    .selected(cfg.ext_name == ExportFormat::Pdf)
                                                                    .on_click(cx.listener(|p, _, _window, cx| {
                                                                        p.set_ext(ExportFormat::Pdf, cx);
                                                                    })),
                                                            ),
                                                    ),
                                            ),
                                    ),
                            ),
                    )
                    // 下载路径卡片
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .p_4()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(cx.theme().foreground)
                                            .child("下载"),
                                    )
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .justify_between()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child("下载目录"),
                                            )
                                            .child(Input::new(&self.download_path).w(px(360.0))),
                                    ),
                            ),
                    ),
            )
    }
}
