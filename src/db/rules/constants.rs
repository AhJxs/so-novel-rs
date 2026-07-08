//! 规则模块常量 (PR #17 拆分, 2026-07-08).
//!
//! 包含 `META_*` 模板查询 (给 `apply_default_rule` 填默认值用) + `BUNDLED_RULES`
//! 编译期嵌入的规则文件列表 (给 `init_rules_dir` 首次启动落盘用)。

/// meta 默认查询 (与 Java `util.SourceUtils` 常量一致)。
///
/// `apply_default_rule` 在 `book` 字段缺失时回落到这些查询, 让规则可以
/// 依赖浏览器解析 `<meta>` 标签的能力 (很多站点在 head 里塞 og:* 元信息)。
pub const META_BOOK_NAME: &str = r#"meta[property="og:novel:book_name"]"#;
pub const META_AUTHOR: &str = r#"meta[property="og:novel:author"]"#;
pub const META_INTRO: &str = r#"meta[name="description"]"#;
pub const META_CATEGORY: &str = r#"meta[property="og:novel:category"]"#;
pub const META_COVER_URL: &str = r#"meta[property="og:image"]"#;
pub const META_LATEST_CHAPTER: &str = r#"meta[property="og:novel:latest_chapter_name"]"#;
pub const META_LATEST_CHAPTER_URL: &str = r#"meta[property="og:novel:latest_chapter_url"]"#;
pub const META_LAST_UPDATE_TIME: &str = r#"meta[property="og:novel:update_time"]"#;
pub const META_STATUS: &str = r#"meta[property="og:novel:status"]"#;

/// 编译期嵌入的规则文件列表。`init_rules_dir` 首次启动时把这里的内容
/// 写到 `~/.sonovel/rules/`, 已存在的文件不覆盖 (尊重用户修改)。
pub(super) const BUNDLED_RULES: &[(&str, &str)] = &[
    ("main.json", include_str!("../../../bundle/rules/main.json")),
    (
        "cloudflare.json",
        include_str!("../../../bundle/rules/cloudflare.json"),
    ),
    (
        "no-search.json",
        include_str!("../../../bundle/rules/no-search.json"),
    ),
    (
        "rate-limit.json",
        include_str!("../../../bundle/rules/rate-limit.json"),
    ),
    (
        "proxy-required.json",
        include_str!("../../../bundle/rules/proxy-required.json"),
    ),
];
