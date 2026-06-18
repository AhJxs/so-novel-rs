//! 顶层 `RootView`：TitleBar + 可折叠 Sidebar + 内容区 + 覆盖层。
//!
//! - 左侧 `Sidebar`：`SidebarMenuItem` × 5（Search / Tasks / Library / Sources / Settings）。
//!   - 可折叠：`SidebarCollapsible::Icon` — 折叠到 48px 图标宽度，展开/收起 200ms 缓动。
//!   - `SidebarToggleButton` 放在 TitleBar 最左侧；`Cmd+B` 快捷键也可切换。
//!   - **无 footer** — sidebar 只渲染 menu + header，干净。
//! - 右侧内容区：按 `current_page` 渲染对应 page（`SettingsPage` 也用 gpui-component
//!   的 `Settings` 组件搭）。
//! - GPUI actions + keybindings：
//!   - `ShowSearch/Tasks/Library/Sources/Settings` + `cmd-1` ~ `cmd-5` 直接跳。
//!   - `NextPage` / `PrevPage` + `f6` / `shift-f6` 循环翻页。
//!   - `ToggleSidebar` + `cmd-b` 折叠/展开侧边栏。
//!   - `Escape` 由 `gpui-component::Root` 自动处理关闭顶层 dialog / sheet / notification。
//! - 顶层 `Root::render_dialog_layer / sheet_layer / notification_layer` 渲染覆盖层。

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, InteractiveElement, IntoElement,
    KeyBinding, ParentElement, Render, SharedString, Styled, Window, actions, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Root, TitleBar, WindowExt as _,
    sidebar::{Sidebar, SidebarMenu, SidebarMenuItem, SidebarToggleButton},
};

use crate::app::AppModel;
use crate::gpui_app::i18n::ts;
use crate::gpui_app::pages::{LibraryPage, SearchPage, SettingsPage, SourcesPage, TasksPage};

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
/// `F6` / `Shift+F6` 翻页：
/// - `F6` → 下一页（搜索 → 任务 → 书库 → 书源 → 搜索 ...）
/// - `Shift+F6` → 上一页
///
/// `cmd-1..4` 跳 page，`cmd-5` 打开设置窗口，`cmd-b` 折叠 sidebar。
///
/// 为什么不用 `Ctrl+Tab` / `Ctrl+Shift+Tab`：
/// `gpui-component::InputState` 把 `tab` / `shift-tab` 绑到了自己的 `IndentInline` / `OutdentInline`
/// 动作（用于多行输入的 tab 字符插入）。当焦点在 Input 上时，**Tab 整个事件被 Input 消费**，
/// 即使是 `ctrl-tab` 在某些平台上也会被 Input 的 keydown handler 先处理（事件冒泡被 stop）
/// → 应用级翻页 action 拿不到事件。改用 `F6` 避开 Input 的 Tab 绑定。
pub fn register_key_bindings(cx: &mut App) {
    cx.bind_keys([
        // 直接跳（5 个 page）
        KeyBinding::new("cmd-1", ShowSearch, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-2", ShowTasks, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-3", ShowLibrary, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-4", ShowSources, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-5", ShowSettings, Some(KEY_CONTEXT)),
        // 翻页 — F6 避开 Input 的 Tab 绑定
        KeyBinding::new("f6", NextPage, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f6", PrevPage, Some(KEY_CONTEXT)),
        // 折叠/展开 sidebar（VSCode 风格 cmd-b）
        KeyBinding::new("cmd-b", ToggleSidebar, Some(KEY_CONTEXT)),
    ]);
}

/// Root view：sidebar shell + 当前页面占位。
pub struct RootView {
    // 持有 model 用于：1) `new()` 里 clone 给子 page；2) `toggle_sidebar` 读 / 写
    // `config.sidebar_collapsed` 并触发持久化。
    model: Entity<AppModel>,
    current_page: NavPage,
    /// Sidebar 是否折叠（true = 仅图标宽度）。由 `toggle_sidebar` / `Cmd+B` 翻转。
    ///
    /// 初始值来自 `AppConfig.sidebar_collapsed`，每次翻转后写回 config 并自动
    /// 落盘（`persist_settings`），所以重启后保持上次状态。
    sidebar_collapsed: bool,
    /// 焦点 handle — 在 new() 里 window.focus(&_focus) 让 RootView 拥有初始焦点，
    /// 这样 `F6` / `Cmd+1..5` 等 KEY_CONTEXT 绑定的快捷键能稳定 fire
    /// （不依赖 focus 落到哪个具体子元素上）。
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

    /// 切换 sidebar 折叠状态。由 `ToggleSidebar` action / `Cmd+B` / `SidebarToggleButton` 三处入口调用。
    ///
    /// 同时把新值写回 `AppConfig.sidebar_collapsed` 并 `persist_settings()` 落盘 —
    /// 重启后保留。Cmd+B 频率很低，每次写盘完全可接受（config.toml 小，几 ms）。
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

    /// 设置 modal：半透明 backdrop + 居中可拖动 panel。
    /// 构建左侧 Sidebar。支持折叠（`sidebar_collapsed`）：
    ///
    /// - 展开态（默认）：宽 220px，header 显示 "So Novel" 文字（居中）；
    ///   5 个 `SidebarMenuItem`（Search / Tasks / Library / Sources / Settings）。
    /// - 折叠态：宽 48px（gpui-component `COLLAPSED_WIDTH`），**header 不渲染**；
    ///   5 个菜单项自动收成 icon-only。`SidebarMenuItem` 通过 `Collapsible` trait
    ///   自动隐藏文字。
    /// - **无 footer** — sidebar 只渲染 menu + header，干净。
    ///
    /// 折叠按钮在 TitleBar 最左侧（`render_title_bar`）；header 只作标题区，折叠后隐藏。
    ///
    /// 200ms `ease_in_out_cubic` 缓动由 `gpui-component::Sidebar` 内部 `Transition` 提供，
    /// 折叠/展开丝滑过渡。
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

        // Header — 仅展开态显示 "So Novel" 文字居中。折叠态不调用 `.header(...)`，
        // gpui-component 内部 `when_some(self.header.take(), ...)` 会跳过整个 header 容器，
        // 不占任何垂直空间。文本走 `i18n::tr`（项目名 3 种语言都是 "So Novel"，但 key
        // 还是走 i18n 表以保持一致性）。
        let header = div().w_full().flex().items_center().justify_center().child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_base()
                .child(ts("App.title")),
        );

        Sidebar::left()
            .w(px(220.0))
            // true → SidebarCollapsible::Icon：折叠到图标宽度（48px），带 200ms 缓动。
            .collapsible(true)
            .collapsed(collapsed)
            .bg(cx.theme().sidebar)
            .border_color(cx.theme().border)
            // 仅展开态注入 header。`header` 被闭包 move 捕获，未调用时直接 drop，
            // gpui-component 内部 `when_some(self.header.take(), ...)` 跳过整个 header 容器。
            .when(!collapsed, |sb| sb.header(header))
            // 无 footer —— 不调 `.footer(...)`，gpui-component 内部 `when_some` 跳过整个
            // footer 容器，sidebar 底部自然空白。
            .child(SidebarMenu::new().children(items))
    }

    /// 渲染 gpui-component `TitleBar`。
    ///
    /// **最左侧放 `SidebarToggleButton`**（用默认 small ghost 样式，24×24），
    /// 右侧自动渲染 `WindowControls`。TitleBar 自身按平台处理
    /// （`WindowDecorations::Client` 在 `mod.rs` 设置）：
    ///
    /// | 平台 | 左 padding | 按钮 | 双击 | 特殊 |
    /// |------|-----------|------|------|------|
    /// | macOS | 80px（让位 traffic lights） | 原生 traffic light | `titlebar_double_click` | 自动 appear transparent |
    /// | Windows | 12px | 自定义 34px 按钮（hover/active 状态）| OS 默认最大化 | 走系统集成 |
    /// | Linux | — | 自定义按钮（需手动 click）| `zoom_window` | 右键 = show_window_menu |
    ///
    /// 通用：
    /// - 背景色 + 底边：`cx.theme().title_bar` / `title_bar_border` — 自动主题适配
    /// - 整个左半区域 = drag area（鼠标拖动 → `start_window_move()`）通过 `WindowControlArea::Drag`
    ///   — 但 toggle button 自己消费 mousedown，不会触发拖动
    /// - Linux 可选 `on_close_window` 回调（已 ready，未挂 — 默认行为：关闭窗口）
    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // 闭包签名：`SidebarToggleButton::on_click` 收 `Fn(&ClickEvent, &mut Window, &mut App)`，
        // 不能直接用 cx.listener，所以走 entity.update 桥接到 `toggle_sidebar`。
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
        root.on_action(cx.listener(Self::navigate_to::<ShowSearch>))
            .on_action(cx.listener(Self::navigate_to::<ShowTasks>))
            .on_action(cx.listener(Self::navigate_to::<ShowLibrary>))
            .on_action(cx.listener(Self::navigate_to::<ShowSources>))
            .on_action(cx.listener(Self::navigate_to::<ShowSettings>))
            .on_action(cx.listener(Self::cycle_page::<NextPage>))
            .on_action(cx.listener(Self::cycle_page::<PrevPage>))
            .on_action(cx.listener(Self::toggle_sidebar_action))
    }

    /// `ToggleSidebar` action 入口。`on_click` 走 `toggle_sidebar` 直接调。
    fn toggle_sidebar_action(
        &mut self,
        _action: &ToggleSidebar,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_sidebar(cx);
    }

    /// 5 个 `ShowXxx` action → 跳到目标 page。
    fn navigate_to<T: gpui::Action>(
        &mut self,
        _action: &T,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = std::any::Any::type_id(_action);
        let page = if id == std::any::TypeId::of::<ShowSearch>() {
            NavPage::Search
        } else if id == std::any::TypeId::of::<ShowTasks>() {
            NavPage::Tasks
        } else if id == std::any::TypeId::of::<ShowLibrary>() {
            NavPage::Library
        } else if id == std::any::TypeId::of::<ShowSources>() {
            NavPage::Sources
        } else if id == std::any::TypeId::of::<ShowSettings>() {
            NavPage::Settings
        } else {
            return;
        };
        self.navigate(page, cx);
    }

    /// `NextPage` / `PrevPage` 共用 — direction +1 / -1。
    fn cycle_page<T: gpui::Action>(
        &mut self,
        _action: &T,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = std::any::Any::type_id(_action);
        let next = if id == std::any::TypeId::of::<NextPage>() {
            self.current_page.next()
        } else if id == std::any::TypeId::of::<PrevPage>() {
            self.current_page.prev()
        } else {
            return;
        };
        self.navigate(next, cx);
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 排空 `AppModel::pending_notifications` —— `events::drain` 没 `&mut Window`，
        // 把构造好的 `Notification` 推到 model 队列；这里拿到 window 后真正 push。
        //
        // 用 `mem::take` 拿走整个 Vec 而不是逐个 pop：避免 1) drain 期间 model
        // 被其他路径新增 notification 时迭代器失效；2) 多次 push 同一帧时把
        // 全部都消费完。
        let pending = std::mem::take(
            &mut self
                .model
                .update(cx, |m, _| std::mem::take(&mut m.pending_notifications)),
        );
        for note in pending {
            window.push_notification(note, cx);
        }

        self.bind_nav_actions(div().key_context(KEY_CONTEXT), cx)
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_title_bar(cx))
            // 主体：sidebar + 内容。`track_focus` 放在 body 内部，让 KEY_CONTEXT 上下文链稳定。
            .child(
                div()
                    .track_focus(&self._focus)
                    .flex_1()
                    .size_full()
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .child(self.render_sidebar(cx))
                    .child(self.render_content(cx)),
            )
            // Root 的覆盖层：dialog / sheet / notification。
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}
