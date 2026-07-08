//! 21 个主题 JSON 编译期嵌入 + 列表助手。
//!
//! `include_str!` 必须在静态上下文使用字面量路径, 所以每个主题一个 const。
//! 主题内容走 [`embedded_themes`] 返回 `(文件名, JSON 字符串)` 列表, 由
//! [`super::user_dir::ensure_user_themes_dir`] 同步到 `~/.sonovel/themes/`。

/// 字号范围 (px)。gpui-component 默认 16; 设置页 slider 的 min/max 复用这两个常量。
pub const FONT_SIZE_MIN: f32 = 12.0;
pub const FONT_SIZE_MAX: f32 = 24.0;
pub const FONT_SIZE_DEFAULT: f32 = 16.0;

// ----- 21 个主题 JSON embed (编译期嵌入; `include_str!` 路径必须字面量) -----

pub const THEME_ADVENTURE: &str = include_str!("../themes/adventure.json");
pub const THEME_ALDUIN: &str = include_str!("../themes/alduin.json");
pub const THEME_ASCIINEMA: &str = include_str!("../themes/asciinema.json");
pub const THEME_AYU: &str = include_str!("../themes/ayu.json");
pub const THEME_CATPPUCCIN: &str = include_str!("../themes/catppuccin.json");
pub const THEME_EVERFOREST: &str = include_str!("../themes/everforest.json");
pub const THEME_FAHRENHEIT: &str = include_str!("../themes/fahrenheit.json");
pub const THEME_FLEXOKI: &str = include_str!("../themes/flexoki.json");
pub const THEME_GRUVBOX: &str = include_str!("../themes/gruvbox.json");
pub const THEME_HARPER: &str = include_str!("../themes/harper.json");
pub const THEME_HYBRID: &str = include_str!("../themes/hybrid.json");
pub const THEME_JELLYBEANS: &str = include_str!("../themes/jellybeans.json");
pub const THEME_KIBBLE: &str = include_str!("../themes/kibble.json");
pub const THEME_MACOS_CLASSIC: &str = include_str!("../themes/macos-classic.json");
pub const THEME_MATRIX: &str = include_str!("../themes/matrix.json");
pub const THEME_MELLIFLUOUS: &str = include_str!("../themes/mellifluous.json");
pub const THEME_MOLOKAI: &str = include_str!("../themes/molokai.json");
pub const THEME_SOLARIZED: &str = include_str!("../themes/solarized.json");
pub const THEME_SPACEDUCK: &str = include_str!("../themes/spaceduck.json");
pub const THEME_TOKYONIGHT: &str = include_str!("../themes/tokyonight.json");
pub const THEME_TWILIGHT: &str = include_str!("../themes/twilight.json");

/// `(file_name, json_content)` 列表。init 时按需写到用户 themes 目录。
pub(super) fn embedded_themes() -> Vec<(&'static str, &'static str)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 21 个 embed JSON 全部合法、文件名 *.json、含 `themes[]` + `name`。
    /// 这条测试是「编译期 embed 没断 + 内容 JSON 没手贱改坏」的双重保险。
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
}