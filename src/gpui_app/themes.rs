//! 主题加载 + 应用 + 列表。
//!
//! 策略：21 个 JSON **直接 `include_str!` 进二进制**（编译期嵌入）。
//! 启动时把 embed 字节同步到用户主目录下的 `~/.sonovel/themes/`，然后调
//! `ThemeRegistry::watch_dir(themes_dir, cx, _)` 让 gpui-component 扫目录、
//! parse 为 `ThemeSet`、把每个变体注册到 global `HashMap<SharedString, Rc<ThemeConfig>>`。
//!
//! `themes_dir` 同步规则（见 [`ensure_user_themes_dir`]）：
//! - 目录不存在 → 创建 + 写入全部 21 个 embed 主题
//! - 目录存在 → 只补缺失的 embed 文件（app 升级加新主题时自动加进来），
//!   **不覆盖**已有文件 —— 用户可能改过
//! - 用户也可手动放自定义 *.json 进去，gpui-component 的 file watcher 会自动 reload
//!
//! 业务层 API：
//! - [`init`] — 启动时调一次
//! - [`apply_theme_by_name`] — 把指定名称主题装到 `Theme::global_mut`；找不到 → fallback 默认 light
//! - [`list_theme_names`] — 列出全部主题名（设置页 Select 用）
//!
//! 不做的事：
//! - 不在 dev / release 之间区分路径（统一 embed）
//! - 不依赖 CWD / exe 同目录
//! - 不删用户已有的 themes（即使看着像 embed 默认）

use std::path::Path;
use std::rc::Rc;

use gpui::{App, SharedString, Window, px};
use gpui_component::{Theme, ThemeConfig, ThemeMode, ThemeRegistry};

use crate::config::{ConfigPaths, ThemeDynMode, ThemeKind, ThemePref};

/// 字号范围（px）。gpui-component 默认 16；设置页 slider 的 min/max 复用这两个常量。
pub const FONT_SIZE_MIN: f32 = 12.0;
pub const FONT_SIZE_MAX: f32 = 24.0;
pub const FONT_SIZE_DEFAULT: f32 = 16.0;

// ----- 21 个主题 JSON embed（编译期嵌入；`include_str!` 路径必须字面量）-----
pub const THEME_ADVENTURE: &str = include_str!("themes/adventure.json");
pub const THEME_ALDUIN: &str = include_str!("themes/alduin.json");
pub const THEME_ASCIINEMA: &str = include_str!("themes/asciinema.json");
pub const THEME_AYU: &str = include_str!("themes/ayu.json");
pub const THEME_CATPPUCCIN: &str = include_str!("themes/catppuccin.json");
pub const THEME_EVERFOREST: &str = include_str!("themes/everforest.json");
pub const THEME_FAHRENHEIT: &str = include_str!("themes/fahrenheit.json");
pub const THEME_FLEXOKI: &str = include_str!("themes/flexoki.json");
pub const THEME_GRUVBOX: &str = include_str!("themes/gruvbox.json");
pub const THEME_HARPER: &str = include_str!("themes/harper.json");
pub const THEME_HYBRID: &str = include_str!("themes/hybrid.json");
pub const THEME_JELLYBEANS: &str = include_str!("themes/jellybeans.json");
pub const THEME_KIBBLE: &str = include_str!("themes/kibble.json");
pub const THEME_MACOS_CLASSIC: &str = include_str!("themes/macos-classic.json");
pub const THEME_MATRIX: &str = include_str!("themes/matrix.json");
pub const THEME_MELLIFLUOUS: &str = include_str!("themes/mellifluous.json");
pub const THEME_MOLOKAI: &str = include_str!("themes/molokai.json");
pub const THEME_SOLARIZED: &str = include_str!("themes/solarized.json");
pub const THEME_SPACEDUCK: &str = include_str!("themes/spaceduck.json");
pub const THEME_TOKYONIGHT: &str = include_str!("themes/tokyonight.json");
pub const THEME_TWILIGHT: &str = include_str!("themes/twilight.json");

/// `(file_name, json_content)` 列表。init 时按需写到用户 themes 目录。
fn embedded_themes() -> Vec<(&'static str, &'static str)> {
    vec![
        ("adventure.json", THEME_ADVENTURE),
        ("alduin.json", THEME_ALDUIN),
        ("asciinema.json", THEME_ASCIINEMA),
        ("ayu.json", THEME_AYU),
        ("catppuccin.json", THEME_CATPPUCCIN),
        ("everforest.json", THEME_EVERFOREST),
        ("fahrenheit.json", THEME_FAHRENHEIT),
        ("flexoki.json", THEME_FLEXOKI),
        ("gruvbox.json", THEME_GRUVBOX),
        ("harper.json", THEME_HARPER),
        ("hybrid.json", THEME_HYBRID),
        ("jellybeans.json", THEME_JELLYBEANS),
        ("kibble.json", THEME_KIBBLE),
        ("macos-classic.json", THEME_MACOS_CLASSIC),
        ("matrix.json", THEME_MATRIX),
        ("mellifluous.json", THEME_MELLIFLUOUS),
        ("molokai.json", THEME_MOLOKAI),
        ("solarized.json", THEME_SOLARIZED),
        ("spaceduck.json", THEME_SPACEDUCK),
        ("tokyonight.json", THEME_TOKYONIGHT),
        ("twilight.json", THEME_TWILIGHT),
    ]
}

/// 启动时调用一次：把 embed JSON 喂给 `ThemeRegistry::watch_dir`，
/// reload 完成后用 `saved_theme` 名字应用主题 + refresh 所有窗口。
///
/// 主题目录走 `paths.themes_dir`（`~/.sonovel/themes/`，由 [`ensure_user_themes_dir`]
/// 同步），`gpui-component::ThemeRegistry::themes` 字段私有，公开 API 只有
/// `watch_dir(path, cx, on_load)`，所以还是需要一个真实目录 —— 这次用持久用户目录，
/// 不用 `tempfile::tempdir()` + `mem::forget` 泄漏。
///
/// **关键时序**：`watch_dir` 内部 `cx.spawn(...)` **异步** 跑 reload，立即返回。
/// `on_load` 回调在 reload 完成后被调，那时 registry 才包含全部主题，
/// `apply_theme_by_name` 在那里调用才对。
///
/// - `saved_theme`：config.toml 里的主题名；空串 = 保持 gpui-component 默认
/// - `font_size`：config.toml 里的字号（px）；在 `apply_theme_by_name` **之后**应用，
///   因为 `Theme::apply_config` 会用主题 JSON 的 `font_size`（缺省 16）覆盖
///   `Theme.font_size`，先调字号后装主题会被冲掉。
/// - 主题名找不到时 `apply_theme_by_name` 内部静默 fallback
pub fn init(cx: &mut App, paths: &ConfigPaths, theme_pref: &ThemePref, font_size: f32) {
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
        // on_load: reload 已完成，registry 现在有 21 个 embed 主题（变体展开后
        // 30+ 项）。应用持久化主题偏好。
        tracing::info!(
            "themes loaded: {} entries from {:?}",
            list_theme_names(cx).len(),
            themes_dir
        );
        apply_theme_pref(&pref, None, cx);
        // 必须在装主题之后：apply_config 会把字号重置成主题默认值，这里再覆回用户值。
        apply_font_size(font_size, cx);
    }) {
        tracing::warn!("watch themes dir {:?} failed: {e}", themes_dir_for_log);
    } else {
        tracing::info!("watching themes dir: {:?}", themes_dir_for_log);
    }
}

/// 把字号写进全局 `Theme.font_size` 并刷新所有窗口。
///
/// `Root::render` 每帧用 `window.set_rem_size(cx.theme().font_size)` 把主题字号设成
/// rem 基准，组件全用 `rems(...)` 缩放，所以改这一个字段 = 全局等比缩放。
///
/// `Theme::global_mut` 返回的是 `&mut Theme` 原地改，**不会**触发 `observe_global::<Theme>`
/// observer，所以必须显式 `cx.refresh_windows()` 让 `Root::render` 重跑拿到新 rem_size。
///
/// `size` 会被钳到 `[FONT_SIZE_MIN, FONT_SIZE_MAX]`，防止配置被手改成越界值后 UI 失控。
pub fn apply_font_size(size: f32, cx: &mut App) {
    let size = size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
    Theme::global_mut(cx).font_size = px(size);
    cx.refresh_windows();
}

/// 把 embed 主题同步到用户 themes 目录：
/// - 不存在 → 创建 + 写全部 21 个
/// - 已存在 → 只补缺失的（app 升级新增主题时自动加进来）
/// - **不覆盖任何已存在文件** —— 用户可能改过、或全是自定义主题
fn ensure_user_themes_dir(path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
        for (name, content) in embedded_themes() {
            std::fs::write(path.join(name), content)?;
        }
        tracing::info!(
            "created themes dir at {:?} with {} embedded themes",
            path,
            embedded_themes().len()
        );
    } else {
        let mut added = 0usize;
        for (name, content) in embedded_themes() {
            let target = path.join(name);
            if !target.exists() {
                std::fs::write(&target, content)?;
                added += 1;
            }
        }
        if added > 0 {
            tracing::info!("added {} new themes to existing {:?}", added, path);
        }
    }
    Ok(())
}

/// 解析主题名 → `ThemeConfig`。空串 / 找不到时返回 `None`。
fn lookup_theme(name: &str, cx: &App) -> Option<Rc<ThemeConfig>> {
    if name.is_empty() {
        return None;
    }
    let key = SharedString::from(name.to_string());
    ThemeRegistry::global(cx).themes().get(&key).cloned()
}

/// 应用主题偏好到全局 `Theme`，并刷新所有窗口。
///
/// 两种模式（见 [`ThemePref`]）：
/// - **Static**：`static_name` 同时塞进浅/深两槽 → `apply_config`，整 app 不随系统明暗切换。
/// - **Dynamic**：`dyn_light` / `dyn_dark` 分别装进两槽（找不到/空 → registry 默认），
///   再按 `dyn_mode` 调 `Theme::change` 选激活槽；`system` 走 `sync_system_appearance` 跟 OS。
///
/// **关键：双槽都装 + Theme::change**，不能只 apply_config 单槽 —— 否则 `Theme::change`
/// 读的是槽引用，没装就 fallback 默认主题，且残留另一槽引用。
///
/// `window`：启动 on_load 拿不到 → 传 `None`（`cx.window_appearance()` 兜底）；
/// 设置页实时改时传 `Some(window)` 拿到精确 appearance。
///
/// 找不到的主题名静默 fallback 到 registry 默认主题，不 panic。
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

            // 双槽同塞：Static 不区分明暗 —— 切到 Static 后不会被残留槽影响（之前 Dynamic
            // 选过的另一 mode 主题残留不会再回来）。
            let theme = Theme::global_mut(cx);
            theme.light_theme = cfg.clone();
            theme.dark_theme = cfg.clone();
            theme.apply_config(&cfg);
            // apply_config 已把 mode 设成主题自身 mode；显式同步一次保证 Theme.mode 一致。
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
                        && lookup_theme(&pref.dyn_light, cx)
                            .map(|c| c.mode.is_dark())
                            .unwrap_or(false)
                    {
                        // 用户给浅槽选了个深色主题 → 过滤掉，回落默认浅色（设置页 UI 也会
                        // 过滤，这里是防御性兜底）。
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
                        && lookup_theme(&pref.dyn_dark, cx)
                            .map(|c| !c.mode.is_dark())
                            .unwrap_or(false)
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

    // Theme::change / apply_config 都触发窗口刷新，但 global_mut 改槽引用不触发 observer，
    // 显式 refresh 兜底。
    cx.refresh_windows();
}

/// 列出当前可用的所有主题变体名（按 name 字典序）。
///
/// `HashMap` 迭代顺序不稳定，必须显式排序才能给 Select 稳定选项顺序。
pub fn list_theme_names(cx: &App) -> Vec<SharedString> {
    let mut names: Vec<SharedString> = ThemeRegistry::global(cx).themes().keys().cloned().collect();
    names.sort_by_key(|a| a.to_lowercase());
    names
}

/// 列出指定模式（light / dark）的主题变体名（按 name 字典序）。
///
/// 动态模式选浅/深主题时用：过滤掉与目标 mode 不符的变体，避免用户把深色主题选进浅色槽。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 21 个 embed JSON 全部合法、文件名 *.json、含 `themes[]` + `name`。
    #[test]
    fn embedded_themes_complete_and_nonempty() {
        let themes = embedded_themes();
        assert_eq!(themes.len(), 21, "expect 21 embedded theme files");
        for (name, content) in &themes {
            assert!(name.ends_with(".json"), "filename must end .json: {name}");
            assert!(!content.is_empty(), "empty content: {name}");
            let v: serde_json::Value = serde_json::from_str(content)
                .unwrap_or_else(|e| panic!("invalid JSON in {name}: {e}"));
            let arr = v
                .get("themes")
                .and_then(|t| t.as_array())
                .unwrap_or_else(|| panic!("{name}: missing 'themes' array"));
            assert!(!arr.is_empty(), "{name}: 'themes' array empty");
            for (i, item) in arr.iter().enumerate() {
                let n = item
                    .get("name")
                    .and_then(|s| s.as_str())
                    .unwrap_or_else(|| panic!("{name}#{i}: missing 'name'"));
                assert!(!n.is_empty(), "{name}#{i}: empty name");
            }
        }
    }

    /// 首次调用 → 创建目录 + 写入 21 个 embed 主题。
    #[test]
    fn ensure_user_themes_dir_creates_when_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("themes");
        assert!(!path.exists(), "precondition: dir should not exist");

        ensure_user_themes_dir(&path).expect("first call");

        assert!(path.is_dir(), "should create dir");
        let count = std::fs::read_dir(&path)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .count();
        assert_eq!(count, 21, "should write all 21 embedded themes");

        // 每个文件都能 parse 回合法 JSON。
        for entry in std::fs::read_dir(&path).unwrap().flatten() {
            let p = entry.path();
            let s = std::fs::read_to_string(&p).expect("read back");
            let _: serde_json::Value =
                serde_json::from_str(&s).unwrap_or_else(|e| panic!("bad json {:?}: {e}", p));
        }
    }

    /// 后续调用 → 已存在文件**不覆盖**（保留用户修改）。
    #[test]
    fn ensure_user_themes_dir_preserves_user_modifications() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("themes");
        ensure_user_themes_dir(&path).expect("first call");

        // 用户改了 adventure.json
        let modified = path.join("adventure.json");
        let custom_payload = r#"{"themes":[{"name":"my-custom","mode":"light"}]}"#;
        std::fs::write(&modified, custom_payload).expect("user modification");

        // 第二次调用不应覆盖
        ensure_user_themes_dir(&path).expect("second call");
        let content = std::fs::read_to_string(&modified).expect("read back");
        assert_eq!(
            content, custom_payload,
            "user-modified file should NOT be overwritten"
        );
    }

    /// 后续调用 → 缺失文件被补齐（模拟 app 升级新增主题）。
    #[test]
    fn ensure_user_themes_dir_adds_missing_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("themes");
        ensure_user_themes_dir(&path).expect("first call");

        // 用户删了一个 embed 主题
        let removed = path.join("adventure.json");
        std::fs::remove_file(&removed).expect("delete");

        // 第二次调用应补回来
        ensure_user_themes_dir(&path).expect("second call");
        assert!(
            removed.exists(),
            "missing embedded theme should be re-added"
        );
        let content = std::fs::read_to_string(&removed).expect("read back");
        assert_eq!(content, THEME_ADVENTURE, "should match embedded content");
    }
}
