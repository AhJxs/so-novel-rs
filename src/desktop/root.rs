//! 顶层 `RootView`: `TitleBar` + 可折叠 Sidebar + 内容区。
//!
//! - Sidebar: `SidebarMenuItem` × 6, 可折叠到 48px 图标宽度, `Cmd+B` 或 `TitleBar`
//!   最左侧按钮切换; **无 footer**。
//! - 内容区按 `current_page` 渲染对应 page; `cmd-1`~`cmd-6` 直接跳, `F6`/`Shift+F6`
//!   循环翻页, `Escape` 关顶层覆盖层由 `gpui_kit::base::Root` 处理。
//! - 覆盖层 (dialog / sheet / notification / menu) 由外层 `Root` 上挂的 `RootPlugin`
//!   叠加, 不由本 view 渲染。

use std::rc::Rc;

use gpui_kit::component::sidebar::{Sidebar, SidebarMenu, SidebarMenuItem, SidebarToggleButton};
use gpui_kit::component::{ActiveTheme as _, Icon, TitleBar, WindowExt as _};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, StyleRefinement, Styled,
    Window, div, px,
};
use rust_i18n::t;

use super::logo::render_logo;
use super::nav::{KEY_CONTEXT, NavPage};
use super::notifications::ui_event_to_notification;
use crate::desktop::model::AppModel;
use crate::desktop::pages::reader::{CloseReaderFn, ImmersiveFn};
use crate::desktop::pages::{
    LibraryPage, ReaderPage, SearchPage, SettingsPage, SourcesPage, TasksPage, ToolsPage,
};
use crate::desktop::{
    NextPage, PrevPage, ShowLibrary, ShowSearch, ShowSettings, ShowSources, ShowTasks, ShowTools,
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
    focus: FocusHandle,

    // 6 个 page entity 一次性创建, 跨切换保持内部状态 (输入框 / 滚动位置)。
    library_page: Entity<LibraryPage>,
    search_page: Entity<SearchPage>,
    tasks_page: Entity<TasksPage>,
    sources_page: Entity<SourcesPage>,
    tools_page: Entity<ToolsPage>,
    settings_page: Entity<SettingsPage>,

    /// 打开的阅读器；`Some` 时覆盖主区域并隐藏侧栏。
    reader: Option<Entity<ReaderPage>>,
}

impl RootView {
    /// 建根视图，并实例化六个页面各自持有同一个 `AppModel`。
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        // `Window::focus` 传 `cx` (Context 自动 deref 成 `&mut App`)。
        window.focus(&focus, cx);

        let library_page = cx.new(|cx| LibraryPage::new(model.clone(), window, cx));
        let search_page = cx.new(|cx| SearchPage::new(model.clone(), window, cx));
        let tasks_page = cx.new(|cx| TasksPage::new(model.clone(), window, cx));
        let sources_page = cx.new(|cx| SourcesPage::new(model.clone(), window, cx));
        let tools_page = cx.new(|cx| ToolsPage::new(model.clone(), window, cx));
        let settings_page = cx.new(|cx| SettingsPage::new(model.clone(), window, cx));

        // 书架卡片点击 → RootView::open_reader。用弱引用避免 RootView ↔ LibraryPage 循环持有。
        let root_handle = cx.entity().downgrade();
        library_page.update(cx, |page, _cx| {
            page.set_open_reader_handler(Rc::new(move |book_id, window, cx| {
                let _ = root_handle.update(cx, |root, cx| root.open_reader(book_id, window, cx));
            }));
        });

        // 从 config 恢复上次折叠状态, 否则每次启动 sidebar 都会弹开。
        let sidebar_collapsed = model.read(cx).config.global.sidebar_collapsed;

        // 关窗落盘有两条路由，缺一不可：
        // - 标题栏 X / Alt+F4 是原生 `WindowControlArea`，Windows 直接发 WM_CLOSE，
        //   窗口与 `RootView` 会在 `on_app_quit` 观察者运行前销毁（弱引用升级失败），
        //   所以必须在窗口级 `on_window_should_close` 里同步落盘，返回 true 放行关闭。
        // - Cmd+Q / 菜单退出 / 应用级退出走 `on_app_quit`；此时窗口可能已不存在，
        //   只能靠它兜底（macOS 的 terminate 也会先关窗口）。
        // `flush_before_quit` 幂等（`reader.take()` + `StateStore::close`），先后触发也安全。
        let weak_root = cx.entity().downgrade();
        window.on_window_should_close(cx, move |_window, cx| {
            let _ = weak_root.update(cx, Self::flush_before_quit);
            true
        });
        cx.on_app_quit(|root, cx| {
            root.flush_before_quit(cx);
            async {}
        })
        .detach();

        Self {
            model,
            current_page: NavPage::default(),
            sidebar_collapsed,
            focus,
            library_page,
            search_page,
            tasks_page,
            sources_page,
            tools_page,
            settings_page,
            reader: None,
        }
    }

    /// 打开阅读器；书已不在书架或格式不可读时推错误通知。
    pub fn open_reader(&mut self, book_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        // 已经开着阅读器时先把旧的按关闭契约收尾，再开新的：
        // 直接覆盖 `self.reader` 会跳过 `close_and_flush`，还会把沉浸全屏留在窗口上。
        if self.reader.is_some() {
            self.close_reader(window, cx);
        }
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            self.model.update(cx, |model, _| {
                model.push_error(t!("Library.error.book_not_found").to_string());
            });
            return;
        };
        // 文件丢了 / 格式读不了：按 spec 5.2 打开书籍信息（含重新定位入口），
        // 而不是开一个注定失败的阅读器、或只弹一条 toast。
        if book.missing || !book.format.is_readable() {
            self.library_page.update(cx, |page, cx| {
                page.open_info_dialog(book_id, window, cx);
            });
            return;
        }
        let model = self.model.clone();
        let reader = cx.new(|cx| ReaderPage::new(model, book, window, cx));
        let root_handle = cx.entity().downgrade();
        let close_handler: CloseReaderFn = Rc::new(move |window, cx| {
            let _ = root_handle.update(cx, |root, cx| root.close_reader(window, cx));
        });
        reader.update(cx, |page, _cx| page.set_close_handler(close_handler));
        let immersive_handle = cx.entity().downgrade();
        // `ImmersiveFn` 只在这里用于把 `Rc<closure>` 协转成 `Rc<dyn Fn(&mut App)>`。
        let immersive_handler: ImmersiveFn = Rc::new(move |cx| {
            let _ = immersive_handle.update(cx, |_root, cx| cx.notify());
        });
        reader.update(cx, |page, _cx| {
            page.set_immersive_handler(immersive_handler);
        });
        self.reader = Some(reader);
        cx.notify();
    }

    /// 关闭阅读器：落盘进度、恢复窗口装饰，并把键盘焦点交还主窗口。
    pub fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(reader) = self.reader.take() else {
            return;
        };
        let immersive = reader.read(cx).is_immersive();
        reader.update(cx, ReaderPage::close_and_flush);
        if immersive {
            window.toggle_fullscreen();
        }
        cx.notify();
        // 焦点原本在 `ReaderPage` 自己的 focus handle 上，entity 随本函数被丢掉后
        // 键盘不再有归属。显式交还 `RootView` 的 handle —— 阅读器分支直接 track 它，
        // 主视图分支经 `render_content` track 它，`KEY_CONTEXT` 都挂在两者的祖先根上，
        // 所以恢复焦点即可让 Cmd+3 等无需先点窗口。
        window.focus(&self.focus, cx);
    }

    /// 退出前的统一落盘点：`on_window_should_close`（原生关窗）与 `on_app_quit`
    /// （Cmd+Q / 应用级退出）都会调用；两条路由先后触发也不会丢数据（`reader.take()`
    /// 与 `StateStore::close` 幂等，重复调用最多少写一次已关闭的库）。
    ///
    /// 有阅读器就走它自己的关闭契约（采位置、存字号、落库），没有也至少把设置与书架写一次；
    /// 任务列表最后补一次（同步提交，返回即已 fsync），随后显式关闭状态库写 WAL 收尾标记。
    /// `StateStore::close` 幂等，实体析构时 `Tree::drop` 再碰到也只会是空操作。
    fn flush_before_quit(&mut self, cx: &mut Context<Self>) {
        if let Some(reader) = self.reader.take() {
            reader.update(cx, ReaderPage::close_and_flush);
        } else {
            self.model.update(cx, |model, _| {
                model.persist_settings();
                model.flush_library();
            });
        }
        self.model.update(cx, |model, _| {
            model.persist_tasks();
            model.state.close();
        });
    }

    fn navigate(&mut self, page: NavPage, cx: &mut Context<Self>) {
        if self.current_page != page {
            self.current_page = page;
            // 进设置页时重读系统代理：用户可能刚在 Clash 里切过开关，打开设置页
            // 就是为了看到最新探测结果（也顺手让 client 跟上）。
            if page == NavPage::Settings {
                self.model.update(cx, |m, _| m.refresh_system_proxy());
            }
            cx.notify();
        }
    }

    /// 切换 sidebar 折叠 (`ToggleSidebar` / `Cmd+B` / `SidebarToggleButton` 三处入口),
    /// 新值写回 config 并落盘; Cmd+B 频率低, 每次写盘可接受。
    #[tracing::instrument(name = "RootView::toggle_sidebar", skip_all)]
    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        // 阅读器是全屏阅读视图：侧边栏不可见，改它的折叠状态只会让用户莫名其妙。
        if self.reader.is_some() {
            return;
        }
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
            NavPage::Tools => self.tools_page.clone().into_any_element(),
            NavPage::Settings => self.settings_page.clone().into_any_element(),
        }
    }

    /// 构建左侧 Sidebar。展开 220px, 折叠 48px (菜单项收成 icon-only); header
    /// 两态都渲染 (保留 logo)。200ms 缓动由组件库内部提供。
    fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let collapsed = self.sidebar_collapsed;

        // 展开态行高被组件库定死为 h_7 (1.75rem), 折叠态却是 icon + 上下 p_2, 每项
        // 差一截, 各项叠起来就是切换时的「散开 → 收拢」。图标固定 1rem、纵向内边距
        // 0.375rem 后两态行高都是 1.75rem, 图标也正好填满展开态内容区。
        let item_style = StyleRefinement::default().py_1p5();

        let items: Vec<SidebarMenuItem> = [
            NavPage::Search,
            NavPage::Tasks,
            NavPage::Library,
            NavPage::Sources,
            NavPage::Tools,
            NavPage::Settings,
        ]
        .iter()
        .map(|page| {
            let active = *page == self.current_page;
            let page = *page;
            let mut item = SidebarMenuItem::new(page.label())
                .icon(Icon::new(page.icon()).size_4())
                .active(active)
                // 必须显式设 `accessibility_label`: 折叠态下菜单项只剩 icon,
                // 不设标签屏幕阅读器读不出这是哪个页面。
                .accessibility_label(page.label())
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.navigate(page, cx);
                }));
            *item.style() = item_style.clone();
            item
        })
        .collect();

        // Header: logo + 全大写细体 app 名。折叠态保留 logo, 文字由 `when(!collapsed)`
        // 隐藏; gpui 无 letter_spacing API, 靠大写 + 细体 + 小字营造 logo 字感。
        // 两态内边距不同 (header pt_3/pt_2、折叠态根节点多 gap_2、内容区 p_2/px_3),
        // 由 header 自身的 pt_1 / pb_4 补齐; 标题行高钉 20px 与 logo 同高,
        // 两态 logo 与首个菜单项的 y 坐标才一致 (实测见 tasks/todo.md)。
        let title_text = t!("App.title").to_uppercase();
        let header = div()
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .when(collapsed, Styled::pt_1)
            .when(!collapsed, Styled::pb_4)
            .child(render_logo(px(20.0)))
            .when(!collapsed, |h| {
                h.child(
                    div()
                        .text_sm()
                        .line_height(px(20.0))
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
        // Linux 自绘关闭按钮走 gpui-component 的 `ControlIcon`，直接 `window.remove_window()`：
        // 它既不过 `on_should_close`，也不会等 `on_app_quit`，窗口与 `RootView` 一起销毁，
        // 所以必须在这之前同步落盘。该回调只在 Linux 被读取（`TitleBar::on_close_window`
        // 内部 `cfg!(target_os = "linux")`）；Windows 关闭按钮是原生 `WindowControlArea`、
        // macOS 用系统按钮，注册了也不会被调用，不影响那两条路由。若极端情况下
        // `on_app_quit` 之后还能跑到，`flush_before_quit` 幂等，重复调用安全。
        let root_for_close = cx.entity().downgrade();
        TitleBar::new()
            .on_close_window(move |_ev, window, cx| {
                let _ = root_for_close.update(cx, Self::flush_before_quit);
                window.remove_window();
            })
            .child(
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

    /// 退出落盘收敛到三条路由，最终都进 `flush_before_quit`（见 `new` / `render_title_bar`）：
    /// - Cmd+Q / `Quit` action → `cx.quit()` → `on_app_quit`；
    /// - Windows / macOS 原生关窗（`WM_CLOSE` / `windowShouldClose`）→ `on_window_should_close`；
    /// - Linux 自绘标题栏关闭按钮 → `TitleBar::on_close_window`。
    ///
    /// 这里只负责按关闭契约收起阅读器，别在这里再刷一次书架。
    ///
    /// 单独拆出来是因为阅读器分支只挂它：阅读器内 Cmd+Q 仍可退出，但 Cmd+1..6 /
    /// F6 / Cmd+B 不该把用户从正在读的书里踢出去。
    fn bind_quit_action(root: Div, cx: &Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &super::Quit, window, cx| {
            this.close_reader(window, cx);
            cx.quit();
        }))
    }

    /// 9 个导航 action 的 listener 挂到传入的 div 上；先挂退出 listener，
    /// 保证非阅读器路径不会漏掉 Cmd+Q 的收起阅读器行为。
    fn bind_nav_actions(root: Div, cx: &Context<Self>) -> Div {
        let root = Self::bind_quit_action(root, cx);
        root.on_action(cx.listener(|this, _: &ShowSearch, window, cx| {
            this.close_reader(window, cx);
            this.navigate(NavPage::Search, cx);
        }))
        .on_action(cx.listener(|this, _: &ShowTasks, window, cx| {
            this.close_reader(window, cx);
            this.navigate(NavPage::Tasks, cx);
        }))
        .on_action(cx.listener(|this, _: &ShowLibrary, window, cx| {
            this.close_reader(window, cx);
            this.navigate(NavPage::Library, cx);
        }))
        .on_action(cx.listener(|this, _: &ShowSources, window, cx| {
            this.close_reader(window, cx);
            this.navigate(NavPage::Sources, cx);
        }))
        .on_action(cx.listener(|this, _: &ShowTools, window, cx| {
            this.close_reader(window, cx);
            this.navigate(NavPage::Tools, cx);
        }))
        .on_action(cx.listener(|this, _: &ShowSettings, window, cx| {
            this.close_reader(window, cx);
            this.navigate(NavPage::Settings, cx);
        }))
        .on_action(cx.listener(|this, _: &NextPage, window, cx| {
            this.close_reader(window, cx);
            let next = this.current_page.next();
            this.navigate(next, cx);
        }))
        .on_action(cx.listener(|this, _: &PrevPage, window, cx| {
            this.close_reader(window, cx);
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

        // 阅读器打开时覆盖整个主区域（Sidebar + TitleBar + 内容区），仅沉浸模式下也隐藏
        // TitleBar；关闭由 RootView 丢弃 entity 完成。退出旁路挂在这一分支：
        // 两个分支互斥（早返回），每帧只有一套 listener，不会双绑定。
        if let Some(reader) = self.reader.clone() {
            let immersive = reader.read(cx).is_immersive();
            // 阅读器内只保留 Cmd+Q：导航快捷键不挂在这个分支，Cmd+1..6 / F6
            // 与 Cmd+B 落地无 listener；阅读器自己的 Esc / 方向键仍由 `ReaderPage` 处理。
            return Self::bind_quit_action(
                div()
                    .track_focus(&self.focus)
                    .key_context(KEY_CONTEXT)
                    .flex()
                    .flex_col()
                    .size_full(),
                cx,
            )
            .when(!immersive, |this| this.child(self.render_title_bar(cx)))
            .child(div().flex_1().min_h_0().child(reader))
            .into_any_element();
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
            .into_any_element()
    }
}
