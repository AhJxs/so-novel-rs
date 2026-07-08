//! 主题应用 + 主题枚举。
//!
//! [`apply_theme_pref`] 是核心入口: 把 [`ThemePref`] 装到全局 `Theme`。
//! [`apply_font_size`] 在装主题之后单独调, 覆盖 `apply_config` 重置的字号。
//! [`list_theme_names`] / [`list_theme_names_by_mode`] 供设置页 Select 用。

use std::rc::Rc;

use gpui::{App, SharedString, Window, px};
use gpui_component::{Theme, ThemeConfig, ThemeMode, ThemeRegistry};

use crate::config::{ThemeDynMode, ThemeKind, ThemePref};

use super::embedded::{FONT_SIZE_MAX, FONT_SIZE_MIN};

/// 把字号写进全局 `Theme.font_size` 并刷新所有窗口。
///
/// `Root::render` 每帧用 `window.set_rem_size(cx.theme().font_size)` 把主题字号设成
/// rem 基准, 组件全用 `rems(...)` 缩放, 所以改这一个字段 = 全局等比缩放。
///
/// `Theme::global_mut` 返回的是 `&mut Theme` 原地改, **不会**触发
/// `observe_global::<Theme>` observer, 所以必须显式 `cx.refresh_windows()` 让
/// `Root::render` 重跑拿到新 `rem_size`.
///
/// `size` 会被钳到 `[FONT_SIZE_MIN, FONT_SIZE_MAX]`, 防止配置被手改成越界值后 UI 失控。
#[tracing::instrument(name = "themes::apply_font_size", skip_all, fields(size))]
pub fn apply_font_size(size: f32, cx: &mut App) {
    let size = size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
    tracing::Span::current().record("size", size);
    Theme::global_mut(cx).font_size = px(size);
    cx.refresh_windows();
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
/// **关键: 双槽都装 + `Theme::change`**, 不能只 `apply_config` 单槽 —— 否则 `Theme::change`
/// 读的是槽引用, 没装就 fallback 默认主题, 且残留另一槽引用。
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
            // 选过的另一 mode 主题残留不会再回来)。
            let theme = Theme::global_mut(cx);
            theme.light_theme = cfg.clone();
            theme.dark_theme = cfg.clone();
            theme.apply_config(&cfg);
            // apply_config 已把 mode 设成主题自身 mode; 显式同步一次保证 Theme.mode 一致。
            Theme::change(cfg.mode, None, cx);
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

            // 双槽装好 → Theme::change 内部 apply_config 对应槽。
            {
                let theme = Theme::global_mut(cx);
                theme.light_theme = light_cfg;
                theme.dark_theme = dark_cfg;
            }

            match pref.dyn_mode {
                ThemeDynMode::System => Theme::sync_system_appearance(window, cx),
                ThemeDynMode::Light => Theme::change(ThemeMode::Light, None, cx),
                ThemeDynMode::Dark => Theme::change(ThemeMode::Dark, None, cx),
            }
        }
    }

    // Theme::change / apply_config 都触发窗口刷新, 但 global_mut 改槽引用不触发 observer,
    // 显式 refresh 兜底。
    cx.refresh_windows();
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
