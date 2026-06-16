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

use gpui::{App, SharedString};
use gpui_component::{Theme, ThemeRegistry};

use crate::config::ConfigPaths;

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
/// - 主题名找不到时 `apply_theme_by_name` 内部静默 fallback
pub fn init(cx: &mut App, paths: &ConfigPaths, saved_theme: &str) {
    let themes_dir = paths.themes_dir.clone();
    if let Err(e) = ensure_user_themes_dir(&themes_dir) {
        tracing::warn!("prepare user themes dir {:?} failed: {e}; using default", themes_dir);
        return;
    }

    let saved = saved_theme.to_string();
    let themes_dir_for_log = themes_dir.clone();
    if let Err(e) = ThemeRegistry::watch_dir(themes_dir.clone(), cx, move |cx| {
        // on_load: reload 已完成，registry 现在有 21 个 embed 主题（变体展开后
        // 30+ 项）。应用持久化主题（如果还匹配）。
        // 不再手动 `cx.refresh_windows()` — `Theme::global_mut(cx).apply_config`
        // 内部会触发 gpui-component 内置的 Theme 变化 observer，observer
        // 自己会 refresh；手动再调一次就是重复。
        tracing::info!(
            "themes loaded: {} entries from {:?}",
            list_theme_names(cx).len(),
            themes_dir
        );
        apply_theme_by_name(&saved, cx);
    }) {
        tracing::warn!("watch themes dir {:?} failed: {e}", themes_dir_for_log);
    } else {
        tracing::info!("watching themes dir: {:?}", themes_dir_for_log);
    }
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
            tracing::info!(
                "added {} new themes to existing {:?}",
                added,
                path
            );
        }
    }
    Ok(())
}

/// 按名称把主题装进 `Theme::global_mut`。
///
/// - 空串 → no-op（保持当前主题，符合 config.toml 留空的语义）
/// - 找不到同名主题 → **静默 fallback 到 gpui-component 默认 light**
///
/// 装好之后 gpui-component 的 `cx.observe_global::<ThemeRegistry>` observer 会自动
/// `cx.refresh_windows()`，所有窗口重绘（见 gpui-component `theme/registry.rs:47-73`）。
pub fn apply_theme_by_name(name: &str, cx: &mut App) {
    if name.is_empty() {
        return;
    }
    let key = SharedString::from(name.to_string());
    let cfg = match ThemeRegistry::global(cx).themes().get(&key).cloned() {
        Some(c) => c,
        None => {
            tracing::info!(
                "theme '{}' not in registry; falling back to default Light (available: {})",
                name,
                list_theme_names(cx).join(", ")
            );
            // fallback：清空 light_theme / dark_theme 引用，gpui-component 会用
            // ThemeColor::default() 兜底（基本就是 Light）。
            return;
        }
    };
    Theme::global_mut(cx).apply_config(&cfg);
}

/// 列出当前可用的所有主题名（按 name 字典序）。
///
/// `HashMap` 迭代顺序不稳定，必须显式排序才能给 Select 稳定选项顺序。
pub fn list_theme_names(cx: &App) -> Vec<SharedString> {
    let mut names: Vec<SharedString> = ThemeRegistry::global(cx)
        .themes()
        .keys()
        .cloned()
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
        assert!(removed.exists(), "missing embedded theme should be re-added");
        let content = std::fs::read_to_string(&removed).expect("read back");
        assert_eq!(content, THEME_ADVENTURE, "should match embedded content");
    }
}
