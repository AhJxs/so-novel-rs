//! 书源规则的文件 I/O：目录初始化 + 文件加载。
//!
//! - 首次启动时将编译期嵌入的规则文件复制到 `~/.sonovel/rules/`；
//! - 从文件或目录加载规则，合并禁用状态。
//!
//! 注意：`load_rules_from_path`、`apply_default_rule`、`RulesError` 等底层
//! 解析逻辑在 `crate::rules` 模块中，本模块只负责目录初始化和活跃规则加载。

use std::collections::HashSet;
use std::path::Path;

use crate::models::Rule;
use crate::rules::load_rules_from_path;

use super::SourcesConfig;

// ─── 目录初始化 ────────────────────────────────────────

/// 编译期嵌入的规则文件列表。
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

/// 初始化规则目录：创建目录 + 补齐缺失的规则文件。
///
/// - 目录不存在时创建；
/// - 已存在的文件不覆盖（尊重用户修改）；
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

/// 获取所有可用的规则文件名（不含路径）。
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
                // 跳过模板文件
                if name_lower.starts_with("rule-template") {
                    continue;
                }
                // 只接受 .json 和 .json5 文件
                if name_lower.ends_with(".json") || name_lower.ends_with(".json5") {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    files
}

// ─── 活跃规则加载 ────────────────────────────────────

/// 从文件加载活跃书源规则（合并禁用状态）。
///
/// 这是主入口 — `app.rs` / `cli.rs` 都从这里拿规则。
/// 从 `rules_dir` 加载 `sources_config.active_file` 指定的文件，
/// 并合并 `sources_config.disabled_urls` 设置 `Rule.disabled`。
///
/// 注意：`load_rules_from_path` 内部已调用 `apply_default_rule`，
/// 本函数只负责合并禁用状态，不重复填充默认值。
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

/// 应用禁用 URL 列表到规则上。
///
/// 不调用 `apply_default_rule` —— `load_rules_from_path` 已经做过了。
fn apply_disabled_urls(rules: &mut [Rule], disabled_urls: &HashSet<String>) {
    for rule in rules.iter_mut() {
        let url_key = rule.url.trim().to_lowercase();
        if disabled_urls.contains(&url_key) {
            rule.disabled = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let mut cfg = SourcesConfig::default();
        cfg.active_file = "b.json".to_string();

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

        let mut cfg = SourcesConfig::default();
        cfg.active_file = "nonexistent.json".to_string();

        let rules = load_active_rules(&rules_dir, &cfg).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "A");
    }
}
