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
use crate::config::AppLang;

pub mod components;
pub mod i18n;
mod pages;
mod root;
pub mod themes;
pub use root::{NavPage, RootView};

/// 把 `AppConfig.app_lang`（应用 UI 语言）映射到 gpui-component 接受的 locale 字符串。
///
/// gpui-component 用 `rust_i18n` 做内部国际化（`locales/ui.yml`），内置 4 种 locale：
/// `en` / `zh-CN` / `zh-HK` / `it`（`fallback = "en"`，找不到 key 就退回英文）。
///
/// 我们的 `AppLang` 3 个值映射：
/// - `ZhCn`  → `"zh-CN"` （精确匹配）
/// - `ZhTw`  → `"zh-HK"` （传统中文；gpui-component 没有 `zh-TW`，fallback 用 `zh-HK`）
/// - `En`    → `"en"`   （精确匹配）
///
/// 不在列表内的 locale rust_i18n 自动 fallback 到 `en`，所以传 `zh-TW` 也会显示英文
/// —— 显式映射到 `zh-HK` 让传统中文用户能直接看到中文 UI（gpui-component
/// 内部 zh-CN/zh-HK 的简体/繁体翻译完全一样）。
///
/// 何时调用：
/// 1. **启动时**（`gpui_app::run`）—— 把 `config.app_lang` 同步给 gpui-component，
///    让 Sidebar 搜索框 placeholder / Select placeholder / Dialog OK|Cancel 等
///    内部文案立刻用对语言。
/// 2. **用户改语言时**（settings page 的 `界面语言` setter）—— `set_locale` 立即生效 +
///    `cx.refresh_windows()` 触发整 app 重 render，所有 `t!("...")` 重新读取 locale。
///
/// 注意：**只**对应"应用 UI 语言"（`AppLang`），跟"书源语言"（`LangType`）无关。
/// `LangType` 是书源筛选用的 locale hint，不影响 gpui-component 内部 i18n。
pub fn locale_for(lang: AppLang) -> &'static str {
    match lang {
        AppLang::ZhCn => "zh-CN",
        AppLang::ZhTw => "zh-HK",
        AppLang::En => "en",
    }
}

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
        //    themes 目录 = `~/.sonovel/themes/`（首次启动写入 21 个 embed，
        //    之后用户可手动放自定义 *.json 进去热加载）。
        let (app_paths, saved_theme) = {
            let s = model.read(cx);
            (s.paths.clone(), s.config.theme.clone())
        };
        themes::init(cx, &app_paths, &saved_theme);

        // 5. 把 `AppConfig.app_lang`（应用 UI 语言）同步给 gpui-component —— 影响内部
        //    Sidebar 搜索 placeholder / Select placeholder / Dialog OK|Cancel 等所有
        //    `t!()` 调用的文案。必须在开任何带 Sidebar / Select / Dialog 的窗口前调用，
        //    否则首次 render 就会用错误的 fallback locale。
        gpui_component::set_locale(locale_for(model.read(cx).config.app_lang));

        // 6. 居中开窗 + 最小尺寸 + 自定义 TitleBar 配置。
        use gpui::{px, size};
        let window_size = size(px(1200.0), px(800.0));
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

        // 7. Root 包装 RootView（持有 AppModel + sidebar + TitleBar）。
        cx.open_window(opts, |window, cx| {
            let view = cx.new(|cx| RootView::new(model.clone(), window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .expect("open_window failed");
    });

    Ok(())
}
