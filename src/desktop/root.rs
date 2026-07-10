//! 顶层 `RootView`: `TitleBar` + 可折叠 Sidebar + 内容区 + 覆盖层。
//!
//! - 左侧 `Sidebar`: `SidebarMenuItem` × 5 (Search / Tasks / Library / Sources / Settings),
//!   可折叠到 48px 图标宽度 (`SidebarCollapsible::Icon`, 200ms 缓动); 折叠按钮在 `TitleBar`
//!   最左侧, `Cmd+B` 快捷键也可切换。**无 footer** — sidebar 只渲染 menu + header。
//! - 右侧内容区: 按 `current_page` 渲染对应 page (`SettingsPage` 用 gpui-component
//!   `Settings` 组件搭)。
//! - GPUI actions + keybindings: `cmd-1`~`cmd-5` 直接跳, `F6`/`Shift+F6` 循环翻页,
//!   `cmd-b` 折叠 sidebar; `Escape` 由 `gpui-component::Root` 自动处理顶层覆盖层关闭。
//! - 顶层覆盖层走 `Root::render_dialog_layer / sheet_layer / notification_layer`。
//!
//! 子模块:
//! - [`super::logo`] — sidebar logo 解码 + 渲染
//! - [`super::nav`] — `NavPage` enum + actions + key bindings
//! - [`super::notifications`] — `UIEvent → Notification` 翻译层

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, Root, TitleBar, WindowExt as _,
    sidebar::{Sidebar, SidebarMenu, SidebarMenuItem, SidebarToggleButton},
};

use crate::desktop::model::AppModel;
use crate::desktop::pages::{LibraryPage, SearchPage, SettingsPage, SourcesPage, TasksPage};

use super::logo::render_logo;
use super::nav::{KEY_CONTEXT, NavPage};
use super::notifications::ui_event_to_notification;
use crate::desktop::{
    NextPage, PrevPage, ShowLibrary, ShowSearch, ShowSettings, ShowSources, ShowTasks,
    ToggleSidebar,
};

/// Root view: sidebar shell + 当前页面占位。
pub struct RootView {
    /// 1) `new()` 里 clone 给子 page; 2) `toggle_sidebar` 读 / 写 `config.global.sidebar_collapsed` 并持久化。
    model: Entity<AppModel>,
    current_page: NavPage,
    /// 初始值来自 `AppConfig.sidebar_collapsed`, 翻转后写回 config + 落盘, 重启保持。
    sidebar_collapsed: bool,
    /// `new()` 里 `window.focus(&focus)` 让 `RootView` 拥有初始焦点 —— `KEY_CONTEXT`
    /// 绑定的快捷键 (`F6` / `Cmd+1..5`) 稳定 fire, 不依赖 focus 落到哪个子元素。
    focus: gpui::FocusHandle,

    // 5 个 page entity 一次性创建, 跨切换保持内部状态 (输入框 / 滚动位置)。
    library_page: Entity<LibraryPage>,
    search_page: Entity<SearchPage>,
    tasks_page: Entity<TasksPage>,
    sources_page: Entity<SourcesPage>,
    settings_page: Entity<SettingsPage>,
}

impl RootView {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);

        let library_page = cx.new(|cx| LibraryPage::new(model.clone(), window, cx));
        let search_page = cx.new(|cx| SearchPage::new(model.clone(), window, cx));
        let tasks_page = cx.new(|cx| TasksPage::new(model.clone(), window, cx));
        let sources_page = cx.new(|cx| SourcesPage::new(model.clone(), window, cx));
        let settings_page = cx.new(|cx| SettingsPage::new(model.clone(), window, cx));

        // 从 config 恢复上次折叠状态 — 不持久化的话, 每次启动 sidebar 都会弹开。
        let sidebar_collapsed = model.read(cx).config.global.sidebar_collapsed;

        Self {
            model,
            current_page: NavPage::default(),
            sidebar_collapsed,
            focus,
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

    /// 切换 sidebar 折叠 (`ToggleSidebar` action / `Cmd+B` / `SidebarToggleButton` 三处入口)。
    /// 新值写回 `AppConfig.sidebar_collapsed` 并落盘, 重启保留; Cmd+B 频率低, 每次写盘可接受。
    #[tracing::instrument(name = "RootView::toggle_sidebar", skip_all)]
    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.sidebar_collapsed = !self.sidebar_collapsed;
        let new_value = self.sidebar_collapsed;
        self.model.update(cx, |m, _| {
            m.config.global.sidebar_collapsed = new_value;
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

    /// 构建左侧 Sidebar。支持折叠 (`sidebar_collapsed`):
    /// - 展开: 宽 220px, header 显示 "SO NOVEL" + 5 个 `SidebarMenuItem` (Search / Tasks /
    ///   Library / Sources / Settings)。
    /// - 折叠: 宽 48px (gpui-component `COLLAPSED_WIDTH`), 菜单项自动收成 icon-only
    ///   (`Collapsible` trait 隐藏文字); header 仍渲染 (保留 logo), 文字由 `when(!collapsed)` 隐藏。
    /// - **无 footer** — 不调 `.footer(...)`, gpui-component 内部 `when_some` 跳过。
    ///
    /// 200ms `ease_in_out_cubic` 缓动由 `gpui-component::Sidebar` 内部 `Transition` 提供。
    /// 折叠按钮在 `TitleBar` 最左侧 (见 `render_title_bar`)。
    fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
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

        // Header: logo + 全大写细体 app 名, 水平居中。
        // 折叠态保留 logo, 文字由 `when(!collapsed)` 隐藏 —— 48px 装不下整串文字。
        // 项目名 i18n 三语都是 "So Novel", `to_uppercase()` 转大写; gpui 无 letter_spacing
        // API, 靠大写 + 细体 + 小字营造 logo 字感。
        let title_text = crate::i18n::ts("App.title").to_uppercase();
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .child(render_logo(px(20.0)))
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
    /// 最左侧 `SidebarToggleButton` (默认 small ghost 样式), 右侧自动 `WindowControls`。
    /// `TitleBar` 按平台处理 (`WindowDecorations::Client` 在 `mod.rs` 设置):
    /// macOS traffic lights 自动 / Windows 自定义 34px 按钮 / Linux 自定义。
    /// 背景色 / 底边走 `cx.theme().title_bar[_border]`, 自动主题适配; 整个左半区域是
    /// drag area (toggle button 自己消费 mousedown, 不会触发拖动)。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        // `SidebarToggleButton::on_click` 收 `Fn(&ClickEvent, &mut Window, &mut App)`,
        // 没法直接用 cx.listener, 走 entity.update 桥接到 `toggle_sidebar`。
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

    /// 右侧内容区。按 `current_page` 渲染对应 page entity。
    fn render_content(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .flex_1()
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_current_page())
    }

    /// 8 个导航 action 的 listener 挂到传入的 div 上, 返回挂好后的 div。
    /// 抽出到独立方法, 避免 render 主体被 action 链淹没。
    /// 不取 `&self` —— 只用 `cx` 就能 `cx.listener(...)`, 避免 `unused_self`。
    fn bind_nav_actions(root: gpui::Div, cx: &Context<Self>) -> gpui::Div {
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
        // 排空 `AppModel::pending_ui_events` —— `events::drain` 没 `&mut Window`,
        // 把构造好的 `UIEvent` 推到 model 队列; 这里拿到 window 后真正 push。
        //
        // 用 `mem::take` 拿走整个 Vec 而不是逐个 pop: 避免 1) drain 期间 model
        // 被其他路径新增 UIEvent 时迭代器失效; 2) 多次 push 同一帧时把
        // 全部都消费完。
        //
        // 翻译层 `ui_event_to_notification` 在这里完成 `UIEvent → Notification` 的转换,
        // 包括 `OpenLink` 变体的 `on_click(cx.open_url)` 挂载。
        let pending = std::mem::take(
            &mut self
                .model
                .update(cx, |m, _| std::mem::take(&mut m.pending_ui_events)),
        );
        for ev in pending {
            window.push_notification(ui_event_to_notification(ev), cx);
        }

        Self::bind_nav_actions(div().key_context(KEY_CONTEXT), cx)
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
            // Root 的覆盖层: dialog / sheet / notification。
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}
