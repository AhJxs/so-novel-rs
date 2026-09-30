//! 顶层 `RootView`: `TitleBar` + 可折叠 Sidebar + 内容区。
//!
//! - Sidebar: `SidebarMenuItem` × 5, 可折叠到 48px 图标宽度, `Cmd+B` 或 `TitleBar`
//!   最左侧按钮切换; **无 footer**。
//! - 内容区按 `current_page` 渲染对应 page; `cmd-1`~`cmd-5` 直接跳, `F6`/`Shift+F6`
//!   循环翻页, `Escape` 关顶层覆盖层由 `gpui_kit::base::Root` 处理。
//! - 覆盖层 (dialog / sheet / notification / menu) 由外层 `Root` 上挂的 `RootPlugin`
//!   叠加, 不由本 view 渲染。

use gpui_kit::component::{
    ActiveTheme as _, Icon, TitleBar, WindowExt as _,
    sidebar::{Sidebar, SidebarMenu, SidebarMenuItem, SidebarToggleButton},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, Styled, Window, div, px,
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

/// Root view: sidebar shell + 当前页面。
pub struct RootView {
    /// `new()` 里 clone 给子 page; `toggle_sidebar` 读 / 写 `config.global.sidebar_collapsed` 并持久化。
    model: Entity<AppModel>,
    current_page: NavPage,
    sidebar_collapsed: bool,
    /// `new()` 里 `window.focus(&focus)` 让 `RootView` 拥有初始焦点 —— 否则
    /// `KEY_CONTEXT` 快捷键依赖 focus 落到哪个子元素, 不稳定。
    focus: gpui_kit::FocusHandle,

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
        // `Window::focus` 传 `cx` (Context 自动 deref 成 `&mut App`)。
        window.focus(&focus, cx);

        let library_page = cx.new(|cx| LibraryPage::new(model.clone(), window, cx));
        let search_page = cx.new(|cx| SearchPage::new(model.clone(), window, cx));
        let tasks_page = cx.new(|cx| TasksPage::new(model.clone(), window, cx));
        let sources_page = cx.new(|cx| SourcesPage::new(model.clone(), window, cx));
        let settings_page = cx.new(|cx| SettingsPage::new(model.clone(), window, cx));

        // 从 config 恢复上次折叠状态, 否则每次启动 sidebar 都会弹开。
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

    /// 切换 sidebar 折叠 (`ToggleSidebar` / `Cmd+B` / `SidebarToggleButton` 三处入口),
    /// 新值写回 config 并落盘; Cmd+B 频率低, 每次写盘可接受。
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

    /// 构建左侧 Sidebar。展开 220px, 折叠 48px (菜单项收成 icon-only); header
    /// 两态都渲染 (保留 logo)。200ms 缓动由组件库内部提供。
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
                // 必须显式设 `accessibility_label`: 折叠态下菜单项只剩 icon,
                // 不设标签屏幕阅读器读不出这是哪个页面。
                .accessibility_label(page.label())
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.navigate(page, cx);
                }))
        })
        .collect();

        // Header: logo + 全大写细体 app 名。折叠态保留 logo, 文字由 `when(!collapsed)`
        // 隐藏; gpui 无 letter_spacing API, 靠大写 + 细体 + 小字营造 logo 字感。
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

        Sidebar::new("app-sidebar")
            .w(px(220.0))
            .collapsible(true)
            .collapsed(collapsed)
            .border_color(cx.theme().border)
            .header(header)
            .child(SidebarMenu::new().children(items))
    }

    /// 渲染 gpui-kit 组件库的 `TitleBar`（左侧 `SidebarToggleButton`, 右侧自动 `WindowControls`）。
    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        // `SidebarToggleButton::on_click` 收 `Fn(...)` 而非 listener, 用 entity.update 桥接。
        let root_entity = cx.entity();
        TitleBar::new().child(
            // 必须用 `occlude()` 把按钮从祖先 drag hitbox 里「挖」出来:
            // gpui-kit 组件库 `TitleBar` 把整条 children 行标成 `window_control_area(Drag)`,
            // Windows 上 NCHITTEST 对按钮区返回 HTCAPTION, OS 按下即接管为拖窗,
            // 按钮收不到 mouse_up → click 失效 (悬浮有反应但点不动)。occlude 让该点
            // NCHITTEST 落回 HTCLIENT, 再在 mousedown 上 `stop_propagation` 免得
            // TitleBar 自己的拖窗监听抢走手势。
            div()
                .id("sidebar-toggle-hitbox")
                .flex()
                .h_full()
                .items_center()
                .occlude()
                .on_mouse_down(MouseButton::Left, |_ev, _window, cx| {
                    cx.stop_propagation();
                })
                .child(
                    SidebarToggleButton::new()
                        .collapsed(self.sidebar_collapsed)
                        .on_click(move |_ev, _window, app_cx| {
                            root_entity.update(app_cx, |this, ctx| {
                                this.toggle_sidebar(ctx);
                            });
                        }),
                ),
        )
    }

    /// 按 `current_page` 渲染对应 page entity。
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

    /// 8 个导航 action 的 listener 挂到传入的 div 上。
    fn bind_nav_actions(root: gpui_kit::Div, cx: &Context<Self>) -> gpui_kit::Div {
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
        // 排空 `AppModel::pending_ui_events` (`events::drain` 没有 `&mut Window`, 只能先
        // 入队), 翻译成 `Notification` 并 push。`mem::take` 整取而非逐个 pop, 避免遍历
        // 中 model 被追加事件。
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
    }
}
