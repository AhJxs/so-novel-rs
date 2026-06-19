//! 设置 page：用 gpui-component `Settings` 组件搭的一级导航页面。
//!
//! 与其他 4 个 page（Library / Search / Tasks / Sources）一样：
//! - `RootView` 一次性创建 entity，跨切换保留内部状态；
//! - 通过 sidebar 5 个 `SidebarMenuItem` 中的一项 + `Cmd+5` 直接跳。
//!
//! ## 文件结构
//!
//! ```
//! settings/
//! ├── mod.rs         # SettingsPage struct + impl + 新 build_pages（顶层组装 16 行）
//! ├── ctx.rs         # PageCtx<'a>：owner-cached entity 透传 + PickFolderListener alias
//! ├── fields.rs      # string_field / bool_field / number_field_* / dropdown_field 6 个 helper
//! ├── page_general.rs # Page 1: 外观 + 网络 + 下载
//! ├── page_crawl.rs   # Page 2: 抓取
//! ├── page_proxy.rs   # Page 3: 代理
//! └── page_about.rs   # Page 4: 关于
//! ```
//!
//! ## 页面结构
//!
//! 4 个 page（gpui-component `Settings` 的左侧 sidebar 切换）：
//!
//! 1. **常规** — 主题 / 语言 / 字号 / GitHub 代理 / Cloudflare bypass / 下载目录 / 默认格式 /
//!    TXT 编码 / 保留章节缓存 / 启用下载进度条
//! 2. **抓取** — 搜索条数上限 / 过滤低相似度 / 并发上限 / 请求间隔 / 启用失败重试 /
//!    最大重试次数 / 重试间隔
//! 3. **代理** — 启用 HTTP 代理 / 代理 Host / 代理 Port / 起点 Cookie
//! 4. **关于** — 版本 / 检查更新 / 项目主页
//!
//! ## 保存机制（auto-save，无手动按钮）
//!
//! 每个 setter 改完字段后**立即**调 `model.persist_settings()` 写盘 ——
//! 没有单独的"立即保存"按钮：任何字段改动都 O(1) 立刻落盘（改主题 / 改 host 均如此）。
//!
//! 主题列表（`SettingField::dropdown`）每次 Render 重新 snapshot —— 用户装了
//! 新主题 → `ThemeRegistry::watch_dir` reload → observer 触发 `cx.notify()` → 本页
//! 重渲染 → dropdown 自动出现新选项。
//!
//! `NumberFieldOptions` 接 `f64`；对 `Option<i32>` 用 sentinel `-1.0` 表示"不限制"。

mod ctx;
mod fields;
mod page_about;
mod page_crawl;
mod page_general;
mod page_proxy;

use gpui::{App, AppContext, Context, Entity, IntoElement, Render, SharedString, Window};
use gpui_component::{
    group_box::GroupBoxVariant,
    input::{InputEvent, InputState},
    select::{SearchableVec, SelectState},
    setting::Settings,
    slider::{SliderEvent, SliderState, SliderValue},
};

use crate::app::AppModel;
use crate::gpui_app::{i18n::ts, themes};

use ctx::{PageCtx, PickFolderListener};

/// 设置 page entity。由 `RootView::new` 创建，挂在 sidebar 第 5 个 nav item。
pub struct SettingsPage {
    model: Entity<AppModel>,
    /// 下载目录输入框的 `InputState` —— 同样必须在 `new` 里建一次并缓存，
    /// 原因同下面的 theme SelectState（见下）。这里的额外目的是挂 `InputEvent::Change` 订阅，
    /// 用户键入时实时把新值回写到 `model.config.download_path`。
    download_path_input: Entity<InputState>,
    /// 上次同步到各 theme SelectState 的主题名列表（按 `list_theme_names` 的排序）。
    ///
    /// 主题列表**不是**静态的：启动时 `ThemeRegistry::watch_dir` async 加载 21 个
    /// embed json，根 root 是 `cx.spawn(...)`，所以 `SettingsPage::new` 跑的时候
    /// 列表里可能只有 gpui-component 默认注册的 Light + Dark 两个。等 async
    /// 加载完，`apply_theme_pref` 触发 `cx.refresh_windows()`，下一次 render
    /// 我们在 `sync_theme_items` 里重新拍快照 + 对比，发现变了就 `set_items` 推
    /// 到对应 SelectState + 按当前 config 选中值重定位。
    last_theme_names: Vec<SharedString>,
    /// 静态模式用的主题 `SelectState`（全量主题，可搜索）。`theme_state_static`。
    /// 复用原因同 `download_path_input`：`SettingField::render` 闭包每帧重建会丢
    /// popup/focus/选中，必须在 `new` 里建一次缓存。
    theme_state_static: Entity<SelectState<SearchableVec<SharedString>>>,
    /// 动态模式 — 浅色主题 `SelectState`（已按 mode=light 过滤）。
    theme_state_dyn_light: Entity<SelectState<SearchableVec<SharedString>>>,
    /// 动态模式 — 深色主题 `SelectState`（已按 mode=dark 过滤）。
    theme_state_dyn_dark: Entity<SelectState<SearchableVec<SharedString>>>,
    /// 上次同步到 `theme_state_dyn_light` 的浅色主题名列表。
    last_light_names: Vec<SharedString>,
    /// 上次同步到 `theme_state_dyn_dark` 的深色主题名列表。
    last_dark_names: Vec<SharedString>,
    /// 字号滑块的 `SliderState` —— 同 `theme_state` 一样必须在 `new` 里建一次并缓存：
    /// `SettingField::render` 闭包每次 render 都跑，现建会导致拖拽位置 / focus 丢失。
    /// 订阅 `SliderEvent::Change` → 写 `config.font_size` + persist + `apply_font_size`
    /// 实时缩放整个 app。
    font_size_state: Entity<SliderState>,
    /// 「下载目录」输入框右侧「浏览」按钮的 click listener ——
    /// `new` 里通过 `cx.listener(...)` 建一次并缓存为 `Rc<dyn Fn>`，render
    /// 闭包里只 `self.pick_folder_listener.as_ref()` 复用。
    ///
    /// 为什么不放进 `SettingField::render` 闭包里现建？
    /// 那个闭包签名是 `Fn(&RenderOptions, &mut Window, &mut App) -> impl IntoElement + 'static`，
    /// **拿不到** `&mut Context<Self>`，也就调不了 `cx.listener`。
    ///
    /// 为什么不沿用早先的「render 闭包里 `page_handle.update(cx, |_page, ctx| ctx.spawn(...))`」？
    /// 那条路径在 GPUI 0.2.2 + gpui-component 0.5.1 下 click 不触发（疑似 suffix 内的
    /// button 被 Input 的 `on_mouse_down` 抢走了 hit，或者经过双重 update 后 WeakEntity
    /// 已 stale），`sources.rs::pick_and_add` 的 working pattern（`cx.listener` 绑到
    /// entity、entity 方法内直接 `cx.spawn` 拿 `&mut Context<Self>`）才是稳的。
    pick_folder_listener: PickFolderListener,
}

impl SettingsPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // ---- 主题 SelectState ×3 ----
        // 静态槽：全量主题（不区分明暗）；动态浅/深槽：按 mode 过滤。
        // 三者都可搜索（主题变体 30+ 项）。items 初始可能只有默认 Light/Dark（async
        // 加载未完），render 里 sync_theme_items 会异步补齐 + 重定位选中。
        let initial_names = themes::list_theme_names(cx);
        let initial_light = themes::list_theme_names_by_mode(cx, false);
        let initial_dark = themes::list_theme_names_by_mode(cx, true);

        let pref0 = model.read(cx).config.theme_pref.clone();
        // 宏而非闭包：闭包捕获 `window: &mut Window` 后只能调一次（&mut 借用），
        // 三个 SelectState 各需独立借用 → 用宏在调用处展开，每次各自借 window。
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

        // 三个 Select 各自订阅 Confirm —— 写对应 config 字段 + persist + apply_theme_pref。
        // `apply_theme_pref` 需要 `Option<&mut Window>`；订阅 handler 拿不到 window，
        // 传 None（Dynamic/System 模式走 `cx.window_appearance()` 兜底，足够）。
        cx.subscribe(&theme_state_static, |this, _s, ev, cx| {
            if let gpui_component::select::SelectEvent::Confirm(Some(v)) = ev {
                let name = v.to_string();
                this.model.update(cx, |m, _| {
                    m.config.theme_pref.static_name = name;
                    m.persist_settings();
                });
                this.reapply_theme(None, cx);
            }
        })
        .detach();
        cx.subscribe(&theme_state_dyn_light, |this, _s, ev, cx| {
            if let gpui_component::select::SelectEvent::Confirm(Some(v)) = ev {
                let name = v.to_string();
                this.model.update(cx, |m, _| {
                    m.config.theme_pref.dyn_light = name;
                    m.persist_settings();
                });
                this.reapply_theme(None, cx);
            }
        })
        .detach();
        cx.subscribe(&theme_state_dyn_dark, |this, _s, ev, cx| {
            if let gpui_component::select::SelectEvent::Confirm(Some(v)) = ev {
                let name = v.to_string();
                this.model.update(cx, |m, _| {
                    m.config.theme_pref.dyn_dark = name;
                    m.persist_settings();
                });
                this.reapply_theme(None, cx);
            }
        })
        .detach();

        // 下载目录输入框：初始值 = 当前 config.download_path。
        // `default_value` 存到 InputState，渲染时第一次显示。
        let initial_download_path = model.read(cx).config.download_path.clone();
        let download_path_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(ts("Settings.desc.download_path"))
                .default_value(initial_download_path.clone())
        });

        // 订阅 InputEvent::Change —— 用户键入 / 粘贴时实时回写到 model 并落盘。
        // 保留前后对比避免无意义的 persist（InputState 在第一次建时也会发一次 Change）。
        cx.subscribe(&download_path_input, |this, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let new_val = input.read(cx).value().to_string();
                let cur = this.model.read(cx).config.download_path.clone();
                if new_val != cur {
                    this.model.update(cx, |m, _| {
                        m.config.download_path = new_val;
                        m.persist_settings();
                    });
                }
            }
        })
        .detach();

        // 「下载目录」按钮的 click listener —— 必须 owner-cache，render 闭包里
        // `cx.listener` 拿不到 `Context<Self>`。
        let pick_folder_listener: PickFolderListener =
            Rc::new(cx.listener(|this, _ev, _window, cx| {
                this.pick_folder(cx);
            }));

        // 字号滑块：min/max 复用 themes 的常量，step 1px，初值 = 当前 config（钳到范围内）。
        let initial_font_size = model
            .read(cx)
            .config
            .font_size
            .clamp(themes::FONT_SIZE_MIN, themes::FONT_SIZE_MAX);
        let font_size_state = cx.new(|_cx| {
            SliderState::new()
                .min(themes::FONT_SIZE_MIN)
                .max(themes::FONT_SIZE_MAX)
                .step(1.0)
                .default_value(initial_font_size)
        });

        // 订阅 SliderEvent::Change —— 拖拽过程中每 px 触发一次：写 config + persist
        // （persist 内部 500ms debounce 合并）+ apply_font_size 实时刷新。字号写入
        // `Theme.font_size` 后 `Root::render` 下一帧用新值设 rem_size，全 app 缩放。
        cx.subscribe(&font_size_state, |this, _state, event, cx| {
            // SliderEvent 目前只有 Change 一个变体。
            let SliderEvent::Change(value) = event;
            let size = match *value {
                SliderValue::Single(v) => v,
                SliderValue::Range(_, end) => end,
            };
            this.model.update(cx, |m, _| {
                m.config.font_size = size;
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
        }
    }

    /// 把当前 `config.theme_pref` 应用到全局 Theme + 重应用字号。
    ///
    /// 抽出来给三处 Select Confirm 订阅 + kind/dyn-mode dropdown setter 复用，
    /// 避免每个 setter 各写一遍「apply_theme_pref + apply_font_size」。
    /// apply_theme_pref 内部会 apply_config（重置字号），所以字号必须在后面重应用。
    fn reapply_theme(&self, window: Option<&mut Window>, cx: &mut App) {
        let pref = self.model.read(cx).config.theme_pref.clone();
        themes::apply_theme_pref(&pref, window, cx);
        themes::apply_font_size(self.model.read(cx).config.font_size, cx);
    }

    /// 「下载目录」旁边的「浏览」按钮点击 → 调 rfd 选目录 → 回写 model + persist + notify。
    ///
    /// 用 `rfd::AsyncFileDialog`（rfd 0.15 + `tokio` feature）——
    /// 内部走 `tokio::task::spawn_blocking`，dialog 在 tokio 专门的 blocking thread
    /// pool 上跑，正确初始化 COM apartment + message pump。
    ///
    /// 别用同步 `rfd::FileDialog::pick_folder()` 丢 `cx.background_executor().spawn`
    /// 上 —— Windows 下 `IFileOpenDialog::Show()` 需要 STA + message pump，
    /// tokio worker thread 没有，`Show()` 静默失败立即返回 None 且 dialog 不显示。
    /// 详见 memory `rfd-windows-async-file-dialog-only.md`。
    ///
    /// `cur` 在 click handler 里同步读出再 move 进 async —— 别在 async 里
    /// `model.read(async_cx)`，那里只有 `&mut AsyncApp`，拿不到 `&App`。
    fn pick_folder(&mut self, cx: &mut Context<Self>) {
        let cur = self.model.read(cx).config.download_path.clone();
        let title = ts("Settings.choose_download_dir_dialog_title");
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
                        m.config.download_path = path_str;
                        m.persist_settings();
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// 主题列表变 → 同步到 SelectState。
    ///
    /// 拿不到 `cx.observe_global::<ThemeRegistry>` 的 Window 参数（callback 只有
    /// `&mut Context<Self>`），改在 `Render::render` 里做差量更新：每次 render 重
    /// 拍快照 `themes::list_theme_names(cx)`，对比 `self.last_theme_names`：
    ///   - 没变 → 直接返回，0 开销
    ///   - 变了 → `theme_state.update` 内部调 `set_items` + `set_selected_value`
    ///
    /// 触发源：`themes::init` 的 async reload 完成 → `apply_theme_by_name` 触发
    /// `Theme` observer → `cx.refresh_windows()` → render → 这里检测到变化。
    fn sync_theme_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pref = self.model.read(cx).config.theme_pref.clone();

        // 静态槽：全量主题。
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

        // 动态浅色槽：按 mode=light 过滤。
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

        // 动态深色槽：按 mode=dark 过滤。
        let new_dark = themes::list_theme_names_by_mode(cx, true);
        if new_dark != self.last_dark_names {
            let items: SearchableVec<SharedString> = new_dark.clone().into();
            let cur = SharedString::from(pref.dyn_dark.clone());
            let sel = <SearchableVec<SharedString> as SelectDelegate>::position(&items, &cur);
            self.theme_state_dyn_dark.update(cx, |s, cx| {
                s.set_items(items, window, cx);
                s.set_selected_index(sel, window, cx);
            });
            self.last_dark_names = new_dark;
        }
    }

    /// 外部改了 `model.config.download_path`（目前唯一的外部源是 rfd 选目录）→ 同步到
    /// InputState。常规键入走 `InputEvent::Change` 订阅，那条路径已经维护一致性。
    ///
    /// `InputState::set_value` 需要 `&mut Window`，observer 拿不到，所以走 render 路径
    /// —— 和 `sync_theme_items` 同样的套路。
    fn sync_download_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model_val = self.model.read(cx).config.download_path.clone();
        let input_val = self.download_path_input.read(cx).value().to_string();
        if model_val == input_val {
            return; // 已同步
        }
        // model 是真值源，覆写 input。
        self.download_path_input.update(cx, |state, cx| {
            state.set_value(model_val, window, cx);
        });
    }

    /// 构造 4 个 SettingPage —— 拆到 `page_general` / `page_crawl` / `page_proxy` / `page_about` 4 个模块。
    /// 这里只组装，不做业务逻辑。
    fn build_pages(&self, cx: &App) -> Vec<gpui_component::setting::SettingPage> {
        let ctx = PageCtx {
            model: &self.model,
            theme_state_static: &self.theme_state_static,
            theme_state_dyn_light: &self.theme_state_dyn_light,
            theme_state_dyn_dark: &self.theme_state_dyn_dark,
            font_size_state: &self.font_size_state,
            download_path_input: &self.download_path_input,
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
        // 主题列表可能晚于 SettingsPage::new 加载完（async file watcher），
        // 每次 render 重新拍快照 + 差量同步到 SelectState。详见 sync_theme_items。
        self.sync_theme_items(window, cx);
        // rfd 选完目录回写 model 后，同步到 InputState。详见 sync_download_path。
        self.sync_download_path(window, cx);

        let pages = self.build_pages(cx);

        // Settings id 固定 —— 不再随 language 变。
        //
        // 早期为「实时切语言」把 lang 塞进 id，靠 `use_keyed_state` 见新 key 重建
        // SettingsState 来刷新 placeholder。但语言切换已改成重启生效（见 language setter），
        // 不再有「切语言当帧刷新已缓存值」的需求，固定 id 即可，也避免切语言误触重置
        // 用户的 page / 搜索框 / 滚动位置。
        //
        // Settings 组件自带 sidebar（页切换）+ 主区布局。`with_group_variant(Outline)`
        // 给所有 group 加 1px 边框（`cx.theme().border`），视觉上把不同 group 分开。
        Settings::new("settings-page")
            .with_group_variant(GroupBoxVariant::Outline)
            .pages(pages)
    }
}

// 用到的 trait —— `Rc::new` 需要 `Rc` 类型 + `SelectState::position` 关联函数。
use gpui_component::select::SelectDelegate;
use std::rc::Rc;
