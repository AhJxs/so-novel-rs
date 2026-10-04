//! GUI 栈（gpui-kit）。
//!
//! - `RootView` 是应用内容视图（`TitleBar` + Sidebar + 内容区）; dialog / sheet /
//!   notification 覆盖层由 `gpui_kit::open_window` 包出的 `Root` 上挂的 `RootPlugin` 渲染。
//! - 页面在 `pages/`, 共享组件在 `components/`, 后台通道 → UI 重绘走 `drain_loop`。
//!
//! 本模块仅依赖 gpui-kit + 业务模块（`crate::desktop::model`）。

use anyhow::Result;
use gpui_kit::component::TitleBar;
use gpui_kit::{
    App, AppContext, Bounds, WindowBackgroundAppearance, WindowBounds, WindowOptions, actions, px,
    size,
};

use crate::desktop::model::AppModel;

// `actions!` 生成的类型落在调用点 (即 `desktop::*`), 供 root.rs / nav.rs 引用。
actions!(
    desktop,
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
// 为 actions! 生成的 PartialEq 单元结构体补充 Eq 实现。
impl Eq for ShowSearch {}
impl Eq for ShowTasks {}
impl Eq for ShowLibrary {}
impl Eq for ShowSources {}
impl Eq for ShowSettings {}
impl Eq for NextPage {}
impl Eq for PrevPage {}
impl Eq for ToggleSidebar {}

pub mod components;
mod drain_loop;
mod logo;
pub mod model;
mod nav;
mod notifications;
mod pages;
mod root;
pub mod themes;
pub use nav::{NavPage, register_key_bindings};
pub use root::RootView;

/// 把 `AppConfig.language`（应用语言）映射到 `gpui_kit::component` 接受的 locale 字符串。
///
/// **只**对应"应用 UI 语言"（`Language`），跟"书源语言"（`LangType`）无关。
use crate::i18n::locale_for;

/// 启动 GPUI 应用。`main.rs` 在无参数分支调用。
///
/// 启动顺序：`component::init` → 创建 `Entity<AppModel>` → 注册快捷键 →
/// 启动 drain 循环 → 加载 themes → `set_locale` → 开窗。
///
/// `title: None` — OS 任务栏标题仍由 `RootView` 内的 `TitleBar` 渲染。
/// `appears_transparent: true` — 让 OS 不画原生 chrome（触发 `hide_title_bar = true`，
/// Windows 平台据此响应 `WM_NCHITTEST`，3 个按钮才有点击处理）。
///
/// 注意：不要同时设 `window_decorations: Some(WindowDecorations::Client)` —— 与
/// `appears_transparent: true` 组合会破坏 Windows 平台的事件处理。
///
/// # Panics
///
/// `cx.open_window` 失败时（仅在 `WindowOptions` 非法或 GPU 已满载时）弹错误对话框后
/// 直接 `return`, 不会 panic; 显式处理是为了避免用户看到空白窗口以为还在加载。
pub fn run() -> Result<()> {
    let app = gpui_kit::application().with_assets(gpui_kit::assets::Assets);
    app.run(move |cx: &mut App| {
        // rust-i18n 扩展注册 —— **必须在 `component::init` 之前**, 且只调一次。
        // 方向: 组件查 key 时先查我们的 `app.yml`, 查不到再回落组件内置 `ui.yml`（反向不成立）。
        // `extend!` 收 ident, 所以必须先 `use ... as gpui_component`。
        {
            use gpui_kit::component as gpui_component;
            rust_i18n::extend!(gpui_component);
        }

        // 必须在第一个窗口前调用。
        gpui_kit::component::init(cx);

        // 1. 创建 AppModel。启动期致命错误 → 弹原生对话框后退出 GPUI 循环, 不开窗口。
        let (model, wakeup_rx) = match AppModel::new_with_wakeup() {
            Ok((m, rx)) => (cx.new(|_cx| m), rx),
            Err(e) => {
                tracing::error!("AppModel 初始化失败: {e:#}");
                rfd::MessageDialog::new()
                    .set_title("So Novel 启动失败")
                    .set_description(format!(
                        "初始化失败，无法启动应用：\n\n{e:#}\n\n请检查磁盘权限或重装应用。"
                    ))
                    .set_level(rfd::MessageLevel::Error)
                    .show();
                return;
            }
        };

        register_key_bindings(cx);

        drain_loop::spawn_drain_loop(&model, wakeup_rx, cx);

        // 4. 加载 themes/*.json 到 ThemeRegistry（on_load 里 apply + refresh）。
        let (app_paths, theme_pref, font_size) = {
            let s = model.read(cx);
            (
                s.paths.clone(),
                s.config.global.theme_pref.clone(),
                s.config.global.font_size,
            )
        };
        themes::init(cx, &app_paths, &theme_pref, font_size);

        // 5. 把 `AppConfig.language` 同步给 gpui_kit::component, 必须在开窗**前**调,
        //    否则首帧用错 fallback locale。
        gpui_kit::component::set_locale(locale_for(model.read(cx).config.global.language));

        let window_size = size(px(1200.0), px(800.0));
        let min_size = size(px(900.0), px(600.0));
        #[allow(unused_mut)] // mut 仅 Linux cfg 块使用
        let mut opts = WindowOptions {
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
        // Linux WM 不会因 appears_transparent 自动隐藏原生标题栏，需要 Client
        // decorations 抑制；Windows 上此组合会破坏事件。同时切 Transparent 背景，
        // 让 GPU shader 渲染的 CSD 圆角 alpha 能穿透。
        #[cfg(target_os = "linux")]
        {
            opts.window_decorations = Some(gpui_kit::WindowDecorations::Client);
        }

        // 7. 开窗：应用内容 view 直接返回 `RootView`, **不要**自己再包一层 `Root`
        //    （会嵌套 Root / 覆盖层挂错层）。
        if let Err(e) = gpui_kit::open_window(opts, cx, |window, cx| {
            cx.new(|cx| RootView::new(model.clone(), window, cx))
        }) {
            tracing::error!("open_window 失败: {e:#}");
            rfd::MessageDialog::new()
                .set_title("So Novel 启动失败")
                .set_description(format!("无法打开主窗口：\n\n{e:#}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
    });

    Ok(())
}
