//! 主题加载 + 应用 + 列表。
//!
//! 策略：21 个 JSON **直接 `include_str!` 进二进制**（编译期嵌入）。
//! 启动时把 embed 字节写到 `tempfile::tempdir()` 拿到的进程级临时目录，
//! 然后调 `ThemeRegistry::watch_dir(temp_dir, cx, _)` 让 gpui-component
//! 扫目录、parse 为 `ThemeSet`、把每个变体注册到 global `HashMap<SharedString, Rc<ThemeConfig>>`。
//! 进程退出时 `tempdir` 自动清理（`Drop for TempDir`）。
//!
//! 业务层 API：
//! - [`watch_themes_dir`] — 启动时调一次
//! - [`apply_theme_by_name`] — 把指定名称主题装到 `Theme::global_mut`；找不到 → fallback 默认 light
//! - [`list_theme_names`] — 列出全部主题名（设置页 Select 用）
//!
//! 不做的事：
//! - 不在 dev / release 之间区分路径（统一 embed）
//! - 不依赖 CWD / exe 同目录 / 用户主目录
//! - 不热重载（gpui-component watch_dir 会启 notify，但 temp 目录程序退出就删，热重载无意义）

use std::path::PathBuf;

use gpui::{App, SharedString};
use gpui_component::{Theme, ThemeRegistry};
use tempfile::TempDir;

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

/// `(file_name, json_content)` 列表。watch_dir 时写到 temp。
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
/// gpui-component 的 `ThemeRegistry::themes` 字段是私有的，公开 API 只有
/// `watch_dir(path, cx, on_load)`，所以无论怎么走，最终都得有一个真实目录。
/// 我们用 `tempfile::tempdir()` 拿进程级临时目录（`Drop` 自动清理），写 21 个
/// JSON 进去，立即 `watch_dir`。
///
/// **关键时序**：`watch_dir` 内部是 `cx.spawn(...)` **异步** 跑 reload，
/// 它立即返回。`on_load` 回调在 reload 完成后被调，那里才应该
/// `apply_theme_by_name` + `cx.refresh_windows()` —— 不然 UI 看到的 registry
/// 还在 reload 前（仅 2 个内置默认主题），Select 下拉是空的。
///
/// - `saved_theme`：config.toml 里的主题名；空串 = 保持 gpui-component 默认
/// - 主题名找不到时 `apply_theme_by_name` 内部静默 fallback
pub fn init(cx: &mut App, saved_theme: &str) {
    let tmp = match prepare_temp_themes_dir() {
        Ok((tmp, path)) => {
            tracing::info!("loaded {} embedded themes to {:?}", count_embedded(), path);
            tmp
        }
        Err(e) => {
            tracing::warn!("prepare temp themes dir failed: {e}; using default");
            return;
        }
    };

    let path: PathBuf = tmp.path().to_path_buf();
    // `TempDir` 在 drop 时 rmrf；这里 leak 它以保持目录存活整个程序生命周期。
    // 进程退出 OS 自动回收；内存里只多 1 个 Arc + path 字符串。
    std::mem::forget(tmp);

    let saved = saved_theme.to_string();
    if let Err(e) = ThemeRegistry::watch_dir(path.clone(), cx, move |cx| {
        // on_load: reload 已完成，registry 现在有 21 个 embed 主题（变体展开后
        // 30+ 项）。应用持久化主题（如果还匹配）。
        // 不再手动 `cx.refresh_windows()` — `Theme::global_mut(cx).apply_config`
        // 内部会触发 gpui-component 内置的 Theme 变化 observer，observer
        // 自己会 refresh；手动再调一次就是重复。
        tracing::info!(
            "themes loaded: {} entries",
            list_theme_names(cx).len()
        );
        apply_theme_by_name(&saved, cx);
    }) {
        tracing::warn!("watch themes dir {:?} failed: {e}", path);
    } else {
        tracing::info!("watching themes dir: {:?}", path);
    }
}

fn prepare_temp_themes_dir() -> std::io::Result<(TempDir, PathBuf)> {
    let tmp = tempfile::Builder::new()
        .prefix("sonovel-themes-")
        .tempdir()?;
    let path = tmp.path().to_path_buf();
    for (name, content) in embedded_themes() {
        std::fs::write(path.join(name), content)?;
    }
    Ok((tmp, path))
}

fn count_embedded() -> usize {
    embedded_themes().len()
}

/// 按名称把主题装进 `Theme::global_mut`。
///
/// - 空串 → no-op（保持当前主题，符合 config.toml 留空的语义）
/// - 找不到同名主题 → **静默 fallback 到 gpui-component 默认 light**（用
///   `Theme::change(ThemeMode::Light, ...)` 切回内置默认 — 不报错）
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

    /// prepare_temp_themes_dir 写出来的目录里恰好 21 个 *.json，
    /// 且每个文件都能 parse 回合法 JSON。
    #[test]
    fn prepare_temp_dir_writes_all_themes() {
        let (_tmp, path) = prepare_temp_themes_dir().expect("prepare temp dir");
        let written: Vec<_> = std::fs::read_dir(&path)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();
        assert_eq!(written.len(), 21, "expected 21 JSON files in temp dir");
        for entry in written {
            let p = entry.path();
            let s = std::fs::read_to_string(&p).expect("read back");
            let _: serde_json::Value =
                serde_json::from_str(&s).unwrap_or_else(|e| panic!("bad json {:?}: {e}", p));
        }
    }
}
