//! 书源规则：目录初始化 + 文件解析 + 默认值填充 + 活跃规则加载。
//!
//! 一切围绕 `Rule`（定义在 `crate::models::rule`）这个结构：
//!
//! - **常量** (`META_*` / `BUNDLED_RULES`) — 模板字符串和编译期嵌入的规则文件。
//! - **解析** (`load_rules_from_path` + 私有 `walk_rule_files` / `parse_one_file`) —
//!   从 `.json` / `.json5` 文件或目录读出 `Vec<Rule>`，分配自增 ID。
//! - **默认值** (`apply_default_rule`) — 给空字段回填 `baseUri` / `timeout` /
//!   `book.*` 的 meta 后备查询（与 Java 端 `util.SourceUtils#applyDefaultRule` 等价）。
//! - **目录初始化** (`init_rules_dir` / `list_rule_files`) — 首次启动把编译期
//!   嵌入的规则文件落到 `~/.sonovel/rules/`，并按 `.json` / `.json5` 枚举现有文件。
//! - **活跃规则** (`load_active_rules`) — 主入口：读 `SourcesConfig::active_file`
//!   指定的文件 + 合并禁用状态；找不到则回退到整个目录。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::LangType;
use crate::models::Rule;
use crate::util::lang::detect_system_lang;

use super::SourcesConfig;

// =====================================================================
// 常量
// =====================================================================

/// meta 默认查询（与 Java `util.SourceUtils` 常量一致）。
///
/// `apply_default_rule` 在 `book` 字段缺失时回落到这些查询，让规则可以
/// 依赖浏览器解析 `<meta>` 标签的能力（很多站点在 head 里塞 og:* 元信息）。
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
/// 写到 `~/.sonovel/rules/`，已存在的文件不覆盖（尊重用户修改）。
const BUNDLED_RULES: &[(&str, &str)] = &[
    ("main.json", include_str!("../../bundle/rules/main.json")),
    (
        "cloudflare.json",
        include_str!("../../bundle/rules/cloudflare.json"),
    ),
    (
        "no-search.json",
        include_str!("../../bundle/rules/no-search.json"),
    ),
    (
        "rate-limit.json",
        include_str!("../../bundle/rules/rate-limit.json"),
    ),
    (
        "proxy-required.json",
        include_str!("../../bundle/rules/proxy-required.json"),
    ),
];

// =====================================================================
// 错误类型
// =====================================================================

#[derive(Debug, Error)]
pub enum RulesError {
    #[error("规则路径不存在: {0}")]
    NotFound(PathBuf),
    #[error("规则文件读取失败 {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("规则文件解析失败 {path}: {message}")]
    Parse { path: PathBuf, message: String },
}

// =====================================================================
// 文件解析：load_rules_from_path + 私有 walk / parse
// =====================================================================

/// 加载一个规则路径（文件或目录）。
///
/// - 若 `path` 是 `.json` / `.json5` 文件，直接当 `Vec<Rule>` 解析；
/// - 若 `path` 是目录，递归查找 `*.json` / `*.json5`（跳过 `rule-template.json5`
///   这类模板）；
/// - 加载完毕统一调 [`apply_default_rule`] 填默认值，并按顺序赋自增 ID。
pub fn load_rules_from_path(path: &Path) -> Result<Vec<Rule>, RulesError> {
    if !path.exists() {
        return Err(RulesError::NotFound(path.to_path_buf()));
    }

    let mut rules = Vec::new();
    if path.is_file() {
        rules.extend(parse_one_file(path)?);
    } else {
        // 目录：枚举所有规则文件
        let mut files: Vec<PathBuf> = walk_rule_files(path)?;
        files.sort();
        for f in files {
            rules.extend(parse_one_file(&f)?);
        }
    }

    // 填充默认值 + 分配 ID
    let lang = detect_system_lang();
    for (idx, rule) in rules.iter_mut().enumerate() {
        apply_default_rule(rule, lang);
        rule.id = (idx + 1) as i32;
    }

    Ok(rules)
}

/// 递归枚举目录下所有 `.json` / `.json5` 文件。
/// 跳过 `rule-template.json5` 这类模板（保留给用户当参考，不要当成书源加载）。
fn walk_rule_files(dir: &Path) -> Result<Vec<PathBuf>, RulesError> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| RulesError::Io {
        path: dir.to_path_buf(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| RulesError::Io {
            path: dir.to_path_buf(),
            source: e,
        })?;
        let p = entry.path();
        if p.is_dir() {
            out.extend(walk_rule_files(&p)?);
            continue;
        }
        let Some(ext) = p.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        let name_lower = p
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if name_lower.starts_with("rule-template") {
            continue;
        }
        match ext.to_ascii_lowercase().as_str() {
            "json" | "json5" => out.push(p),
            _ => {}
        }
    }
    Ok(out)
}

/// 解析单个规则文件：`.json5` 直接走 json5 解析；`.json` 先用严格
/// `serde_json`，失败再用 json5 兜底（带注释的 .json 也能加载）。
fn parse_one_file(path: &Path) -> Result<Vec<Rule>, RulesError> {
    let bytes = std::fs::read(path).map_err(|e| RulesError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let text = String::from_utf8_lossy(&bytes);
    let is_json5 = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("json5"))
        .unwrap_or(false);

    let rules: Vec<Rule> = if is_json5 {
        json5::from_str(&text).map_err(|e| RulesError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?
    } else {
        // 现有 main.json 等是严格 JSON，先用 serde_json，失败再用 json5 兜底
        match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_strict_err) => json5::from_str(&text).map_err(|e| RulesError::Parse {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?,
        }
    };
    Ok(rules)
}

// =====================================================================
// 默认值填充：apply_default_rule
// =====================================================================

/// 给一条 `Rule` 填默认值。等价于 Java `util.SourceUtils#applyDefaultRule`：
/// - `language` 空 → 用系统检测到的 locale；
/// - `search/book/toc/chapter.base_uri` 空 → 用 `rule.url`；
/// - 各 section 的 `timeout` 空 → 15s（toc 60s）；
/// - `book.*` 字段空 → 用 `META_*` 常量（让 scraper 走浏览器 meta 解析）。
pub fn apply_default_rule(rule: &mut Rule, system_lang: LangType) {
    if rule.language.trim().is_empty() {
        rule.language = system_lang.as_str().to_string();
    }

    let url = rule.url.clone();

    if let Some(s) = rule.search.as_mut() {
        if s.base_uri.is_empty() {
            s.base_uri = url.clone();
        }
        if s.timeout.is_none() {
            s.timeout = Some(15);
        }
    }
    if let Some(b) = rule.book.as_mut() {
        if b.base_uri.is_empty() {
            b.base_uri = url.clone();
        }
        if b.timeout.is_none() {
            b.timeout = Some(15);
        }
        // book 字段缺失时回落到 meta 查询（与 Java 端 `StrUtil.emptyToDefault` 等价）。
        if b.book_name.is_empty() {
            b.book_name = META_BOOK_NAME.to_string();
        }
        if b.author.is_empty() {
            b.author = META_AUTHOR.to_string();
        }
        if b.intro.is_empty() {
            b.intro = META_INTRO.to_string();
        }
        if b.cover_url.is_empty() {
            b.cover_url = META_COVER_URL.to_string();
        }
        if b.category.is_empty() {
            b.category = META_CATEGORY.to_string();
        }
        if b.latest_chapter.is_empty() {
            b.latest_chapter = META_LATEST_CHAPTER.to_string();
        }
        if b.latest_chapter_url.is_empty() {
            b.latest_chapter_url = META_LATEST_CHAPTER_URL.to_string();
        }
        if b.last_update_time.is_empty() {
            b.last_update_time = META_LAST_UPDATE_TIME.to_string();
        }
        if b.status.is_empty() {
            b.status = META_STATUS.to_string();
        }
    }
    if let Some(t) = rule.toc.as_mut() {
        if t.base_uri.is_empty() {
            t.base_uri = url.clone();
        }
        if t.timeout.is_none() {
            t.timeout = Some(60);
        }
    }
    if let Some(c) = rule.chapter.as_mut() {
        if c.base_uri.is_empty() {
            c.base_uri = url.clone();
        }
        if c.timeout.is_none() {
            c.timeout = Some(15);
        }
    }
}

// =====================================================================
// 目录初始化：init_rules_dir + list_rule_files
// =====================================================================

/// 初始化规则目录：创建目录 + 补齐 [`BUNDLED_RULES`] 缺失的规则文件。
///
/// - 目录不存在时创建；
/// - 已存在的文件**不覆盖**（尊重用户修改）；
/// - 返回新创建的文件数量。
pub fn init_rules_dir(rules_dir: &Path) -> std::io::Result<usize> {
    if !rules_dir.exists() {
        std::fs::create_dir_all(rules_dir)?;
    }

    let mut created = 0;
    for (filename, content) in BUNDLED_RULES {
        let path = rules_dir.join(filename);
        if !path.exists() {
            std::fs::write(&path, content)?;
            tracing::info!("已创建默认规则文件: {}", path.display());
            created += 1;
        }
    }

    if created > 0 {
        tracing::info!("规则目录初始化完成，创建了 {} 个文件", created);
    }

    Ok(created)
}

/// 列出目录下所有规则文件名（不含路径、跳过模板）。
/// 用于"切活跃书源文件"下拉的可选项填充。
pub fn list_rule_files(rules_dir: &Path) -> Vec<String> {
    if !rules_dir.exists() {
        return Vec::new();
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(rules_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                let name_lower = name.to_ascii_lowercase();
                if name_lower.starts_with("rule-template") {
                    continue;
                }
                if name_lower.ends_with(".json") || name_lower.ends_with(".json5") {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    files
}

// =====================================================================
// 活跃规则加载：load_active_rules + apply_disabled_urls
// =====================================================================

/// 从文件加载活跃书源规则（合并禁用状态）。
///
/// 主入口 — `app.rs` / `cli.rs` 都从这里拿规则。
/// 从 `rules_dir` 加载 `sources_config.active_file` 指定的文件，
/// 并合并 `sources_config.disabled_urls` 设置 `Rule.disabled`。
///
/// `load_rules_from_path` 内部已调用 `apply_default_rule`，本函数
/// 只负责合并禁用状态，不重复填充默认值。
pub fn load_active_rules(
    rules_dir: &Path,
    sources_config: &SourcesConfig,
) -> anyhow::Result<Vec<Rule>> {
    let file_path = rules_dir.join(&sources_config.active_file);
    if !file_path.exists() {
        tracing::warn!(
            "活跃书源文件不存在: {}，尝试加载目录下所有文件",
            file_path.display()
        );
        // 回退：加载目录下所有文件
        let mut rules = load_rules_from_path(rules_dir)?;
        apply_disabled_urls(&mut rules, &sources_config.disabled_urls);
        return Ok(rules);
    }

    let mut rules = load_rules_from_path(&file_path)?;
    apply_disabled_urls(&mut rules, &sources_config.disabled_urls);
    Ok(rules)
}

/// 按 `disabled_urls` 把对应 `Rule.disabled = true`。不调 `apply_default_rule` —
/// `load_rules_from_path` 已经做过了。
fn apply_disabled_urls(rules: &mut [Rule], disabled_urls: &HashSet<String>) {
    for rule in rules.iter_mut() {
        let url_key = rule.url.trim().to_lowercase();
        if disabled_urls.contains(&url_key) {
            rule.disabled = true;
        }
    }
}

// =====================================================================
// 测试
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_rules_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bundle")
            .join("rules")
    }

    // ----- 目录初始化 -----

    #[test]
    fn init_creates_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join("rules");

        let created = init_rules_dir(&rules_dir).unwrap();
        assert_eq!(created, 5);

        assert!(rules_dir.join("main.json").exists());
        assert!(rules_dir.join("cloudflare.json").exists());
        assert!(rules_dir.join("no-search.json").exists());
        assert!(rules_dir.join("rate-limit.json").exists());
        assert!(rules_dir.join("proxy-required.json").exists());
    }

    #[test]
    fn init_does_not_overwrite_existing() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join("rules");

        std::fs::create_dir_all(&rules_dir).unwrap();
        let custom_content = r#"[{"url":"https://custom.com","name":"Custom"}]"#;
        std::fs::write(rules_dir.join("main.json"), custom_content).unwrap();

        let created = init_rules_dir(&rules_dir).unwrap();
        assert_eq!(created, 4);

        let content = std::fs::read_to_string(rules_dir.join("main.json")).unwrap();
        assert_eq!(content, custom_content);
    }

    #[test]
    fn list_rule_files_excludes_templates() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).unwrap();

        std::fs::write(rules_dir.join("main.json"), "[]").unwrap();
        std::fs::write(rules_dir.join("custom.json5"), "[]").unwrap();
        std::fs::write(rules_dir.join("rule-template.json5"), "[]").unwrap();
        std::fs::write(rules_dir.join("readme.txt"), "text").unwrap();

        let files = list_rule_files(&rules_dir);
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"main.json".to_string()));
        assert!(files.contains(&"custom.json5".to_string()));
        assert!(!files.iter().any(|f| f.contains("template")));
        assert!(!files.iter().any(|f| f.contains("readme")));
    }

    // ----- 活跃规则加载 -----

    #[test]
    fn load_active_rules_uses_active_file() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).unwrap();

        std::fs::write(
            rules_dir.join("a.json"),
            r#"[{"url":"https://a.com","name":"A"}]"#,
        )
        .unwrap();
        std::fs::write(
            rules_dir.join("b.json"),
            r#"[{"url":"https://b.com","name":"B"}]"#,
        )
        .unwrap();

        let cfg = SourcesConfig {
            active_file: "b.json".to_string(),
            ..Default::default()
        };

        let rules = load_active_rules(&rules_dir, &cfg).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "B");
    }

    #[test]
    fn load_active_rules_applies_disabled_urls() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).unwrap();

        std::fs::write(
            rules_dir.join("main.json"),
            r#"[{"url":"https://a.com","name":"A"},{"url":"https://b.com","name":"B"}]"#,
        )
        .unwrap();

        let mut cfg = SourcesConfig::default();
        cfg.disabled_urls.insert("https://b.com".to_string());

        let rules = load_active_rules(&rules_dir, &cfg).unwrap();
        assert_eq!(rules.len(), 2);
        assert!(!rules[0].disabled);
        assert!(rules[1].disabled);
    }

    #[test]
    fn load_active_rules_falls_back_to_directory() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).unwrap();

        std::fs::write(
            rules_dir.join("a.json"),
            r#"[{"url":"https://a.com","name":"A"}]"#,
        )
        .unwrap();

        let cfg = SourcesConfig {
            active_file: "nonexistent.json".to_string(),
            ..Default::default()
        };

        let rules = load_active_rules(&rules_dir, &cfg).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "A");
    }

    // ----- 解析 + 默认值 -----

    #[test]
    fn loads_real_main_json() {
        let path = repo_rules_dir().join("main.json");
        assert!(path.exists(), "{} missing", path.display());
        let rules = load_rules_from_path(&path).unwrap();

        // main.json 的书源数量根据 BOOK_SOURCES.md 是 13；测试只断言 ≥ 5 防止误删一两个就崩。
        assert!(
            rules.len() >= 5,
            "expected ≥5 rules in main.json, got {}",
            rules.len()
        );

        // 第一个一定是"香书小说"（main.json 第一个 entry）。
        let first = &rules[0];
        assert_eq!(first.id, 1);
        assert!(first.name.contains("香书"));
        assert!(first.url.starts_with("http"));

        // 默认值填充检查：search.base_uri、book.book_name 应被填充。
        let s = first.search.as_ref().expect("search rule");
        assert!(!s.base_uri.is_empty(), "search.baseUri should be defaulted");
        let b = first.book.as_ref().expect("book rule");
        // 香书小说的 book 是空对象，因此 book.bookName 应回退到 meta 默认值。
        assert!(b.book_name.starts_with("meta["));
        assert!(b.author.starts_with("meta["));
    }

    #[test]
    fn loads_directory_recursively_skipping_template() {
        let dir = repo_rules_dir();
        let rules = load_rules_from_path(&dir).unwrap();

        // 现有 5 个规则文件 27 个书源。允许变化，但下限保守。
        assert!(
            rules.len() >= 20,
            "expected ≥20 rules across all files, got {}",
            rules.len()
        );

        // ID 从 1 起严格自增。
        for (idx, r) in rules.iter().enumerate() {
            assert_eq!(r.id, (idx + 1) as i32, "rule {} has wrong id {}", idx, r.id);
        }

        // 模板文件 rule-template.json5 必须被跳过。
        for r in &rules {
            assert_ne!(
                r.url, "",
                "url must not be empty (template should have been skipped)"
            );
        }
    }

    #[test]
    fn applies_meta_defaults_to_empty_book_section() {
        // 构造一条最小规则，验证 apply_default_rule。
        let json = r##"{
            "url": "https://example.com/",
            "name": "demo",
            "search": { "url": "https://example.com/s?q=%s", "method": "get",
                        "result": ".item", "bookName": ".name a" },
            "book": {},
            "toc": { "item": "dl > dd > a" },
            "chapter": { "title": "h1", "content": "#content",
                         "paragraphTagClosed": true, "filterTxt": "", "filterTag": "" }
        }"##;
        let mut rule: Rule = serde_json::from_str(json).unwrap();

        apply_default_rule(&mut rule, LangType::ZhCn);
        let b = rule.book.unwrap();
        assert_eq!(b.book_name, META_BOOK_NAME);
        assert_eq!(b.author, META_AUTHOR);
        assert_eq!(b.cover_url, META_COVER_URL);
        assert_eq!(b.intro, META_INTRO);
        assert_eq!(b.base_uri, "https://example.com/");
    }

    #[test]
    fn parses_json5_with_comments() {
        // 写一个带注释的 json5 临时文件，确认能解析。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("demo.json5");
        std::fs::write(
            &path,
            r#"// 顶部注释
[
  {
    url: "https://demo.test/",
    name: "demo",
    /* 多行注释
       OK */
    search: { url: "https://demo.test/?q=%s", method: "get",
              result: ".x", bookName: ".n a" }
  }
]"#,
        )
        .unwrap();

        let rules = load_rules_from_path(&path).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "demo");
    }
}
