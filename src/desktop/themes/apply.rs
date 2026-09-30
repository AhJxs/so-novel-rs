//! 主题应用 + 主题枚举。
//!
//! [`apply_theme_pref`] 把 [`ThemePref`] 装到全局 `Theme`; [`apply_font_size`] 在装主题
//! 之后单独调, 覆盖 `apply_config` 重置的字号; `list_theme_names*` 供设置页 Select 用。

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeMode, ThemeRegistry};
use gpui_kit::{App, SharedString, Window, px};

use crate::config::{ThemeDynMode, ThemeKind, ThemePref};

use super::embedded::{FONT_SIZE_MAX, FONT_SIZE_MIN};

/// 把字号写进全局 `Theme.font_size` 并刷新所有窗口。`Root` 每帧 `prepare` 里 `set_rem_size` 把它设为
/// rem 基准, 组件全用 `rems(...)` 缩放, 所以改这一个字段 = 全局等比缩放。
///
/// **必须走 `Theme::update` 而不是 `Theme::global_mut`**: 前者会 `reconcile` tokens / colors、`sync_base`
/// 投影到 gpui-base 层、再 `refresh_windows`; 直接改全局会跳过这些, base 层组件读到的仍是旧投影。
/// `size` 钳到 `[FONT_SIZE_MIN, FONT_SIZE_MAX]`, 防止配置被手改越界后 UI 失控。
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
/// **关键: 双槽都装**, 不能只 `apply_config` 单槽 —— 否则 `Theme::change` 读的是槽引用, 没装就 fallback
/// 默认主题, 且残留另一槽引用。Static 两槽塞同一个 `cfg`; Dynamic 分别装 `dyn_light` / `dyn_dark`
/// (找不到 / mode 不符 → registry 默认), 再按 `dyn_mode` 跟 OS 或调 `Theme::change`。
///
/// **写路径必须走 `Theme::update` / `Theme::change` 事务写, 不能用 `global_mut`**: 事务写才会 `reconcile`
/// token 与 color、按闭包**前**的字体家族决定是否 `resolve_default_font`、`sync_base` 投影到 gpui-base
/// 并 `refresh_windows`; `global_mut` 全跳过 (投影留旧值, 主题字体永不 resolve)。`window` 启动
/// `on_load` 拿不到 → `None` (`cx.window_appearance()` 兜底), 设置页传 `Some(window)`。
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
                // 找不到 → 用 registry 默认主题。
                if Theme::global(cx).mode.is_dark() {
                    registry.default_dark_theme().clone()
                } else {
                    registry.default_light_theme().clone()
                }
            });

            // 双槽同塞: Static 不区分明暗, `apply_config` 会把 mode 切成 `cfg.mode` 并装载
            // colors / tokens / 字体 / 圆角。
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
    // 不需要额外 `cx.refresh_windows()` 兜底: 上面每条路径都以事务写 API 收尾, 内部都会
    // `sync_base` + `refresh_windows`。
}

/// 列出当前可用的所有主题变体名 (按 name 字典序)。`HashMap` 迭代顺序不稳定, 必须显式排序才能给
/// Select 稳定选项顺序。
pub fn list_theme_names(cx: &App) -> Vec<SharedString> {
    let mut names: Vec<SharedString> = ThemeRegistry::global(cx).themes().keys().cloned().collect();
    names.sort_by_key(|a| a.to_lowercase());
    names
}

/// 列出指定模式 (light / dark) 的主题变体名 (按 name 字典序)。
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
