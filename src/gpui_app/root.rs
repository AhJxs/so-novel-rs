//! 顶层 `RootView`：TitleBar + 可折叠 Sidebar + 内容区 + 覆盖层。
//!
//! - 左侧 `Sidebar`：`SidebarMenuItem` × 5（Search / Tasks / Library / Sources / Settings），
//!   可折叠到 48px 图标宽度（`SidebarCollapsible::Icon`，200ms 缓动）；折叠按钮在 TitleBar
//!   最左侧，`Cmd+B` 快捷键也可切换。**无 footer** — sidebar 只渲染 menu + header。
//! - 右侧内容区：按 `current_page` 渲染对应 page（`SettingsPage` 用 gpui-component
//!   `Settings` 组件搭）。
//! - GPUI actions + keybindings：`cmd-1`~`cmd-5` 直接跳，`F6`/`Shift+F6` 循环翻页，
//!   `cmd-b` 折叠 sidebar；`Escape` 由 `gpui-component::Root` 自动处理顶层覆盖层关闭。
//! - 顶层覆盖层走 `Root::render_dialog_layer / sheet_layer / notification_layer`。

use std::io::Cursor;
use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, FontWeight, ImageSource,
    InteractiveElement, IntoElement, KeyBinding, ObjectFit, ParentElement, Render, RenderImage,
    SharedString, Styled, StyledImage as _, Window, actions, div, img, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Root, TitleBar, WindowExt as _,
    notification::Notification,
    sidebar::{Sidebar, SidebarMenu, SidebarMenuItem, SidebarToggleButton},
};

use once_cell::sync::Lazy;

/// Sidebar header 用的小 logo（assets/logo.png 编译期嵌入）。
///
/// 用 `.png`（位图）而非 `.svg`：gpui 的 `img()` 不直接吃 SVG 字节——SVG 需要装到
/// asset loader 走 `AssetSource` + 内置 SVG 光栅化。本项目 assets loader 是
/// `gpui_component_assets::Assets`，不包含我们的 logo。最简、零运行时依赖路径就是
/// 嵌 PNG 字节 + `image` crate 解码成 `RenderImage`（流程同 `decode_cover_image`）。
const LOGO_PNG: &[u8] = include_bytes!("../../assets/logo.png");

/// 解码好的 logo（RGBA→BGRA swap 后的 `RenderImage`）。`Lazy` 启动首帧用一次，之后复用。
static LOGO_IMAGE: Lazy<Option<Arc<RenderImage>>> = Lazy::new(|| decode_logo_image(LOGO_PNG));

/// 解码 PNG 字节 → `RenderImage`。流程同 `decode_cover_image`，但 logo 是静态资源 → `Lazy` 缓存。
fn decode_logo_image(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let dynamic = reader.decode().ok()?;
    let mut rgba = dynamic.into_rgba8();
    // RGBA → BGRA：GPUI 纹理期望 BGRA 字节序（见 gpui img.rs L671-674 swap(0,2)）。
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let frame = image::Frame::new(rgba);
    Some(Arc::new(RenderImage::new(vec![frame])))
}

use crate::app::{AppModel, UIEvent};
use crate::gpui_app::pages::{LibraryPage, SearchPage, SettingsPage, SourcesPage, TasksPage};
use crate::i18n::ts;

actions!(
    gpui_app,
    [
        ShowSearch,
        ShowTasks,
        ShowLibrary,
        ShowSources,
        ShowSettings,
        NextPage,
        PrevPage,
        ToggleSidebar,
    ]
);

/// GPUI key context 名。`div().key_context("AppShell")` 时激活。
const KEY_CONTEXT: &str = "AppShell";

/// 5 个一级导航页面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavPage {
    #[default]
    Search,
    Tasks,
    Library,
    Sources,
    Settings,
}

impl NavPage {
    /// NavPage → i18n key（`i18n::tr` 用）。
    fn label_key(self) -> &'static str {
        match self {
            NavPage::Search => "Nav.search",
            NavPage::Tasks => "Nav.tasks",
            NavPage::Library => "Nav.library",
            NavPage::Sources => "Nav.sources",
            NavPage::Settings => "Nav.settings",
        }
    }

    /// 当前应用语言下的用户可见 label —— `t!` 走全局 locale（语言切换时由
    /// `gpui_component::set_locale` 同步），所以这里不需要 `lang` 参数。
    fn label(self) -> SharedString {
        ts(self.label_key())
    }

    fn icon(self) -> IconName {
        match self {
            NavPage::Search => IconName::Search,
            NavPage::Tasks => IconName::Inbox,
            NavPage::Library => IconName::BookOpen,
            NavPage::Sources => IconName::Globe,
            NavPage::Settings => IconName::Settings,
        }
    }

    /// 5 个 page 循环顺序：Search → Tasks → Library → Sources → Settings → Search。
    const ALL: [NavPage; 5] = [
        NavPage::Search,
        NavPage::Tasks,
        NavPage::Library,
        NavPage::Sources,
        NavPage::Settings,
    ];

    /// 下一个 page（循环）。
    fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// 上一个 page（循环）。
    fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// 全局 key bindings 注册。`gpui_app::run` 启动时调一次。
///
/// `cmd-1..5` 跳 5 个 page，`F6`/`Shift+F6` 循环翻页，`cmd-b` 折叠 sidebar。
///
/// 翻页不用 `Ctrl+Tab`：gpui-component `InputState` 把 `tab` / `shift-tab` 绑到自己的
/// `IndentInline` / `OutdentInline` 动作（多行输入 tab 插入），焦点在 Input 时 Tab 事件
/// 被 Input 消费（某些平台连 `ctrl-tab` 也被 keydown handler stop 冒泡），应用级翻页
/// action 拿不到。改用 `F6` 避开。
pub fn register_key_bindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-1", ShowSearch, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-2", ShowTasks, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-3", ShowLibrary, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-4", ShowSources, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-5", ShowSettings, Some(KEY_CONTEXT)),
        KeyBinding::new("f6", NextPage, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f6", PrevPage, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-b", ToggleSidebar, Some(KEY_CONTEXT)),
    ]);
}

/// Root view：sidebar shell + 当前页面占位。
pub struct RootView {
    /// 1) `new()` 里 clone 给子 page；2) `toggle_sidebar` 读 / 写 `config.sidebar_collapsed` 并持久化。
    model: Entity<AppModel>,
    current_page: NavPage,
    /// 初始值来自 `AppConfig.sidebar_collapsed`，翻转后写回 config + 落盘，重启保持。
    sidebar_collapsed: bool,
    /// new() 里 `window.focus(&_focus)` 让 RootView 拥有初始焦点 —— `KEY_CONTEXT`
    /// 绑定的快捷键（`F6` / `Cmd+1..5`）稳定 fire，不依赖 focus 落到哪个子元素。
    _focus: gpui::FocusHandle,

    // 5 个 page entity 一次性创建，跨切换保持内部状态（输入框 / 滚动位置）。
    library_page: Entity<LibraryPage>,
    search_page: Entity<SearchPage>,
    tasks_page: Entity<TasksPage>,
    sources_page: Entity<SourcesPage>,
    settings_page: Entity<SettingsPage>,
}

impl RootView {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _focus = cx.focus_handle();
        window.focus(&_focus);

        let library_page = cx.new(|cx| LibraryPage::new(model.clone(), window, cx));
        let search_page = cx.new(|cx| SearchPage::new(model.clone(), window, cx));
        let tasks_page = cx.new(|cx| TasksPage::new(model.clone(), window, cx));
        let sources_page = cx.new(|cx| SourcesPage::new(model.clone(), window, cx));
        let settings_page = cx.new(|cx| SettingsPage::new(model.clone(), window, cx));

        // 从 config 恢复上次折叠状态 — 不持久化的话，每次启动 sidebar 都会弹开。
        let sidebar_collapsed = model.read(cx).config.sidebar_collapsed;

        Self {
            model,
            current_page: NavPage::default(),
            sidebar_collapsed,
            _focus,
            library_page,
            search_page,
            tasks_page,
            sources_page,
            settings_page,
        }
    }

    fn navigate(&mut self, page: NavPage, cx: &mut Context<Self>) {
        if self.current_page != page {
            self.current_page = page;
            cx.notify();
        }
    }

    /// 切换 sidebar 折叠（`ToggleSidebar` action / `Cmd+B` / `SidebarToggleButton` 三处入口）。
    /// 新值写回 `AppConfig.sidebar_collapsed` 并落盘，重启保留；Cmd+B 频率低，每次写盘可接受。
    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        let new_value = self.sidebar_collapsed;
        self.model.update(cx, |m, _| {
            m.config.sidebar_collapsed = new_value;
            m.persist_settings();
        });
        cx.notify();
    }

    /// 渲染当前选中的 page。
    fn render_current_page(&self) -> AnyElement {
        match self.current_page {
            NavPage::Library => self.library_page.clone().into_any_element(),
            NavPage::Search => self.search_page.clone().into_any_element(),
            NavPage::Tasks => self.tasks_page.clone().into_any_element(),
            NavPage::Sources => self.sources_page.clone().into_any_element(),
            NavPage::Settings => self.settings_page.clone().into_any_element(),
        }
    }

    /// 渲染 logo 图片元素（正方形，object-fit contain）。
    ///
    /// 解码失败 → 返回空 div 占位，不让 UI 崩。`size` 走 `px()` 显式像素而非 rem：
    /// logo 是图标资源，不跟字号缩放。
    fn render_logo(size: gpui::Pixels) -> AnyElement {
        match LOGO_IMAGE.as_ref() {
            Some(rendered) => img(ImageSource::Render(rendered.clone()))
                .object_fit(ObjectFit::Contain)
                .size(size)
                .flex_shrink_0()
                .into_any_element(),
            None => div().size(size).flex_shrink_0().into_any_element(),
        }
    }

    /// 构建左侧 Sidebar。支持折叠（`sidebar_collapsed`）：
    /// - 展开：宽 220px，header 显示 "SO NOVEL" + 5 个 `SidebarMenuItem`（Search / Tasks /
    ///   Library / Sources / Settings）。
    /// - 折叠：宽 48px（gpui-component `COLLAPSED_WIDTH`），菜单项自动收成 icon-only
    ///   （`Collapsible` trait 隐藏文字）；header 仍渲染（保留 logo），文字由 `when(!collapsed)` 隐藏。
    /// - **无 footer** — 不调 `.footer(...)`，gpui-component 内部 `when_some` 跳过。
    ///
    /// 200ms `ease_in_out_cubic` 缓动由 `gpui-component::Sidebar` 内部 `Transition` 提供。
    /// 折叠按钮在 TitleBar 最左侧（见 `render_title_bar`）。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.sidebar_collapsed;

        let items: Vec<SidebarMenuItem> = [
            NavPage::Search,
            NavPage::Tasks,
            NavPage::Library,
            NavPage::Sources,
            NavPage::Settings,
        ]
        .iter()
        .map(|page| {
            let active = *page == self.current_page;
            let page = *page;
            SidebarMenuItem::new(page.label())
                .icon(Icon::new(page.icon()))
                .active(active)
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.navigate(page, cx);
                }))
        })
        .collect();

        // Header：logo + 全大写细体 app 名，水平居中。
        // 折叠态保留 logo，文字由 `when(!collapsed)` 隐藏 —— 48px 装不下整串文字。
        // 项目名 i18n 三语都是 "So Novel"，`to_uppercase()` 转大写；gpui 无 letter_spacing
        // API，靠大写 + 细体 + 小字营造 logo 字感。
        let title_text = ts("App.title").to_uppercase();
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .child(Self::render_logo(px(20.0)))
            .when(!collapsed, |h| {
                h.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::LIGHT)
                        .child(title_text),
                )
            });

        Sidebar::left()
            .w(px(220.0))
            .collapsible(true)
            .collapsed(collapsed)
            .border_color(cx.theme().border)
            .header(header)
            .child(SidebarMenu::new().children(items))
    }

    /// 渲染 gpui-component `TitleBar`。
    ///
    /// 最左侧 `SidebarToggleButton`（默认 small ghost 样式），右侧自动 `WindowControls`。
    /// TitleBar 按平台处理（`WindowDecorations::Client` 在 `mod.rs` 设置）：
    /// macOS traffic lights 自动 / Windows 自定义 34px 按钮 / Linux 自定义。
    /// 背景色 / 底边走 `cx.theme().title_bar[_border]`，自动主题适配；整个左半区域是
    /// drag area（toggle button 自己消费 mousedown，不会触发拖动）。
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // `SidebarToggleButton::on_click` 收 `Fn(&ClickEvent, &mut Window, &mut App)`，
        // 没法直接用 cx.listener，走 entity.update 桥接到 `toggle_sidebar`。
        let root_entity = cx.entity();
        TitleBar::new().child(
            SidebarToggleButton::left()
                .collapsed(self.sidebar_collapsed)
                .on_click(move |_ev, _window, app_cx| {
                    root_entity.update(app_cx, |this, ctx| {
                        this.toggle_sidebar(ctx);
                    });
                }),
        )
    }

    /// 右侧内容区。按 current_page 渲染对应 page entity。
    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self._focus)
            .flex_1()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_current_page())
    }

    /// 8 个导航 action 的 listener 挂到传入的 div 上，返回挂好后的 div。
    /// 抽出到独立方法，避免 render 主体被 action 链淹没。
    fn bind_nav_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(
            cx.listener(|this, _: &ShowSearch, _, cx| this.navigate(NavPage::Search, cx)),
        )
        .on_action(cx.listener(|this, _: &ShowTasks, _, cx| this.navigate(NavPage::Tasks, cx)))
        .on_action(cx.listener(|this, _: &ShowLibrary, _, cx| this.navigate(NavPage::Library, cx)))
        .on_action(cx.listener(|this, _: &ShowSources, _, cx| this.navigate(NavPage::Sources, cx)))
        .on_action(
            cx.listener(|this, _: &ShowSettings, _, cx| this.navigate(NavPage::Settings, cx)),
        )
        .on_action(cx.listener(|this, _: &NextPage, _, cx| {
            let next = this.current_page.next();
            this.navigate(next, cx);
        }))
        .on_action(cx.listener(|this, _: &PrevPage, _, cx| {
            let prev = this.current_page.prev();
            this.navigate(prev, cx);
        }))
        .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_sidebar(cx)))
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 排空 `AppModel::pending_notifications` —— `events::drain` 没 `&mut Window`，
        // 把构造好的 `UIEvent` 推到 model 队列；这里拿到 window 后真正 push。
        //
        // 用 `mem::take` 拿走整个 Vec 而不是逐个 pop：避免 1) drain 期间 model
        // 被其他路径新增 UIEvent 时迭代器失效；2) 多次 push 同一帧时把
        // 全部都消费完。
        //
        // 翻译层 `ui_event_to_notification` 在这里完成 `UIEvent → Notification` 的转换，
        // 包括 `OpenLink` 变体的 `on_click(cx.open_url)` 挂载。
        let pending = std::mem::take(
            &mut self
                .model
                .update(cx, |m, _| std::mem::take(&mut m.pending_notifications)),
        );
        for ev in pending {
            window.push_notification(ui_event_to_notification(ev), cx);
        }

        self.bind_nav_actions(div().key_context(KEY_CONTEXT), cx)
            .size_full()
            .flex()
            .flex_row()
            .child(self.render_sidebar(cx))
            .child(
                div()
                    .flex_1()
                    .size_full()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(self.render_title_bar(cx))
                    .child(self.render_content(cx)),
            )
            // Root 的覆盖层：dialog / sheet / notification。
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

/// `UIEvent` → `gpui_component::Notification` 翻译层。
///
/// `app/` 是 UI 框架解耦的，把意图（要弹什么 toast）以 plain enum 推到
/// `AppModel::pending_notifications`；UI 层 `RootView::render` 拿到 `&mut Window` 后
/// 把每个 `UIEvent` 翻译成 `Notification` 再 `window.push_notification(...)`。
///
/// 翻译层放 `gpui_app/`：`Notification::on_click` / `cx.open_url` 是 UI 框架 API，
/// 跨过去就破坏"app/ 零 GUI 依赖"。`OpenLink` 变体的 `on_click` 在这里挂。
fn ui_event_to_notification(ev: UIEvent) -> Notification {
    match ev {
        UIEvent::Info(s) => Notification::info(s),
        UIEvent::Success(s) => Notification::success(s),
        UIEvent::Warning(s) => Notification::warning(s),
        UIEvent::Error(s) => Notification::error(s),
        UIEvent::OpenLink { message, url } => Notification::new()
            .message(message)
            .on_click(move |_ev, _window, cx| {
                cx.open_url(&url);
            })
            .autohide(true),
    }
}
