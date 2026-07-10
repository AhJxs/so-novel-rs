//! 规则文件加载
//!
//! 3 个公共 fn + 2 个私有 helper:
//! - [`load_rules_from_path`] — 公共入口: 加载文件或目录
//! - [`load_active_rules`] — 公共入口: 从 `SourcesConfig.active_file` 加载并合并禁用状态
//! - [`walk_rule_files`] — 私有: 递归枚举目录
//! - [`parse_one_file`] — 私有: 解析单文件 (json + json5)
//! - [`apply_disabled_urls`] — 私有: 按 `disabled_urls` 设 Rule.disabled = true

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::core::sources as core_sources;
use crate::models::Rule;
use crate::utils::lang::detect_system_lang;

use super::apply_default::apply_default_rule;
use super::error::RulesError;
use crate::db::SourcesConfig;

/// 加载一个规则路径 (文件或目录)。
///
/// - 若 `path` 是 `.json` / `.json5` 文件, 直接当 `Vec<Rule>` 解析;
/// - 若 `path` 是目录, 递归查找 `*.json` / `*.json5` (跳过 `rule-template.json5`
///   这类模板);
/// - 加载完毕统一调 [`apply_default_rule`] 填默认值, 并按顺序赋自增 ID。
///
/// # Examples
///
/// ```ignore
/// // 加载单个文件
/// let rules = load_rules_from_path(Path::new("main.json"))?;
/// // 加载整个目录
/// let rules = load_rules_from_path(rules_dir)?;
/// ```
///
/// # Errors
///
/// - `RulesError::NotFound` — 路径不存在
/// - `RulesError::Io` — 读取失败 (含 path)
/// - `RulesError::Parse` — JSON / JSON5 解析失败
pub fn load_rules_from_path(path: &Path) -> Result<Vec<Rule>, RulesError> {
    if !path.exists() {
        return Err(RulesError::NotFound(path.to_path_buf()));
    }

    let mut rules = Vec::new();
    if path.is_file() {
        rules.extend(parse_one_file(path)?);
    } else {
        // 目录: 枚举所有规则文件
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
        rule.id = i32::try_from(idx + 1).unwrap_or(i32::MAX);
    }

    Ok(rules)
}

/// 从文件加载活跃书源规则 (合并禁用状态)。
///
/// 主入口 — `app.rs` / `cli.rs` 都从这里拿规则。
/// 从 `rules_dir` 加载 `sources_config.active_file` 指定的文件,
/// 并合并 `sources_config.disabled_urls` 设置 `Rule.disabled`。
///
/// `load_rules_from_path` 内部已调用 `apply_default_rule`, 本函数
/// 只负责合并禁用状态, 不重复填充默认值。
///
/// # Errors
///
/// - `RulesError::*` (来自 `load_rules_from_path`)
pub fn load_active_rules(
    rules_dir: &Path,
    sources_config: &SourcesConfig,
) -> anyhow::Result<Vec<Rule>> {
    let file_path = rules_dir.join(&sources_config.active_file);
    if !file_path.exists() {
        tracing::warn!(
            "活跃书源文件不存在: {}, 尝试加载目录下所有文件",
            file_path.display()
        );
        // 回退: 加载目录下所有文件
        let mut rules = load_rules_from_path(rules_dir)?;
        apply_disabled_urls(&mut rules, &sources_config.disabled_urls);
        return Ok(rules);
    }

    let mut rules = load_rules_from_path(&file_path)?;
    apply_disabled_urls(&mut rules, &sources_config.disabled_urls);
    Ok(rules)
}

/// 递归枚举目录下所有 `.json` / `.json5` 文件。
/// 跳过 `rule-template.json5` 这类模板 (保留给用户当参考, 不要当成书源加载)。
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
            .map(str::to_ascii_lowercase)
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

/// 解析单个规则文件: `.json5` 直接走 json5 解析; `.json` 先用严格
/// `serde_json`, 失败再用 json5 兜底 (带注释的 .json 也能加载)。
fn parse_one_file(path: &Path) -> Result<Vec<Rule>, RulesError> {
    let bytes = std::fs::read(path).map_err(|e| RulesError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let text = String::from_utf8_lossy(&bytes);
    let is_json5 = path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("json5"));

    let rules: Vec<Rule> = if is_json5 {
        json5::from_str(&text).map_err(|e| RulesError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?
    } else {
        // 现有 main.json 等是严格 JSON, 先用 serde_json, 失败再用 json5 兜底
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

/// 按 `disabled_urls` 把对应 `Rule.disabled = true`。不调 `apply_default_rule` —
/// `load_rules_from_path` 已经做过了。
fn apply_disabled_urls(rules: &mut [Rule], disabled_urls: &HashSet<String>) {
    for rule in rules.iter_mut() {
        // 键归一走 core::sources::rule_key —— 与 SourcesConfig::toggle_disabled 同源
        if disabled_urls.contains(&core_sources::rule_key(rule)) {
            rule.disabled = true;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::path::PathBuf;

    fn repo_rules_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bundle")
            .join("rules")
    }

    #[test]
    fn load_bundled_main_json() {
        let main = repo_rules_dir().join("main.json");
        let rules = load_rules_from_path(&main).unwrap();
        assert!(!rules.is_empty(), "main.json should have rules");
        // id 必须从 1 开始, 连续
        for (i, r) in rules.iter().enumerate() {
            assert_eq!(
                r.id,
                i32::try_from(i + 1).unwrap(),
                "id should be sequential 1-based"
            );
        }
    }

    #[test]
    fn load_bundled_directory_collects_all_rules() {
        let dir = repo_rules_dir();
        let rules = load_rules_from_path(&dir).unwrap();
        // 至少 main.json 5 个 + 其他几个 = > 10
        assert!(rules.len() > 10, "expected > 10 rules, got {}", rules.len());
    }

    #[test]
    fn load_active_rules_uses_config_active_file() {
        let dir = repo_rules_dir();
        let cfg = SourcesConfig::default(); // active_file = "main.json"
        let rules = load_active_rules(&dir, &cfg).unwrap();
        assert!(!rules.is_empty());
    }

    #[test]
    fn load_active_rules_falls_back_to_dir_on_missing_file() {
        let dir = repo_rules_dir();
        let cfg = SourcesConfig {
            active_file: "nonexistent.json".into(),
            ..SourcesConfig::default()
        };
        // 不 panic, 退到整个目录
        let rules = load_active_rules(&dir, &cfg).unwrap();
        assert!(!rules.is_empty());
    }

    #[test]
    fn load_active_rules_applies_disabled_urls() {
        let dir = repo_rules_dir();
        let mut cfg = SourcesConfig::default();
        // 拿第一条规则的 url, 加入 disabled_urls
        let rules_sample = load_rules_from_path(&dir.join(&cfg.active_file)).unwrap();
        assert!(!rules_sample.is_empty());
        let target = rules_sample[0].url.clone();
        cfg.disabled_urls.insert(target.trim().to_lowercase());

        let rules = load_active_rules(&dir, &cfg).unwrap();
        let r0 = rules.iter().find(|r| r.url == target).unwrap();
        assert!(r0.disabled, "rule should be disabled");
    }

    #[test]
    fn load_not_found_returns_typed_error() {
        let err = load_rules_from_path(Path::new("/nonexistent/path/x.json")).unwrap_err();
        assert!(matches!(err, RulesError::NotFound(_)));
    }

    #[test]
    fn parses_json5_with_comments() {
        // 临时目录写一个带注释的 .json5
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("test.json5");
        std::fs::write(
            &p,
            r#"// 注释行
[
  {
    "url": "https://test.example",
    "name": "测试源",
    "language": "zh-CN"
  }
]"#,
        )
        .unwrap();
        let rules = load_rules_from_path(&p).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "测试源");
    }
}
