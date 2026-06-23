//! 规则文件加载。
//!
//! - 支持加载单个 `.json`/`.json5` 文件，或一个包含多个规则文件的目录；
//! - 加载后填充默认值：baseUri、timeout、book.* 的 meta 后备查询；
//! - 自增 ID（与 Java 端一致：从 1 开始）。
//!
//! 注意：`load_active_rules`、`init_rules_dir`、`list_rule_files` 已迁移到
//! `crate::persistent::rules`，本模块保留 `load_rules_from_path` 和 `apply_default_rule`
//! 供 parser 模块使用。

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::config::LangType;
use crate::models::Rule;
use crate::util::lang::detect_system_lang;

// ----- meta 默认查询（与 Java `util.SourceUtils` 常量一致）-----

pub const META_BOOK_NAME: &str = r#"meta[property="og:novel:book_name"]"#;
pub const META_AUTHOR: &str = r#"meta[property="og:novel:author"]"#;
pub const META_INTRO: &str = r#"meta[name="description"]"#;
pub const META_CATEGORY: &str = r#"meta[property="og:novel:category"]"#;
pub const META_COVER_URL: &str = r#"meta[property="og:image"]"#;
pub const META_LATEST_CHAPTER: &str = r#"meta[property="og:novel:latest_chapter_name"]"#;
pub const META_LATEST_CHAPTER_URL: &str = r#"meta[property="og:novel:latest_chapter_url"]"#;
pub const META_LAST_UPDATE_TIME: &str = r#"meta[property="og:novel:update_time"]"#;
pub const META_STATUS: &str = r#"meta[property="og:novel:status"]"#;

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

/// 加载一个规则路径。
///
/// - 若 `path` 是文件（`.json` / `.json5`），把它当成 `Vec<Rule>` 解析；
/// - 若 `path` 是目录，递归查找 `*.json` / `*.json5`（跳过 `rule-template.json5` 这类模板）；
/// - 加载完毕统一填默认值，并按顺序赋自增 ID。
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

        // 跳过模板：rule-template.json5
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

    // json5 是 json 的超集，遇到带注释的 .json 也用 json5 兜底。
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

/// 默认值填充。等价于 Java `util.SourceUtils#applyDefaultRule`。
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

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_rules_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bundle")
            .join("rules")
    }

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
