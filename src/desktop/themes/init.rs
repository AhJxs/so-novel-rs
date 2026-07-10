//! 主题启动入口: `init()` 把 embed 喂给 `ThemeRegistry` 并在 `on_load` 应用偏好。
//!
//! `apply_theme_pref` / `apply_font_size` 在 [`super::apply`], 目录同步在
//! [`super::user_dir`], embed consts 在 [`super::embedded`]。

use gpui::App;
use gpui_component::ThemeRegistry;

use crate::config::{ConfigPaths, ThemePref};

use super::apply::{apply_font_size, apply_theme_pref, list_theme_names};
use super::user_dir::ensure_user_themes_dir;

/// 启动时调用一次: 把 embed JSON 喂给 `ThemeRegistry::watch_dir`,
/// reload 完成后用 `saved_theme` 名字应用主题 + refresh 所有窗口。
///
/// 主题目录走 `paths.themes_dir` (`~/.sonovel/themes/`, 由 [`ensure_user_themes_dir`]
/// 同步), `gpui-component::ThemeRegistry::themes` 字段私有, 公开 API 只有
/// `watch_dir(path, cx, on_load)`, 所以还是需要一个真实目录 —— 这次用持久用户目录,
/// 不用 `tempfile::tempdir()` + `mem::forget` 泄漏.
///
/// **关键时序**: `watch_dir` 内部 `cx.spawn(...)` **异步** 跑 reload, 立即返回。
/// `on_load` 回调在 reload 完成后被调, 那时 registry 才包含全部主题,
/// `apply_theme_pref` 在那里调用才对。
///
/// - `saved_theme`: config.toml 里的主题名; 空串 = 保持 gpui-component 默认
/// - `font_size`: config.toml 里的字号 (px); 在 `apply_theme_pref` **之后**应用,
///   因为 `Theme::apply_config` 会用主题 JSON 的 `font_size` (缺省 16) 覆盖
///   `Theme.font_size`, 先调字号后装主题会被冲掉。
/// - 主题名找不到时 `apply_theme_pref` 内部静默 fallback
#[tracing::instrument(
    name = "themes::init",
    skip_all,
    fields(themes_dir = ?paths.themes_dir, font_size)
)]
pub fn init(cx: &mut App, paths: &ConfigPaths, theme_pref: &ThemePref, font_size: f32) {
    tracing::Span::current().record("font_size", font_size);
    let themes_dir = paths.themes_dir.clone();
    if let Err(e) = ensure_user_themes_dir(&themes_dir) {
        tracing::warn!(
            "prepare user themes dir {:?} failed: {e}; using default",
            themes_dir
        );
        return;
    }

    let pref = theme_pref.clone();
    let themes_dir_for_log = themes_dir.clone();
    if let Err(e) = ThemeRegistry::watch_dir(themes_dir.clone(), cx, move |cx| {
        // on_load: reload 已完成, registry 现在有 21 个 embed 主题 (变体展开后
        // 30+ 项)。应用持久化主题偏好。
        tracing::info!(
            "themes loaded: {} entries from {:?}",
            list_theme_names(cx).len(),
            themes_dir
        );
        apply_theme_pref(&pref, None, cx);
        // 必须在装主题之后: apply_config 会把字号重置成主题默认值, 这里再覆回用户值。
        apply_font_size(font_size, cx);
    }) {
        tracing::warn!("watch themes dir {:?} failed: {e}", themes_dir_for_log);
    } else {
        tracing::info!("watching themes dir: {:?}", themes_dir_for_log);
    }
}
