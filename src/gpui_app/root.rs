//! Stage 4 + Stage 12：现代侧边栏 Shell + 键盘打磨。
//!
//! - 左侧 `Sidebar`：`SidebarMenuItem` × 5（Search / Tasks / Library / Sources / Settings）。
//! - 右侧内容区：按 `current_page` 渲染对应 page。
//! - GPUI actions + keybindings：
//!   - `ShowSearch/Tasks/Library/Sources/Settings` + `cmd-1` ~ `cmd-5` 直接跳。
//!   - `NextPage` / `PrevPage` + `ctrl-tab` / `ctrl-shift-tab` 循环翻页（Stage 12）。
//!   - `Escape` 由 `gpui-component::Root` 自动处理关闭顶层 dialog / sheet / notification。
//! - 顶层 `Root::render_dialog_layer / sheet_layer / notification_layer` 渲染覆盖层。

use gpui::{
    actions, div, px, AnyElement, App, AppContext, ClickEvent, Context, Entity, InteractiveElement,
    IntoElement, KeyBinding, ParentElement, Render, Styled, Window,
};
use gpui_component::{
    sidebar::{Sidebar, SidebarMenu, SidebarMenuItem},
    ActiveTheme as _, Icon, IconName, Root, TitleBar,
};

use crate::app::AppModel;
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
    ]
);

/// GPUI key context 名。`div().key_context("AppShell")` 时激活。
const KEY_CONTEXT: &str = "AppShell";

/// 新 GUI 的 5 个一级导航页面（Stage 4 起替代旧 `crate::ui::nav::NavPage`）。
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
    /// 5 个 nav item 的元数据：(label_zh, icon, 默认 page 名占位)。
    /// Stage 5+ 替换为真实页面。
    fn label_zh(self) -> &'static str {
        match self {
            NavPage::Search => "搜索下载",
            NavPage::Tasks => "下载任务",
            NavPage::Library => "本地书库",
            NavPage::Sources => "书源管理",
            NavPage::Settings => "设置",
        }
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
/// Stage 12 加 `F6` / `Shift+F6` 翻页：
/// - `F6` → 下一页（搜索 → 任务 → 书库 → 书源 → 设置 → 搜索 ...）
/// - `Shift+F6` → 上一页
///
/// 保留 `cmd-1..5` 直接跳。
///
/// 为什么不用 `Ctrl+Tab` / `Ctrl+Shift+Tab`：
/// `gpui-component::InputState` 把 `tab` / `shift-tab` 绑到了自己的 `IndentInline` / `OutdentInline`
/// 动作（用于多行输入的 tab 字符插入）。当焦点在 Input 上时，**Tab 整个事件被 Input 消费**，
/// 即使是 `ctrl-tab` 在某些平台上也会被 Input 的 keydown handler 先处理（事件冒泡被 stop）
/// → 应用级翻页 action 拿不到事件。改用 `F6` 避开 Input 的 Tab 绑定。
pub fn register_key_bindings(cx: &mut App) {
    cx.bind_keys([
        // 直接跳
        KeyBinding::new("cmd-1", ShowSearch, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-2", ShowTasks, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-3", ShowLibrary, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-4", ShowSources, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-5", ShowSettings, Some(KEY_CONTEXT)),
        // 翻页 — F6 避开 Input 的 Tab 绑定
        KeyBinding::new("f6", NextPage, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f6", PrevPage, Some(KEY_CONTEXT)),
    ]);
}

/// Stage 4 root view：sidebar shell + 当前页面占位。
pub struct RootView {
    // model 在 new() 时持有；后续 stage 用作 sidebar 状态展示。当前 render 没读它。
    #[allow(dead_code)]
    model: Entity<AppModel>,
    current_page: NavPage,
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

        // 一次性创建所有 page。Library 是 Stage 6 真实实现；其他四个当前是占位，
        // Stage 7/8/9/10 替换。
        let library_page = cx.new(|cx| LibraryPage::new(model.clone(), window, cx));
        let search_page = cx.new(|cx| SearchPage::new(model.clone(), window, cx));
        let tasks_page = cx.new(|cx| TasksPage::new(model.clone(), window, cx));
        let sources_page = cx.new(|cx| SourcesPage::new(model.clone(), window, cx));
        let settings_page = cx.new(|cx| SettingsPage::new(model.clone(), window, cx));

        Self {
            model,
            current_page: NavPage::default(),
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

    /// 构建左侧 Sidebar。
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            SidebarMenuItem::new(page.label_zh())
                .icon(Icon::new(page.icon()))
                .active(active)
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.navigate(page, cx);
                }))
        })
        .collect();

        let header = div()
            .px_3()
            .py_2() // 高度小一点（原 .py_3）— 紧凑标题区
            .flex()
            .flex_row()
            .items_center()    // 垂直居中
            .justify_center()  // 水平居中（图标 + 文字一起居中显示）
            .gap_2()
            .child(Icon::new(IconName::BookOpen).text_color(cx.theme().primary))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_base()
                    .child("So Novel"),
            );

        let footer = div()
            .px_3()
            .py_2()
            .flex()
            .flex_col()
            .gap_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(format!("v{}", env!("CARGO_PKG_VERSION")))
            .child(div().child("Cmd+1..5 直达"))
            .child(div().child("F6 / Shift+F6 翻页"));

        Sidebar::left()
            .w(px(220.0))
            .bg(cx.theme().sidebar)
            .border_color(cx.theme().border)
            .header(header)
            .footer(footer)
            .child(SidebarMenu::new().children(items))
    }

    /// 渲染 gpui-component `TitleBar`。
    ///
    /// **完全空白 — 不传任何 children**。TitleBar 自身按平台处理（`WindowDecorations::Client`
    /// 在 `mod.rs` 设置）：
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
    /// - Linux 可选 `on_close_window` 回调（已 ready，未挂 — 默认行为：关闭窗口）
    fn render_title_bar(&self) -> impl IntoElement {
        // 仅 TitleBar 本身 — 右侧自动渲染 WindowControls。
        TitleBar::new()
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

    /// 7 个导航 action 的 listener 挂到传入的 div 上，返回挂好后的 div。
    /// 抽出到独立方法，避免 render 主体被 action 链淹没。
    fn bind_nav_actions(&self, root: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        root.on_action(cx.listener(Self::navigate_to::<ShowSearch>))
            .on_action(cx.listener(Self::navigate_to::<ShowTasks>))
            .on_action(cx.listener(Self::navigate_to::<ShowLibrary>))
            .on_action(cx.listener(Self::navigate_to::<ShowSources>))
            .on_action(cx.listener(Self::navigate_to::<ShowSettings>))
            .on_action(cx.listener(Self::cycle_page::<NextPage>))
            .on_action(cx.listener(Self::cycle_page::<PrevPage>))
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
        self.bind_nav_actions(div().key_context(KEY_CONTEXT), cx)
            .size_full()
            .flex()
            .flex_col()
            .child(self.render_title_bar())
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
