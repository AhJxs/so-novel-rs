//! 设置 page：用 gpui-component `Settings` 组件搭的一级导航页面。
//!
//! 与其他 4 个 page（Library / Search / Tasks / Sources）一样：
//! - `RootView` 一次性创建 entity，跨切换保留内部状态；
//! - 通过 sidebar 5 个 `SidebarMenuItem` 中的一项 + `Cmd+5` 直接跳。
//!
//! ## 页面结构
//!
//! 4 个 page（gpui-component `Settings` 的左侧 sidebar 切换）：
//!
//! 1. **常规** — 主题 / 语言 / GitHub 代理 / Cloudflare bypass / 下载目录 / 默认格式 /
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

use std::rc::Rc;

use gpui::{
    div, App, AppContext, ClickEvent, Context, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, Window,
};

use gpui_component::{
    ActiveTheme as _, AxisExt as _, Disableable, Icon, IconName, Sizable as _, button::{Button, ButtonVariants as _}, group_box::GroupBoxVariant, input::{Input, InputEvent, InputState}, select::{SearchableVec, Select, SelectDelegate, SelectEvent, SelectState}, setting::{
        NumberFieldOptions, SettingField, SettingGroup, SettingItem, SettingPage, Settings,
    }
};

use crate::app::AppModel;
use crate::config::{AppLang, ExportFormat, LangType};
use crate::gpui_app::{
    i18n::{ts, ts_fmt},
    locale_for, themes,
};

/// 「下载目录」按钮 click handler 的类型别名（owner-cache 闭包用）。
///
/// `Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>` 在 4 处直接写太长，
/// 用 alias 简化（见 settings.rs:94, 157）。
type PickFolderListener = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// 设置 page entity。由 `RootView::new` 创建，挂在 sidebar 第 5 个 nav item。
pub struct SettingsPage {
    model: Entity<AppModel>,
    /// 主题下拉的 `SelectState` —— 必须在 `new` 里建一次并缓存。
    ///
    /// 为什么不能像其他 dropdown 那样在 `SettingField::render` 闭包里现建？
    /// `SettingField::render` 闭包每次 render 都会跑一遍（`element.rs:90`），里面
    /// `cx.new(...)` 会拿到新 `entity_id`。`Select::new` 把 `state.entity_id()` 塞进
    /// element id (`select.rs:825`)，并通过 `cx.listener(Self::toggle_menu)` 把 click
    /// 路由到**当前那一次** SelectState entity。render 重建 SelectState 后：
    ///   - element id 变了 → 框架点不到原 hit target
    ///   - listener 绑在新 entity 上，但 popup 状态 / focus handle / 滚动位置全部丢失
    ///
    /// 结果就是"看起来能点，但 popup 不弹出"。
    ///
    /// 解法：建在 `SettingsPage::new` 里（拿到 `Context<Self>` 可挂订阅），闭包只
    /// `Select::new(&self.theme_state)` 复用缓存。
    theme_state: Entity<SelectState<SearchableVec<SharedString>>>,
    /// 下载目录输入框的 `InputState` —— 同样必须在 `new` 里建一次并缓存，
    /// 原因同 `theme_state`（见上）。这里的额外目的是挂 `InputEvent::Change` 订阅，
    /// 用户键入时实时把新值回写到 `model.config.download_path`。
    download_path_input: Entity<InputState>,
    /// 上次同步到 `theme_state` 的主题名列表（按 `list_theme_names` 的排序）。
    ///
    /// 主题列表**不是**静态的：启动时 `ThemeRegistry::watch_dir` async 加载 21 个
    /// embed json，根 root 是 `cx.spawn(...)`，所以 `SettingsPage::new` 跑的时候
    /// 列表里可能只有 gpui-component 默认注册的 Light + Dark 两个。等 async
    /// 加载完，`apply_theme_by_name` 触发 `cx.refresh_windows()`，下一次 render
    /// 我们在 `sync_theme_items` 里重新拍快照 + 对比，发现变了就 `set_items` 推
    /// 到 SelectState + 按当前 `config.theme` 重定位选中。
    last_theme_names: Vec<SharedString>,
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
        // SearchableVec<SharedString>：SharedString 已实现 `SelectItem`
        // (title=value, value=value)，搜索按 title.to_lowercase().contains。
        let initial_names = themes::list_theme_names(cx);
        let items: SearchableVec<SharedString> = initial_names.clone().into();

        // 初始选中：当前 `config.theme` 可能在 items 里（embed 主题），也可能不在
        // （用户后来改了 themes 目录）。不在 → None（空 placeholder），用户重新选即可。
        let cur = SharedString::from(model.read(cx).config.theme.clone());
        let selected =
            <SearchableVec<SharedString> as SelectDelegate>::position(&items, &cur);

        let theme_state = cx.new(|cx| {
            SelectState::new(items, selected, window, cx).searchable(true)
        });

        // 订阅 SelectEvent::Confirm —— 用户在弹层里敲回车 / 点 item 时触发。
        // 4 参数版 `cx.subscribe`：handler 是 `Fn(&mut Self, &E, &Ev, &mut Context<Self>)`。
        cx.subscribe(&theme_state, |this, _state, ev, cx| {
            if let SelectEvent::Confirm(Some(value)) = ev {
                let name = value.to_string();
                this.model.update(cx, |model, _| {
                    model.config.theme = name.clone();
                    model.persist_settings();
                });
                // 实时换肤：Theme::global_mut.apply_config 内部触发
                // gpui-component observer → cx.refresh_windows()。
                themes::apply_theme_by_name(&name, cx);
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

        Self {
            model,
            theme_state,
            download_path_input,
            last_theme_names: initial_names,
            pick_folder_listener,
        }
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
        let new_names = themes::list_theme_names(cx);
        if new_names == self.last_theme_names {
            return;
        }

        let items: SearchableVec<SharedString> = new_names.clone().into();
        // 重新定位当前选中的主题（按 config.theme），保持 UI 与持久化一致。
        let cur = SharedString::from(self.model.read(cx).config.theme.clone());
        let selected = <SearchableVec<SharedString> as SelectDelegate>::position(&items, &cur);

        self.theme_state.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            // set_items 后 selected_index 会失效（list 的 delegate 换了），必须重选。
            state.set_selected_index(selected, window, cx);
        });

        self.last_theme_names = new_names;
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

    /// 构造 4 个 SettingPage + 内部 group + item。
    ///
    /// 每个 setter 走 model.update → 写字段 → 调 `model.persist_settings()`
    /// 立即落盘。无需「立即保存」按钮。
    ///
    /// 所有 page / group / item 标题、description、dropdown option 标签都走
    /// `i18n::tr(key` —— `app_lang` 从 model 读，每次 render 重读，
    /// `set_locale` + `refresh_windows` 后切语言即时生效。
    fn build_pages(&self) -> Vec<SettingPage> {
        // app_lang 的读取放在下面的 `render` 里（用于构造 Settings id 触发重建），
        // 不再在 build_pages 单独读 —— `t!` 走全局 locale，无需依赖本变量。
        // 主题下拉的选项在 render 闭包里现取（themes::list_theme_names），
        // 走可搜索 Select（36 个变体下默认下拉框不够长，需 search 才能快速定位）。

        // 3 种**书源**语言 → (value_str, label)，用 `LangType::as_str()` 的格式
        // （"zh_CN" / "zh_TW" / "zh_Hant"）跟 TOML 持久化层保持一致 —— 之前用
        // "zh-cn" / "zh-tw" / "zh-hant" 这种小写连字符格式，导致 dropdown 的
        // getter 返回值跟 options 不匹配，重启后下拉框看不到当前选项。
        // label 走 i18n：界面语言切到 English 时显示 "Simplified Chinese" 等。
        let lang_options: Vec<(SharedString, SharedString)> = vec![
            (LangType::ZhCn.as_str().into(), ts("Settings.option.booklang.zh_cn"),),
            (LangType::ZhTw.as_str().into(), ts("Settings.option.booklang.zh_tw"),),
            (LangType::ZhHant.as_str().into(), ts("Settings.option.booklang.zh_hant"),),
        ];

        // 3 种**应用 UI** 语言 → (value_str, label)，存到 TOML `[global].app-lang`，
        // 由 `AppLang::as_str()` 给出（"zh-CN" / "zh-TW" / "en"）。
        // label 走 i18n：切到 English 时显示 "Simplified Chinese" / "Traditional Chinese" / "English"。
        let app_lang_options: Vec<(SharedString, SharedString)> = vec![
            (AppLang::ZhCn.as_str().into(), ts("Settings.option.applang.zh_cn"),),
            (AppLang::ZhTw.as_str().into(), ts("Settings.option.applang.zh_tw"),),
            (AppLang::En.as_str().into(), ts("Settings.option.applang.en"),),
        ];

        // 4 种输出格式 → (value_str, label)
        let ext_options: Vec<(SharedString, SharedString)> = vec![
            (ext_value(ExportFormat::Epub).into(), "epub".into()),
            (ext_value(ExportFormat::Txt).into(), "txt".into()),
            (ext_value(ExportFormat::Html).into(), "html".into()),
            (ext_value(ExportFormat::Pdf).into(), "pdf".into()),
        ];

        // 7 种常见 TXT 编码 → (value_str, label)
        // 与 `ENCODINGS` 常量一致。
        let encoding_options: Vec<(SharedString, SharedString)> = TXT_ENCODINGS
            .iter()
            .map(|e| ((*e).into(), (*e).into()))
            .collect();

        let m = self.model.clone();

        vec![
            // ============ Page 1: 常规 ============
            SettingPage::new(ts("Settings.page.general"))
                .resettable(false)
                .default_open(true)
                .groups(vec![
                    // 外观
                    SettingGroup::new().title(ts("Settings.group.appearance")).items(vec![
                        // -- 主题（可搜索下拉，36 个变体）--
                        // gpui-component 0.5.1 的 `SettingField::dropdown` 内部走
                        // Button + PopupMenu，下拉高度硬编码不暴露。改走
                        // `SettingField::render` + 原生 `Select` + `SearchableVec`：
                        // 顶部带搜索框、剩余项可滚动，主题列表长时也能快速定位。
                        // SelectState 由 `SettingsPage::new` 缓存（见 struct 字段说明），
                        // 闭包里只复用，**不要**在这里 `cx.new(...)` —— 重建会导致
                        // click 不响应（见 struct 注释里的详细解释）。
                        SettingItem::new(
                            ts("Settings.item.theme"),
                            SettingField::render({
                                let theme_state = self.theme_state.clone();
                                move |options, _window, _cx| {
                                    // 主题下拉默认宽度太窄，长主题名（如
                                    // "Ayu Light / Ayu Dark / ..."）会被截断。
                                    // `.min_w_48()` = 12rem = 192px 兜底，
                                    // 同时按 layout 设主宽度：horizontal → 256px
                                    // （与 `SettingField::input` 默认行为一致），
                                    // 其他 → 占满整行。`with_size(options.size)`
                                    // 保持与同 group 其他 field 字号一致。
                                    let mut el = Select::new(&theme_state)
                                        .with_size(options.size)
                                        .min_w_48();
                                    if options.layout.is_horizontal() {
                                        el = el.w_64();
                                    } else {
                                        el = el.w_full();
                                    }
                                    el
                                }
                            }),
                        )
                        .description(ts("Settings.desc.theme"),),
                        // -- 界面语言（AppLang：应用 UI 语言）--
                        SettingItem::new(
                            ts("Settings.item.app_lang"),
                            SettingField::dropdown(
                                app_lang_options,
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        let cur = m.read(cx).config.app_lang;
                                        SharedString::from(cur.as_str())
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: SharedString, cx: &mut App| {
                                        let Some(lang) = AppLang::parse(&val) else {
                                            return;
                                        };
                                        m.update(cx, |model, _| {
                                            model.config.app_lang = lang;
                                            model.persist_settings();
                                        });
                                        // 立即更新 gpui-component 内部 i18n locale
                                        // （Sidebar 搜索框 placeholder / Select placeholder / …）
                                        // 并强制所有窗口重 render，让 t!("...") 重新读取 locale。
                                        gpui_component::set_locale(locale_for(lang));
                                        cx.refresh_windows();
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.app_lang"),),
                    ]),
                    // 网络
                    SettingGroup::new().title(ts("Settings.group.network")).items(vec![
                        // -- GitHub 代理 --
                        SettingItem::new(
                            ts("Settings.item.gh_proxy"),
                            SettingField::input(
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        SharedString::from(m.read(cx).config.gh_proxy.clone())
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: SharedString, cx: &mut App| {
                                        let s = val.to_string();
                                        m.update(cx, |model, _| {
                                            model.config.gh_proxy = s;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.gh_proxy"),),
                        // -- Cloudflare bypass --
                        SettingItem::new(
                            ts("Settings.item.cf_bypass"),
                            SettingField::input(
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        SharedString::from(m.read(cx).config.cf_bypass.clone())
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: SharedString, cx: &mut App| {
                                        let s = val.to_string();
                                        m.update(cx, |model, _| {
                                            model.config.cf_bypass = s;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.cf_bypass"),),
                    ]),
                    // 下载
                    SettingGroup::new().title(ts("Settings.group.download")).items(vec![
                        // -- 下载目录（带「浏览…」图标，点击调 rfd 选目录）--
                        // gpui-component 0.5.1 的 `SettingField::input` 只能给裸 Input
                        // 没法挂 suffix icon。改走 `SettingField::render` + 原生
                        // `Input::new(&self.download_path_input).suffix(Button::...)`。
                        // InputState 缓存到 `SettingsPage` struct（和 theme_state 同理，
                        // 避免 click / focus / 输入内容在每次 render 后丢失），rfd 选
                        // 完目录回写 model + notify，下一次 render 走 `sync_download_path`
                        // 把 model 的新值推回 InputState。
                        SettingItem::new(
                            ts("Settings.item.download_path"),
                            SettingField::render({
                                let download_path_input = self.download_path_input.clone();
                                let pick_folder_listener = self.pick_folder_listener.clone();
                                move |options, _window, _cx| {
                                    // 宽度要手动设：SettingField::input 内部 `.w_64()` /
                                    // `.w_full()` 依 layout，不设的话 input 渲染成 0
                                    // 大小 → text 被裁切看不见、suffix button 没 hit area
                                    // → click 不响应。详见 `string.rs:76-86`。
                                    let mut el = Input::new(&download_path_input)
                                        .with_size(options.size)
                                        .suffix({
                                            // ghost + xsmall 让 button 视觉上就是 icon，
                                            // 不抢 input 焦点、看起来像 input 的一部分。
                                            // input_story.rs:240 用的就是这个 pattern。
                                            //
                                            // **click handler**用 owner-cache 的
                                            // `pick_folder_listener`（见 SettingsPage struct
                                            // 注释）—— render 闭包拿不到 `Context<Self>`，
                                            // 在这里现建 `cx.listener` 不可行；早先尝试
                                            // 「`page_handle.update(cx, |_page, ctx| cx.spawn(...))`」
                                            // 双层套娃在 GPUI 0.2.2 下 click 不触发。
                                            //
                                            // `Rc<dyn Fn + 'static>::as_ref()` 拿到的是
                                            // `&'a Rc<dyn Fn>`，**不是 `'static`** —— `Button::on_click`
                                            // 要 `impl Fn + 'static`，传引用被拒。包一层
                                            // `move |...| listener(...)` 转成新的
                                            // `impl Fn + 'static`：捕获 Rc（'static），
                                            // 内部走 Rc::deref 调底层闭包。
                                            let listener = pick_folder_listener.clone();
                                            Button::new("download-path-pick")
                                                .ghost()
                                                .icon(IconName::FolderOpen)
                                                .xsmall()
                                                .on_click(move |ev, window, app| {
                                                    listener(ev, window, app)
                                                })
                                        });
                                    // horizontal layout → 固定 256px（与 `SettingField::input`
                                    // 默认行为一致）；其它 → 占满整行。
                                    if options.layout.is_horizontal() {
                                        el = el.w_64();
                                    } else {
                                        el = el.w_full();
                                    }
                                    el
                                }
                            }),
                        )
                        .description(ts("Settings.desc.download_path"),),
                        // -- 默认格式 --
                        SettingItem::new(
                            ts("Settings.item.default_format"),
                            SettingField::dropdown(
                                ext_options,
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        let cur = m.read(cx).config.ext_name;
                                        SharedString::from(ext_value(cur))
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: SharedString, cx: &mut App| {
                                        let Some(ext) = ext_from_str(&val) else {
                                            return;
                                        };
                                        m.update(cx, |model, _| {
                                            model.config.ext_name = ext;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.default_format"),),
                        // -- TXT 编码 --
                        SettingItem::new(
                            ts("Settings.item.txt_encoding"),
                            SettingField::dropdown(
                                encoding_options,
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        SharedString::from(m.read(cx).config.txt_encoding.clone())
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: SharedString, cx: &mut App| {
                                        let s = val.to_string();
                                        m.update(cx, |model, _| {
                                            model.config.txt_encoding = s;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.txt_encoding"),),
                        // -- 保留章节缓存 --
                        SettingItem::new(
                            ts("Settings.item.preserve_chapter_cache"),
                            SettingField::switch(
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.preserve_chapter_cache
                                },
                                {
                                    let m = m.clone();
                                    move |val: bool, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.preserve_chapter_cache = val;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.preserve_chapter_cache"),),
                        // -- 启用下载进度条 --
                        SettingItem::new(
                            ts("Settings.item.enable_progressbar"),
                            SettingField::switch(
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.enable_progressbar
                                },
                                {
                                    let m = m.clone();
                                    move |val: bool, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.enable_progressbar = val;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.enable_progressbar"),),
                    ]),
                ]),
            // ============ Page 2: 抓取 ============
            SettingPage::new(ts("Settings.page.crawl"))
                .resettable(false)
                .default_open(true)
                .groups(vec![
                    // 书源
                    SettingGroup::new().title(ts("Settings.group.source")).items(vec![
                        // -- 书源语言（LangType：书源筛选的 locale hint）--
                        SettingItem::new(
                            ts("Settings.item.book_lang"),
                            SettingField::dropdown(
                                lang_options,
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        let cur = m.read(cx).config.language;
                                        SharedString::from(cur.as_str())
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: SharedString, cx: &mut App| {
                                        let Some(lang) = LangType::parse(&val) else {
                                            return;
                                        };
                                        m.update(cx, |model, _| {
                                            model.config.language = lang;
                                            model.persist_settings();
                                        });
                                        // 不调 set_locale —— 这是**书源**语言，
                                        // 不影响 gpui-component 内部 i18n。
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.book_lang"),),
                        // -- 搜索条数上限（Option<i32>, -1 = 不限）--
                        SettingItem::new(
                            ts("Settings.item.search_limit"),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: -1.0,
                                    max: 10_000.0,
                                    ..Default::default()
                                },
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        m.read(cx).config.search_limit.map(|v| v as f64).unwrap_or(-1.0)
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: f64, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.search_limit =
                                                if val < 0.0 { None } else { Some(val as i32) };
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.search_limit"),),
                        // -- 过滤低相似度 --
                        SettingItem::new(
                            ts("Settings.item.search_filter"),
                            SettingField::switch(
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.search_filter
                                },
                                {
                                    let m = m.clone();
                                    move |val: bool, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.search_filter = val;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.search_filter"),),
                    ]),
                    // 并发与间隔
                    SettingGroup::new().title(ts("Settings.group.concurrency")).items(vec![
                        // -- 并发上限（Option<i32>, -1 = 自动）--
                        SettingItem::new(
                            ts("Settings.item.concurrency"),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: -1.0,
                                    max: 100.0,
                                    ..Default::default()
                                },
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        m.read(cx).config.concurrency.map(|v| v as f64).unwrap_or(-1.0)
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: f64, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.concurrency =
                                                if val < 0.0 { None } else { Some(val as i32) };
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.concurrency"),),
                        // -- 请求间隔 min --
                        SettingItem::new(
                            ts("Settings.item.min_interval"),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 0.0,
                                    max: 60_000.0,
                                    ..Default::default()
                                },
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.min_interval as f64
                                },
                                {
                                    let m = m.clone();
                                    move |val: f64, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.min_interval = val.max(0.0) as u32;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.min_interval"),),
                        // -- 请求间隔 max --
                        SettingItem::new(
                            ts("Settings.item.max_interval"),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 0.0,
                                    max: 60_000.0,
                                    ..Default::default()
                                },
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.max_interval as f64
                                },
                                {
                                    let m = m.clone();
                                    move |val: f64, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.max_interval = val.max(0.0) as u32;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.max_interval"),),
                    ]),
                    // 重试
                    SettingGroup::new().title(ts("Settings.group.retry")).items(vec![
                        // -- 启用失败重试 --
                        SettingItem::new(
                            ts("Settings.item.enable_retry"),
                            SettingField::switch(
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.enable_retry
                                },
                                {
                                    let m = m.clone();
                                    move |val: bool, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.enable_retry = val;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.enable_retry"),),
                        // -- 最大重试次数 --
                        SettingItem::new(
                            ts("Settings.item.max_retries"),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 0.0,
                                    max: 20.0,
                                    ..Default::default()
                                },
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.max_retries as f64
                                },
                                {
                                    let m = m.clone();
                                    move |val: f64, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.max_retries = val.max(0.0) as u32;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.max_retries"),),
                        // -- 重试间隔 min --
                        SettingItem::new(
                            ts("Settings.item.retry_min_interval"),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 0.0,
                                    max: 60_000.0,
                                    ..Default::default()
                                },
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.retry_min_interval as f64
                                },
                                {
                                    let m = m.clone();
                                    move |val: f64, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.retry_min_interval = val.max(0.0) as u32;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.retry_min_interval"),),
                        // -- 重试间隔 max --
                        SettingItem::new(
                            ts("Settings.item.retry_max_interval"),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 0.0,
                                    max: 60_000.0,
                                    ..Default::default()
                                },
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.retry_max_interval as f64
                                },
                                {
                                    let m = m.clone();
                                    move |val: f64, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.retry_max_interval = val.max(0.0) as u32;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.retry_max_interval"),),
                    ]),
                ]),
            // ============ Page 3: 代理 ============
            SettingPage::new(ts("Settings.page.proxy"))
                .resettable(false)
                .default_open(true)
                .groups(vec![
                    // HTTP 代理
                    SettingGroup::new().title(ts("Settings.group.http_proxy")).items(vec![
                        // -- 启用 HTTP 代理 --
                        SettingItem::new(
                            ts("Settings.item.proxy_enabled"),
                            SettingField::switch(
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.proxy_enabled
                                },
                                {
                                    let m = m.clone();
                                    move |val: bool, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.proxy_enabled = val;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.proxy_enabled"),),
                        // -- 代理 Host --
                        SettingItem::new(
                            ts("Settings.item.proxy_host"),
                            SettingField::input(
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        SharedString::from(m.read(cx).config.proxy_host.clone())
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: SharedString, cx: &mut App| {
                                        let s = val.to_string();
                                        m.update(cx, |model, _| {
                                            model.config.proxy_host = s;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.proxy_host"),),
                        // -- 代理 Port --
                        SettingItem::new(
                            ts("Settings.item.proxy_port"),
                            SettingField::number_input(
                                NumberFieldOptions {
                                    min: 1.0,
                                    max: 65_535.0,
                                    ..Default::default()
                                },
                                {
                                    let m = m.clone();
                                    move |cx: &App| m.read(cx).config.proxy_port as f64
                                },
                                {
                                    let m = m.clone();
                                    move |val: f64, cx: &mut App| {
                                        m.update(cx, |model, _| {
                                            model.config.proxy_port = val as u16;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.proxy_port"),),
                    ]),
                    // Cookie
                    SettingGroup::new().title(ts("Settings.group.cookie")).items(vec![
                        // -- 起点 Cookie --
                        SettingItem::new(
                            ts("Settings.item.qidian_cookie"),
                            SettingField::input(
                                {
                                    let m = m.clone();
                                    move |cx: &App| {
                                        SharedString::from(m.read(cx).config.qidian_cookie.clone())
                                    }
                                },
                                {
                                    let m = m.clone();
                                    move |val: SharedString, cx: &mut App| {
                                        let s = val.to_string();
                                        m.update(cx, |model, _| {
                                            model.config.qidian_cookie = s;
                                            model.persist_settings();
                                        });
                                    }
                                },
                            ),
                        )
                        .description(ts("Settings.desc.qidian_cookie"),),
                    ]),
                ]),
            // ============ Page 4: 关于 ============
            SettingPage::new(ts("Settings.page.about"))
                .resettable(false)
                .default_open(true)
                .groups(vec![SettingGroup::new().title(ts("Settings.group.info")).items(vec![
                    // -- 版本（静态文本）--
                    SettingItem::new(
                        ts("Settings.item.version"),
                        SettingField::render(|_opts, _window, cx| {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("v{}", env!("CARGO_PKG_VERSION")))
                        }),
                    )
                    .description(ts("Settings.desc.version"),),
                    // -- 检查更新 / 下载新版 --
                    SettingItem::new(
                        ts("Settings.item.check_update"),
                        SettingField::render({
                            let m = m.clone();
                            move |_opts, _window, cx| {
                                // 网络请求在跑时 → Button::loading(true) 自动显示
                                // spinner + 屏蔽 click（gpui-component 0.5.1 button.rs:365：
                                // `!(self.disabled || self.loading) && self.on_click.is_some()`）。
                                let state = m.read(cx);
                                let checking = state.update_state.checking;
                                // 检查完成后若有新版本 → 按钮变"下载新版"跳浏览器。
                                if !checking
                                    && let Some(latest) =
                                        state.update_state.latest_version.as_deref()
                                    && latest.trim_start_matches('v')
                                        != env!("CARGO_PKG_VERSION")
                                {
                                    let ver = latest.trim_start_matches('v');
                                    return Button::new("check-update")
                                        .icon(Icon::new(IconName::ExternalLink))
                                        .label(
                                            ts_fmt(
                                                "Settings.download_new_version_button",
                                                &[("ver", ver)],
                                            )
                                            .to_string(),
                                        )
                                        .small()
                                        .on_click(|_ev, _window, cx| {
                                            cx.open_url(
                                                "https://github.com/AhJxs/so-novel-rs/releases/latest",
                                            );
                                        })
                                        .into_any_element();
                                }
                                Button::new("check-update")
                                    .icon(Icon::new(IconName::Loader))
                                    .label(ts("Settings.check_update_button"))
                                    .small()
                                    .disabled(checking)
                                    .loading(checking)
                                    .on_click({
                                        let m = m.clone();
                                        move |_ev, _window, cx| {
                                            m.update(cx, |model, _| {
                                                model.spawn_update_check();
                                            });
                                        }
                                    })
                                    .into_any_element()
                            }
                        }),
                    )
                    .description(ts("Settings.desc.check_update")),
                    // -- 项目主页 --
                    SettingItem::new(
                        ts("Settings.item.open_github"),
                        SettingField::render(|_opts, _window, _cx| {
                            Button::new("open-github")
                                .icon(Icon::new(IconName::Globe))
                                .label(ts("Settings.open_github_button"))
                                .small()
                                .on_click(|_ev, _window, cx| {
                                    cx.open_url("https://github.com/AhJxs/so-novel-rs");
                                })
                        }),
                    )
                    .description(ts("Settings.desc.open_github"),),
                ])]),
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

        let pages = self.build_pages();
        let app_lang = self.model.read(cx).config.app_lang;

        // **关键**：把 app_lang 塞进 `Settings::new(id)` 的 id 里。
        //
        // gpui-component 的 `Settings` 内部用 `window.use_keyed_state(self.id, ...)` 缓存
        // `SettingsState { search_input: Entity<InputState>, ... }`。**而 InputState 的
        // `placeholder(t!("Settings.search_placeholder"))` 是在 `use_keyed_state` 第一次
        // 执行时** 一次性求值的** —— `t!` 返回 `&'static str`，存到 `InputState.placeholder`
        // 字段后再 render 也不会重读。
        //
        // 所以仅靠 `gpui_component::set_locale()` + `cx.refresh_windows()` **不够** —— 全局
        // locale 确实换了，但 InputState 的 placeholder 字段还是旧值。
        //
        // 解法：让 id 包含 lang。切语言后 id 变 → `use_keyed_state` 看到新 key → 重建
        // `SettingsState`（包括新 InputState）→ 新 placeholder 走当前 `t!()` 拿新文案。
        //
        // 副作用：切语言会把当前选中的 page / group / 搜索框内容 / 滚动位置都重置回默认
        // （这些都存在 `use_keyed_state` 里）。**可接受** —— 切语言是低频操作，代价是
        // 用户要重新点开"抓取"之类的 sub-page，UX 上合理。
        //
        // `Settings::new` 接 `impl Into<ElementId>`，实际能用的是 `SharedString` /
        // `&'static str`（前者 `Arc<str>`，后者 static）。我们用 `SharedString`：每次
        // render 都 new 一个（内部 refcount +1，引用同样的字符串内容），lang 一变 id 就变。
        let id: SharedString = format!("settings-page-{}", app_lang.as_str()).into();

        // Settings 组件自带 sidebar（页切换）+ 主区布局。直接放满父容器即可。
        // `with_group_variant(Outline)` 给所有 group 加 1px 边框（`cx.theme().border`），
        // 视觉上把"外观 / 网络 / 下载 / …"等不同 group 清晰分开。
        Settings::new(id)
            .with_group_variant(GroupBoxVariant::Outline)
            .pages(pages)
    }
}

// -------- 常量 + helper --------

/// 7 种常见 TXT 编码。
const TXT_ENCODINGS: &[&str] = &[
    "UTF-8",
    "GBK",
    "GB18030",
    "Big5",
    "BIG5HKSCS",
    "UTF-16LE",
    "UTF-16BE",
];

fn ext_value(e: ExportFormat) -> &'static str {
    match e {
        ExportFormat::Epub => "epub",
        ExportFormat::Txt => "txt",
        ExportFormat::Html => "html",
        ExportFormat::Pdf => "pdf",
    }
}

fn ext_from_str(s: &str) -> Option<ExportFormat> {
    match s {
        "epub" => Some(ExportFormat::Epub),
        "txt" => Some(ExportFormat::Txt),
        "html" => Some(ExportFormat::Html),
        "pdf" => Some(ExportFormat::Pdf),
        _ => None,
    }
}
