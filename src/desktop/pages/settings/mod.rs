//! 设置 page：用 gpui-kit 的 `Settings` 组件搭的一级导航页面。
//!
//! 保存机制（auto-save，无手动按钮）：每个 setter 改完字段后**立即**调
//! `model.persist_settings()` 写盘，没有单独的"立即保存"按钮。
//!
//! `NumberFieldOptions` 接 `f64`；对 `Option<i32>` 用 sentinel `-1.0` 表示"不限制"。

mod ctx;
mod fields;
mod page_about;
mod page_crawl;
mod page_general;
mod page_proxy;

use gpui_kit::component::{
    group_box::GroupBoxVariant,
    input::{InputEvent, InputState, TextareaState},
    select::{SearchableVec, SelectDelegate, SelectEvent, SelectState},
    setting::{SettingPage, Settings},
    slider::{SliderEvent, SliderState, SliderValue},
};
use gpui_kit::{App, AppContext, Context, Entity, IntoElement, Render, SharedString, Window};
use rust_i18n::t;

use crate::desktop::model::AppModel;
use crate::desktop::themes;

use ctx::{PageCtx, PickFolderListener};

pub struct SettingsPage {
    model: Entity<AppModel>,

    /// 所有 `Entity<InputState>` / `SelectState` / `SliderState` 都必须在 `new` 里建一次并缓存：
    /// `SettingField::render` 闭包是 `Fn + 'static`，拿不到 `&mut Context<Self>`。
    download_path_input: Entity<InputState>,
    font_size_state: Entity<SliderState>,
    qidian_cookie_input: Entity<TextareaState>,
    theme_state_static: Entity<SelectState<SearchableVec<SharedString>>>,
    theme_state_dyn_light: Entity<SelectState<SearchableVec<SharedString>>>,
    theme_state_dyn_dark: Entity<SelectState<SearchableVec<SharedString>>>,

    /// 主题名快照，用于差量同步到 `SelectState`：主题是 async 加载的，`sync_theme_items`
    /// 对比快照后补 `set_items` + 重定位选中值。
    last_theme_names: Vec<SharedString>,
    last_light_names: Vec<SharedString>,
    last_dark_names: Vec<SharedString>,

    /// 「下载目录」输入框右侧「浏览」按钮的 click listener —— render 闭包拿不到
    /// `Context<Self>`，所以 `new` 里 `cx.listener(...)` 建一次缓存成 `Rc<dyn Fn>`。
    pick_folder_listener: PickFolderListener,
}

impl SettingsPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 主题 SelectState ×3：静态槽全量、动态浅/深槽按 mode 过滤（可搜索）。
        // items 初始可能缺项（async 加载未完），render 里 `sync_theme_items` 会补齐 + 重定位选中。
        let initial_names = themes::list_theme_names(cx);
        let initial_light = themes::list_theme_names_by_mode(cx, false);
        let initial_dark = themes::list_theme_names_by_mode(cx, true);

        let pref0 = model.read(cx).config.global.theme_pref.clone();
        // 用宏而不是闭包：闭包捕获 `window: &mut Window` 后只能调一次，三个 `SelectState`
        // 各需独立借用 → 宏在调用处展开。
        macro_rules! make_state {
            ($names:expr, $cur:expr) => {{
                let items: SearchableVec<SharedString> = ($names).to_vec().into();
                let sel = SharedString::from($cur.to_string());
                let pos = <SearchableVec<SharedString> as SelectDelegate>::position(&items, &sel);
                cx.new(|cx| SelectState::new(items, pos, window, cx).searchable(true))
            }};
        }
        let theme_state_static = make_state!(&initial_names, &pref0.static_name);
        let theme_state_dyn_light = make_state!(&initial_light, &pref0.dyn_light);
        let theme_state_dyn_dark = make_state!(&initial_dark, &pref0.dyn_dark);

        // 三个 Select 各订阅 Confirm → 写 config + persist + apply_theme_pref。
        // `apply_theme_pref` 要 `Option<&mut Window>`，订阅 handler 拿不到 → 传 None
        // （Dynamic/System 走 `cx.window_appearance()` 兜底）。
        cx.subscribe(&theme_state_static, |this, _s, ev, cx| {
            if let SelectEvent::Confirm(Some(v)) = ev {
                let name = v.to_string();
                this.model.update(cx, |m, _| {
                    m.config.global.theme_pref.static_name = name;
                    m.persist_settings();
                });
                this.reapply_theme(None, cx);
            }
        })
        .detach();
        cx.subscribe(&theme_state_dyn_light, |this, _s, ev, cx| {
            if let SelectEvent::Confirm(Some(v)) = ev {
                let name = v.to_string();
                this.model.update(cx, |m, _| {
                    m.config.global.theme_pref.dyn_light = name;
                    m.persist_settings();
                });
                this.reapply_theme(None, cx);
            }
        })
        .detach();
        cx.subscribe(&theme_state_dyn_dark, |this, _s, ev, cx| {
            if let SelectEvent::Confirm(Some(v)) = ev {
                let name = v.to_string();
                this.model.update(cx, |m, _| {
                    m.config.global.theme_pref.dyn_dark = name;
                    m.persist_settings();
                });
                this.reapply_theme(None, cx);
            }
        })
        .detach();

        let initial_download_path = model.read(cx).config.download.download_path.clone();
        let download_path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Settings.desc.download_path"))
                .default_value(initial_download_path.clone())
        });

        // 前后对比避免无意义 persist（InputState 第一次建也会发 Change）。
        cx.subscribe(&download_path_input, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let new_val = input.read(cx).value().to_string();
                let cur = this.model.read(cx).config.download.download_path.clone();
                if new_val != cur {
                    this.model.update(cx, |m, _| {
                        m.config.download.download_path = new_val;
                        m.persist_settings();
                    });
                }
            }
        })
        .detach();

        // click listener 必须 owner-cache。
        let pick_folder_listener: PickFolderListener =
            Rc::new(cx.listener(|this, _ev, _window, cx| {
                this.pick_folder(cx);
            }));

        // 多行：`TextareaState` 模式本身携带多行，`.rows(3)` 给 3 行高度，
        // 用户可粘贴整段 `Cookie:` 头。
        let initial_qidian_cookie = model.read(cx).config.cookie.qidian_cookie.clone();
        let qidian_cookie_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .placeholder(t!("Settings.placeholder.qidian_cookie"))
                .default_value(initial_qidian_cookie.clone())
        });

        cx.subscribe(&qidian_cookie_input, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let new_val = input.read(cx).value().to_string();
                let cur = this.model.read(cx).config.cookie.qidian_cookie.clone();
                if new_val != cur {
                    this.model.update(cx, |m, _| {
                        m.config.cookie.qidian_cookie = new_val;
                        m.persist_settings();
                    });
                }
            }
        })
        .detach();

        // 字号滑块：min/max 复用 themes 常量，step 1px，初值 = 当前 config（钳到范围内）。
        let initial_font_size = model
            .read(cx)
            .config
            .global
            .font_size
            .clamp(themes::FONT_SIZE_MIN, themes::FONT_SIZE_MAX);
        let font_size_state = cx.new(|_cx| {
            SliderState::new()
                .min(themes::FONT_SIZE_MIN)
                .max(themes::FONT_SIZE_MAX)
                .step(1.0)
                .default_value(initial_font_size)
        });

        // 拖拽每 px 触发：写 config + persist（500ms debounce 合并）+ apply_font_size。
        // 字号写入 `Theme.font_size` 后 `Root::render` 下一帧用新值设 rem_size → 全 app 缩放。
        cx.subscribe(&font_size_state, |this, _state, event, cx| {
            // 连续拖拽期间只关心 `Change`，`Release` 不携带新值（最后一次 Change 已落盘）。
            let SliderEvent::Change(value) = event else {
                return;
            };
            let size = match *value {
                SliderValue::Single(v) => v,
                SliderValue::Range(_, end) => end,
            };
            this.model.update(cx, |m, _| {
                m.config.global.font_size = size;
                m.persist_settings();
            });
            themes::apply_font_size(size, cx);
        })
        .detach();

        Self {
            model,
            download_path_input,
            last_theme_names: initial_names,
            last_light_names: initial_light,
            last_dark_names: initial_dark,
            theme_state_static,
            theme_state_dyn_light,
            theme_state_dyn_dark,
            pick_folder_listener,
            font_size_state,
            qidian_cookie_input,
        }
    }

    /// 把当前 `config.global.theme_pref` 应用到全局 Theme + 重应用字号（`apply_theme_pref` 内部
    /// 会 `apply_config` 重置字号，所以必须在后面重应用）。
    fn reapply_theme(&self, window: Option<&mut Window>, cx: &mut App) {
        let pref = self.model.read(cx).config.global.theme_pref.clone();
        themes::apply_theme_pref(&pref, window, cx);
        themes::apply_font_size(self.model.read(cx).config.global.font_size, cx);
    }

    /// 「下载目录」旁边的「浏览」按钮点击 → 调 rfd 选目录 → 回写 model + persist + notify。
    /// 必须用 `rfd::AsyncFileDialog`（内部走 `tokio::task::spawn_blocking`，能初始化 COM
    /// apartment + message pump）；同步版丢 worker thread 上会因缺 STA 静默返回 None。
    fn pick_folder(&self, cx: &Context<Self>) {
        let cur = self.model.read(cx).config.download.download_path.clone();
        let title = t!("Settings.choose_download_dir_dialog_title");
        let model = self.model.clone();
        let page_handle = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            let mut dlg = rfd::AsyncFileDialog::new().set_title(title);
            if !cur.is_empty() {
                dlg = dlg.set_directory(cur);
            }
            let folder = dlg.pick_folder().await;
            if let Some(folder) = folder {
                let path_str = folder.path().to_string_lossy().to_string();
                let _ = page_handle.update(async_cx, |_page, cx| {
                    model.update(cx, |m, _| {
                        m.config.download.download_path = path_str;
                        m.persist_settings();
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 主题列表变 → 同步到 `SelectState`。拿不到 `cx.observe_global::<ThemeRegistry>` 需要的
    /// Window，改在 render 里差量更新：重拍快照对比 `last_xxx_names`，变了才更新 items。
    fn sync_theme_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pref = self.model.read(cx).config.global.theme_pref.clone();

        let new_names = themes::list_theme_names(cx);
        if new_names != self.last_theme_names {
            let items: SearchableVec<SharedString> = new_names.clone().into();
            let cur = SharedString::from(pref.static_name.clone());
            let sel = <SearchableVec<SharedString> as SelectDelegate>::position(&items, &cur);
            self.theme_state_static.update(cx, |s, cx| {
                s.set_items(items, window, cx);
                s.set_selected_index(sel, window, cx);
            });
            self.last_theme_names = new_names;
        }

        let new_light = themes::list_theme_names_by_mode(cx, false);
        if new_light != self.last_light_names {
            let items: SearchableVec<SharedString> = new_light.clone().into();
            let cur = SharedString::from(pref.dyn_light.clone());
            let sel = <SearchableVec<SharedString> as SelectDelegate>::position(&items, &cur);
            self.theme_state_dyn_light.update(cx, |s, cx| {
                s.set_items(items, window, cx);
                s.set_selected_index(sel, window, cx);
            });
            self.last_light_names = new_light;
        }

        let new_dark = themes::list_theme_names_by_mode(cx, true);
        if new_dark != self.last_dark_names {
            let items: SearchableVec<SharedString> = new_dark.clone().into();
            let cur = SharedString::from(pref.dyn_dark);
            let sel = <SearchableVec<SharedString> as SelectDelegate>::position(&items, &cur);
            self.theme_state_dyn_dark.update(cx, |s, cx| {
                s.set_items(items, window, cx);
                s.set_selected_index(sel, window, cx);
            });
            self.last_dark_names = new_dark;
        }
    }

    /// 外部改了 `model.config.download.download_path`（目前唯一来源是 rfd 选目录）→ 同步到
    /// `InputState`。`InputState::set_value` 需要 `&mut Window`，observer 拿不到，走 render 路径。
    fn sync_download_path(&self, window: &mut Window, cx: &mut Context<Self>) {
        let model_val = self.model.read(cx).config.download.download_path.clone();
        let input_val = self.download_path_input.read(cx).value().to_string();
        if model_val == input_val {
            return;
        }
        self.download_path_input.update(cx, |state, cx| {
            state.set_value(model_val, window, cx);
        });
    }

    /// 组装 4 个 `SettingPage`。
    fn build_pages(&self, cx: &App) -> Vec<SettingPage> {
        let ctx = PageCtx {
            model: &self.model,
            theme_state_static: &self.theme_state_static,
            theme_state_dyn_light: &self.theme_state_dyn_light,
            theme_state_dyn_dark: &self.theme_state_dyn_dark,
            font_size_state: &self.font_size_state,
            download_path_input: &self.download_path_input,
            qidian_cookie_input: &self.qidian_cookie_input,
            pick_folder_listener: &self.pick_folder_listener,
        };
        vec![
            page_general::build(&ctx, cx),
            page_crawl::build(&ctx, cx),
            page_proxy::build(&ctx, cx),
            page_about::build(&ctx, cx),
        ]
    }
}

impl Render for SettingsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_theme_items(window, cx);
        self.sync_download_path(window, cx);

        let pages = self.build_pages(cx);

        // Settings id 固定 —— 不随 language 变，避免切语言（重启生效）误触重置用户的
        // page / 搜索框 / 滚动位置。
        Settings::new("settings-page")
            .with_group_variant(GroupBoxVariant::Outline)
            .pages(pages)
    }
}

// `PickFolderListener` 用到的 `Rc`。
use std::rc::Rc;
