//! 新 GUI 栈：GPUI + gpui-component。
//!
//! Stage 1：占位窗口 + gpui-component 主题已能渲染。
//! Stage 2：`AppModel` 已是 UI 中立结构。
//! Stage 3：后台通道 → GPUI 事件桥接已通过 `app::events::spawn_drain_loop` 跑起来。
//! Stage 4：sidebar shell（5 nav items + actions/keybindings + Root 覆盖层）。
//! Stage 5：共享 GPUI 组件（EmptyState / PageHeader / StatusBadge / 格式化工具）。
//! Stage 6-10：5 个 page（Library / Sources / Tasks / Settings / Search）。
//! Stage 11：egui 全部移除（~7000 行代码）。
//! Stage 12：窗口 Chrome（原生 title bar）+ 键盘导航打磨。
//!
//! 旧 egui 路径已完全删除；本模块仅依赖 GPUI + gpui-component + 业务模块。

use anyhow::Result;
use gpui::{
    App, AppContext, Bounds, Entity, WindowBackgroundAppearance, WindowBounds, WindowOptions,
};
use gpui_component::{Root, TitleBar};

use crate::app::{events, AppModel};

pub mod components;
mod pages;
mod root;
pub mod themes;
pub use root::{NavPage, RootView};

/// 启动 GPUI 应用。`main.rs` 在无参数分支调用。
///
/// 启动顺序：
/// 1. `gpui_component::init(cx)` — 主题 / 内置组件 / 资源；
/// 2. 创建 `Entity<AppModel>` — UI 中立的领域状态；
/// 3. `root::register_key_bindings(cx)` — 绑定 cmd-1..5 + Tab 切页快捷键；
/// 4. 启动 [`events::spawn_drain_loop`] — 每 100ms 排空后台通道 + `cx.notify()`；
/// 5. 打开窗口（**自定义 TitleBar** + native 拖拽 + 3 按钮）：
///    root 是 `Root`（包裹 [`RootView`]，持有 `AppModel` + sidebar + TitleBar + actions）。
///
/// Stage 12：参考官方 `gpui-component` example — 用 `TitleBar::title_bar_options()`
/// 配置 `WindowOptions.titlebar`：
/// - `title: None` — OS 任务栏仍会显示 "So Novel"（由 `RootView` 内的 TitleBar child 渲染标题）
/// - `appears_transparent: true` — 告诉 OS 不画原生 chrome；GPUI 接管所有视觉和事件
///   （关键：触发 `hide_title_bar = true`，让 Windows 平台响应 `WM_NCHITTEST`
///   返回 HTCLOSE / HTMINBUTTON / HTMAXBUTTON，从而触发 3 个按钮的点击处理）
///
/// 注意：不要同时设 `window_decorations: Some(WindowDecorations::Client)` — 与
/// `appears_transparent: true` 组合会破坏 Windows 平台的事件处理。GPUI 通过
/// `titlebar.appears_transparent` 已经能正确处理所有平台（macOS / Windows / Linux）。
pub fn run() -> Result<()> {
    let app = gpui::Application::new().with_assets(gpui_component_assets::Assets);
    app.run(move |cx: &mut App| {
        // 必须在第一个窗口前调用。
        gpui_component::init(cx);

        // 1. 创建 AppModel。
        let model: Entity<AppModel> = cx.new(|_cx| AppModel::new());

        // 2. 注册快捷键。
        root::register_key_bindings(cx);

        // 3. 启动 drain 循环（内部 detach）。
        events::spawn_drain_loop(model.clone(), cx);

        // 4. 加载 themes/*.json 到 ThemeRegistry（on_load 里 apply + refresh）。
        let saved_theme = model.read(cx).config.theme.clone();
        themes::init(cx, &saved_theme);

        // 5. 居中开窗 + 最小尺寸 + 自定义 TitleBar 配置。
        use gpui::{px, size};
        let window_size = size(px(1180.0), px(760.0));
        let min_size = size(px(900.0), px(600.0));
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                window_size,
                cx,
            ))),
            window_min_size: Some(min_size),
            window_background: WindowBackgroundAppearance::Opaque,
            titlebar: Some(TitleBar::title_bar_options()),
            ..Default::default()
        };

        // 6. Root 包装 RootView（持有 AppModel + sidebar + TitleBar）。
        cx.open_window(opts, |window, cx| {
            let view = cx.new(|cx| RootView::new(model.clone(), window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("open_window failed");
    });

    Ok(())
}
