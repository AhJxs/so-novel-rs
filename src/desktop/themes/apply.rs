//! 主题应用 + 主题枚举。
//!
//! [`apply_theme_pref`] 是核心入口: 把 [`ThemePref`] 装到全局 `Theme`。
//! [`apply_font_size`] 在装主题之后单独调, 覆盖 `apply_config` 重置的字号。
//! [`list_theme_names`] / [`list_theme_names_by_mode`] 供设置页 Select 用。

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeMode, ThemeRegistry};
use gpui_kit::{App, SharedString, Window, px};

use crate::config::{ThemeDynMode, ThemeKind, ThemePref};

use super::embedded::{FONT_SIZE_MAX, FONT_SIZE_MIN};

/// 把字号写进全局 `Theme.font_size` 并刷新所有窗口。
///
/// `Root` 的 `WindowState` plugin 每帧 `prepare` 里用
/// `window.set_rem_size(cx.theme().font_size)` 把主题字号设成 rem 基准,
/// 组件全用 `rems(...)` 缩放, 所以改这一个字段 = 全局等比缩放。
///
/// **走 `Theme::update`（0.7.0 新增）而不是 `Theme::global_mut` + `refresh_windows`**:
/// `update` 是唯一的「事务写」入口 —— 它会把 tokens 与 colors 重新 `reconcile`,
/// `sync_base` 把改动投影到 gpui-base 层（base 组件读的是投影后的主题）,
/// 最后 `refresh_windows` 触发整 app 重 render。0.6 时代手动 `global_mut` 不经过
/// `sync_base`, base 层投影会停留在旧值。
///
/// `size` 会被钳到 `[FONT_SIZE_MIN, FONT_SIZE_MAX]`, 防止配置被手改成越界值后 UI 失控。
#[tracing::instrument(name = "themes::apply_font_size", skip_all, fields(size))]
pub fn apply_font_size(size: f32, cx: &mut App) {
    let size = size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
    tracing::Span::current().record("size", size);
    Theme::update(cx, |theme| theme.font_size = px(size));
}

/// 解析主题名 → `ThemeConfig`。空串 / 找不到时返回 `None`。
fn lookup_theme(name: &str, cx: &App) -> Option<Rc<ThemeConfig>> {
    if name.is_empty() {
        return None;
    }
    let key = SharedString::from(name.to_string());
    ThemeRegistry::global(cx).themes().get(&key).cloned()
}

/// 应用主题偏好到全局 `Theme`, 并刷新所有窗口。
///
/// 两种模式 (见 [`ThemePref`]):
/// - **Static**: `static_name` 同时塞进浅/深两槽 → `apply_config`, 整 app 不随系统明暗切换。
/// - **Dynamic**: `dyn_light` / `dyn_dark` 分别装进两槽 (找不到/空 → registry 默认),
///   再按 `dyn_mode` 调 `Theme::change` 选激活槽; `system` 走 `sync_system_appearance` 跟 OS。
///
/// **关键: 双槽都装**, 不能只 `apply_config` 单槽 —— 否则 `Theme::change` 读的是槽引用,
/// 没装就 fallback 默认主题, 且残留另一槽引用。
///
/// **写路径必须走 `Theme::update`（gpui-kit 0.7 新增）而不是 `Theme::global_mut`**:
/// `update` 在闭包执行**前**快照旧 theme, 闭包后再做三件事 ——
/// 1. `tokens.reconcile(colors, colors_before, tokens_before)` 把 token 与 color 收敛到
///    同一个值 (主题文件里的 gradient token 会保留: 配置文件同时写了 color + token);
/// 2. `fonts_changed` 比对**闭包前**的 font 家族, 变了才 `resolve_default_font` ——
///    0.6 时代先 `global_mut` 再 `Theme::change`, `edit` 快照的是**已经改过**的字号/字体,
///    于是 `fonts_changed` 恒为 false, 主题自带的字体永远不会被 resolve;
/// 3. `sync_base` 把改动投影到 gpui-base 层 + `refresh_windows`。
///
/// 三件事都只发生在 `Theme::update` / `Theme::change` 的事务写里, 手写 `global_mut`
/// 会全部跳过。Static 分支因此不再需要额外调一次 `Theme::change` —— `apply_config`
/// 自己就把 `mode` 切成主题的 mode, 剩下的收尾 `update` 已经做了。
///
/// `window`: 启动 `on_load` 拿不到 → 传 `None` (`cx.window_appearance()` 兜底);
/// 设置页实时改时传 `Some(window)` 拿到精确 appearance.
///
/// 找不到的主题名静默 fallback 到 registry 默认主题, 不 panic。
#[tracing::instrument(
    name = "themes::apply_pref",
    skip_all,
    fields(kind = ?pref.kind)
)]
pub fn apply_theme_pref(pref: &ThemePref, window: Option<&mut Window>, cx: &mut App) {
    match pref.kind {
        ThemeKind::Static => {
            let registry = ThemeRegistry::global(cx);
            let cfg = lookup_theme(&pref.static_name, cx).unwrap_or_else(|| {
                if !pref.static_name.is_empty() {
                    tracing::info!(
                        "static theme '{}' not in registry; using default (available: {})",
                        pref.static_name,
                        list_theme_names(cx).join(", ")
                    );
                }
                // 找不到 → 用 registry 当前激活 mode 的默认主题。
                if Theme::global(cx).mode.is_dark() {
                    registry.default_dark_theme().clone()
                } else {
                    registry.default_light_theme().clone()
                }
            });

            // 双槽同塞: Static 不区分明暗 —— 切到 Static 后不会被残留槽影响 (之前 Dynamic
            // 选过的另一 mode 主题残留不会再回来)。`apply_config` 会把 mode 切成
            // `cfg.mode`、装载 colors / tokens / 字体 / 圆角。
            Theme::update(cx, |theme| {
                theme.light_theme = cfg.clone();
                theme.dark_theme = cfg.clone();
                theme.apply_config(&cfg);
            });
        }
        ThemeKind::Dynamic => {
            let registry = ThemeRegistry::global(cx);
            let default_light = registry.default_light_theme().clone();
            let default_dark = registry.default_dark_theme().clone();

            let light_cfg = lookup_theme(&pref.dyn_light, cx)
                .filter(|c| !c.mode.is_dark())
                .unwrap_or_else(|| {
                    if !pref.dyn_light.is_empty()
                        && lookup_theme(&pref.dyn_light, cx).is_some_and(|c| c.mode.is_dark())
                    {
                        // 用户给浅槽选了个深色主题 → 过滤掉, 回落默认浅色 (设置页 UI 也会
                        // 过滤, 这里是防御性兜底)。
                        tracing::info!(
                            "dyn_light '{}' is a dark theme; using default light",
                            pref.dyn_light
                        );
                    }
                    default_light
                });
            let dark_cfg = lookup_theme(&pref.dyn_dark, cx)
                .filter(|c| c.mode.is_dark())
                .unwrap_or_else(|| {
                    if !pref.dyn_dark.is_empty()
                        && lookup_theme(&pref.dyn_dark, cx).is_some_and(|c| !c.mode.is_dark())
                    {
                        tracing::info!(
                            "dyn_dark '{}' is a light theme; using default dark",
                            pref.dyn_dark
                        );
                    }
                    default_dark
                });

            // 双槽装好 → 下面的 `Theme::change` / `sync_system_appearance` 内部
            // `edit(reload_mode = true)` 会 `apply_config` 当前 mode 的那一槽。
            Theme::update(cx, |theme| {
                theme.light_theme = light_cfg;
                theme.dark_theme = dark_cfg;
            });

            match pref.dyn_mode {
                ThemeDynMode::System => Theme::sync_system_appearance(window, cx),
                ThemeDynMode::Light => Theme::change(ThemeMode::Light, None, cx),
                ThemeDynMode::Dark => Theme::change(ThemeMode::Dark, None, cx),
            }
        }
    }
    // 不需要额外的 `cx.refresh_windows()` 兜底: 上面每条路径都以
    // `Theme::update` / `Theme::change` / `sync_system_appearance` 收尾,
    // 它们内部都会 `sync_base` + `refresh_windows`。
}

/// 列出当前可用的所有主题变体名 (按 name 字典序)。
///
/// `HashMap` 迭代顺序不稳定, 必须显式排序才能给 Select 稳定选项顺序。
pub fn list_theme_names(cx: &App) -> Vec<SharedString> {
    let mut names: Vec<SharedString> = ThemeRegistry::global(cx).themes().keys().cloned().collect();
    names.sort_by_key(|a| a.to_lowercase());
    names
}

/// 列出指定模式 (light / dark) 的主题变体名 (按 name 字典序)。
///
/// 动态模式选浅/深主题时用: 过滤掉与目标 mode 不符的变体, 避免用户把深色主题选进浅色槽。
pub fn list_theme_names_by_mode(cx: &App, dark: bool) -> Vec<SharedString> {
    let mut names: Vec<SharedString> = ThemeRegistry::global(cx)
        .themes()
        .iter()
        .filter(|(_, cfg)| cfg.mode.is_dark() == dark)
        .map(|(n, _)| n.clone())
        .collect();
    names.sort_by_key(|a| a.to_lowercase());
    names
}
