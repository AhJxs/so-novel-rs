//! CLI / desktop 共用的书源（Rule）查找 + 解析 + URL 键规范化。
//!
//! 原先 desktop / cli / db 多处各自写同一套 `iter().find(|r| r.id == id)` 或 `r.url.trim().to_lowercase()`
//! 的重复；抽到这里后调用方只用 `find_rule_by_id` / `rule_key` / `disabled_url_key`。
//!
//! key 契约：`SourcesConfig::toggle_disabled` 写 set 时同样 `trim + to_lowercase`，`disabled_url_key` 必须与之
//! 完全一致，否则禁用状态读不回。
//! `parse_rules_bytes` 是"纯字节 → Vec<Rule>"薄壳（4 步 fallback），刻意不复用按后缀分流、走 `RulesError` 的 `parse_one_file`。

use std::path::Path;

use crate::db::{SourcesConfig, load_active_rules};
use crate::models::{Rule, Source};

/// 把 `Rule.url` 标准化为书源查找键（`trim` + `to_lowercase`）。
///
/// ID 在不同 `sources_config.active_file` 间不复用，跨内存查找必须按 URL 归一后比对；
/// `SourcesConfig::toggle_disabled` 用同样的规范化，两者语义必须一致。
pub fn rule_key(rule: &Rule) -> String {
    rule.url.trim().to_lowercase()
}

/// 把任意 URL 字符串标准化为 `SourcesConfig.disabled_urls` 用的键。
/// 等价于 `toggle_disabled` 的内部归一逻辑；导出给两端调用方，避免各自再写一遍。
pub fn disabled_url_key(url: &str) -> String {
    url.trim().to_lowercase()
}

/// 在规则列表里按 ID 找（返回借用，生命周期绑到 `rules`）。
pub fn find_rule_by_id(rules: &[Rule], id: i32) -> Option<&Rule> {
    rules.iter().find(|r| r.id == id)
}

/// 解析规则文件字节 —— 支持严格 JSON / JSON5，单 Rule 或 Vec<Rule>。
///
/// 4 步 fallback：`serde_json` Vec → 单 Rule → `json5` Vec → 单 Rule。**不做** `apply_default_rule`
/// 与 ID 分配（那是 `db::load_rules_from_path`），只做"字节能解析成 Rule"的内容校验。
///
/// # Errors
///
/// 4 步解析全失败 → `anyhow::Error`，错误信息附每一步的根因。
pub fn parse_rules_bytes(bytes: &[u8], path: &Path) -> anyhow::Result<Vec<Rule>> {
    let text = String::from_utf8_lossy(bytes);
    let ctx = format!("解析规则文件失败: {}", path.display());

    serde_json::from_str::<Vec<Rule>>(&text)
        .or_else(|_| serde_json::from_str::<Rule>(&text).map(|r| vec![r]))
        .or_else(|_| json5::from_str::<Vec<Rule>>(&text))
        .or_else(|_| json5::from_str::<Rule>(&text).map(|r| vec![r]))
        .map_err(|e| anyhow::anyhow!("{ctx}: {e}"))
}

/// 加载活跃规则 + 合并 `SourcesConfig.disabled_urls`（`db::load_active_rules` 的薄壳，
/// 提供稳定的 core 层入口，DB 层换实现时只改这一处）。
///
/// # Errors
///
/// `RulesError::*` → `db` 层 IO / 解析 / 资源不存在错误。
pub fn load_active(rules_dir: &Path, sources_config: &SourcesConfig) -> anyhow::Result<Vec<Rule>> {
    load_active_rules(rules_dir, sources_config)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .map_err(|e: anyhow::Error| {
            // 保留路径信息（db::DaoError::Rules 来源的 RulesError::Io 已含 path）
            tracing::warn!(error = %e, "加载活跃规则失败");
            e
        })
}

/// 按 URL origin 自动匹配书源（CLI `run_download` 的 "用户给 URL，自动选源" 用）。
///
/// 遍历 `sources`，**不**依赖 `is_search_enabled`（下载场景无视 `search_disabled`），取第一个 origin
/// 相同的 source（多个匹配按列表顺序）；`url` 或 rule URL 解析失败 → 跳过 / 返回 `None`。
pub fn match_source_by_url<'a>(sources: &'a [Source], url: &str) -> Option<&'a Source> {
    let parsed = url::Url::parse(url).ok()?;
    let origin = parsed.origin();
    sources
        .iter()
        .find(|s| url::Url::parse(&s.rule.url).is_ok_and(|u| u.origin() == origin))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::models::RuleSearch;
    use std::collections::HashSet;

    fn rule(id: i32, url: &str, disabled: bool) -> Rule {
        Rule {
            id,
            url: url.to_string(),
            name: format!("src-{id}"),
            disabled,
            search: Some(RuleSearch::default()),
            ..Rule::default()
        }
    }

    #[test]
    fn rule_key_trims_and_lowercases() {
        let r = rule(1, "  HTTPS://Example.COM/path  ", false);
        assert_eq!(rule_key(&r), "https://example.com/path");
    }

    #[test]
    fn disabled_url_key_trims_and_lowercases() {
        assert_eq!(disabled_url_key("  HTTPS://X  "), "https://x");
        assert_eq!(disabled_url_key(""), "");
        assert_eq!(disabled_url_key("   "), "");
    }

    #[test]
    fn rule_key_matches_sources_config_toggle() {
        // 关键一致性：rule_key(r) 必须 == toggle_disabled 写入的 key，否则禁用状态读不回（假阴性）。
        let r = rule(1, "  HTTPS://Example.com/foo  ", false);
        let mut cfg = SourcesConfig::default();
        let stored_key = {
            // 模拟 toggle_disabled 内部的归一（set 不可克隆，用 disabled_url_key 等价替代）。
            cfg.toggle_disabled(&r.url);
            cfg.disabled_urls.iter().next().cloned().unwrap()
        };
        assert_eq!(stored_key, rule_key(&r));
    }

    #[test]
    fn find_rule_by_id_finds_match() {
        let rules = vec![rule(1, "https://a", false), rule(2, "https://b", false)];
        let r = find_rule_by_id(&rules, 2).unwrap();
        assert_eq!(r.id, 2);
        assert_eq!(r.url, "https://b");
    }

    #[test]
    fn find_rule_by_id_returns_none_when_missing() {
        let rules = vec![rule(1, "https://a", false)];
        assert!(find_rule_by_id(&rules, 99).is_none());
    }

    #[test]
    fn find_rule_by_id_empty_rules_returns_none() {
        assert!(find_rule_by_id(&[], 1).is_none());
    }

    #[test]
    fn parse_json_vec_of_rules() {
        let json = r#"[
            {"url": "https://a", "name": "A"},
            {"url": "https://b", "name": "B"}
        ]"#;
        let rules = parse_rules_bytes(json.as_bytes(), Path::new("a.json")).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].url, "https://a");
        assert_eq!(rules[1].name, "B");
    }

    #[test]
    fn parse_json_single_rule() {
        let json = r#"{"url": "https://only", "name": "Only"}"#;
        let rules = parse_rules_bytes(json.as_bytes(), Path::new("only.json")).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].url, "https://only");
    }

    #[test]
    fn parse_json5_with_comments() {
        let json5 = r#"// 注释行
[
    {
        "url": "https://x",
        "name": "测试源",  // 行尾注释
    }
]"#;
        let rules = parse_rules_bytes(json5.as_bytes(), Path::new("x.json5")).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "测试源");
    }

    #[test]
    fn parse_json5_single_rule() {
        let json5 = r#"{ url: "https://x", name: "X" }"#;
        let rules = parse_rules_bytes(json5.as_bytes(), Path::new("x.json5")).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].url, "https://x");
    }

    #[test]
    fn parse_invalid_bytes_returns_error() {
        let garbage = "not even close to JSON or JSON5 }{][";
        let result = parse_rules_bytes(garbage.as_bytes(), Path::new("bad.json"));
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("bad.json"), "error should mention path: {msg}");
    }

    #[test]
    fn parse_does_not_assign_ids() {
        let json = r#"[{"url": "https://a"}, {"url": "https://b"}]"#;
        let rules = parse_rules_bytes(json.as_bytes(), Path::new("a.json")).unwrap();
        for r in &rules {
            assert_eq!(r.id, 0, "parse_rules_bytes must not assign IDs");
        }
    }

    #[test]
    fn load_active_uses_sources_config_active_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).expect("mkdir");
        let rules_json = r#"[{"url":"https://a","name":"A"}]"#;
        std::fs::write(rules_dir.join("main.json"), rules_json).expect("write");

        let cfg = SourcesConfig {
            active_file: "main.json".to_string(),
            disabled_urls: HashSet::new(),
        };
        let rules = load_active(&rules_dir, &cfg).expect("load_active");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].url, "https://a");
        // db::load_rules_from_path 内部赋 ID（从 1 起）
        assert_eq!(rules[0].id, 1);
    }

    #[test]
    fn load_active_applies_disabled_urls() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).expect("mkdir");
        let rules_json = r#"[
            {"url":"https://a","name":"A"},
            {"url":"https://b","name":"B"}
        ]"#;
        std::fs::write(rules_dir.join("main.json"), rules_json).expect("write");

        let mut disabled = HashSet::new();
        disabled.insert("https://b".to_string());
        let cfg = SourcesConfig {
            active_file: "main.json".to_string(),
            disabled_urls: disabled,
        };
        let rules = load_active(&rules_dir, &cfg).expect("load_active");
        let b = rules.iter().find(|r| r.url == "https://b").unwrap();
        assert!(
            b.disabled,
            "rule with url in disabled_urls should be disabled"
        );
        // disabled_urls 含不存在的 URL → 静默 no-op
        let mut with_phantom = HashSet::new();
        with_phantom.insert("https://does-not-exist".to_string());
        let cfg2 = SourcesConfig {
            active_file: "main.json".to_string(),
            disabled_urls: with_phantom,
        };
        let rules2 = load_active(&rules_dir, &cfg2).expect("load_active");
        assert!(rules2.iter().all(|r| !r.disabled));
    }

    #[test]
    fn match_source_by_url_finds_matching_origin() {
        let cfg = crate::config::AppConfig::default();
        let sources = vec![
            Source::from(rule(1, "https://a.com", false), &cfg),
            Source::from(rule(2, "https://b.com", false), &cfg),
        ];
        let s = match_source_by_url(&sources, "https://b.com/book/123").unwrap();
        assert_eq!(s.rule.id, 2);
    }

    #[test]
    fn match_source_by_url_ignores_path() {
        // origin 只比对 scheme://host[:port]，不比对 path
        let cfg = crate::config::AppConfig::default();
        let sources = vec![Source::from(rule(1, "https://a.com/book", false), &cfg)];
        let s = match_source_by_url(&sources, "https://a.com/different/path?q=1").unwrap();
        assert_eq!(s.rule.id, 1);
    }

    #[test]
    fn match_source_by_url_returns_none_for_invalid_target_url() {
        let cfg = crate::config::AppConfig::default();
        let sources = vec![Source::from(rule(1, "https://a.com", false), &cfg)];
        assert!(match_source_by_url(&sources, "not a url at all").is_none());
    }

    #[test]
    fn match_source_by_url_skips_malformed_rule_urls() {
        let cfg = crate::config::AppConfig::default();
        let sources = vec![
            Source::from(rule(1, "not a url", false), &cfg),
            Source::from(rule(2, "https://good.com", false), &cfg),
        ];
        let s = match_source_by_url(&sources, "https://good.com/x").unwrap();
        assert_eq!(s.rule.id, 2);
    }

    #[test]
    fn match_source_by_url_returns_first_when_multiple_match() {
        // 多个 rule 同 origin → 列表顺序第一个
        let cfg = crate::config::AppConfig::default();
        let sources = vec![
            Source::from(rule(1, "https://shared.com", false), &cfg),
            Source::from(rule(2, "https://shared.com/alt", false), &cfg),
        ];
        let s = match_source_by_url(&sources, "https://shared.com/anything").unwrap();
        assert_eq!(s.rule.id, 1);
    }

    #[test]
    fn match_source_by_url_returns_none_when_no_match() {
        let cfg = crate::config::AppConfig::default();
        let sources = vec![Source::from(rule(1, "https://a.com", false), &cfg)];
        assert!(match_source_by_url(&sources, "https://other.com/x").is_none());
    }

    #[test]
    fn match_source_by_url_does_not_filter_by_search_disabled() {
        // 下载场景无视 search_disabled：故意让 rule.search.disabled = true，
        // 仍然要被命中。
        let cfg = crate::config::AppConfig::default();
        let mut r = rule(1, "https://a.com", false);
        r.search = Some(RuleSearch {
            disabled: true,
            ..RuleSearch::default()
        });
        let sources = vec![Source::from(r, &cfg)];
        let s = match_source_by_url(&sources, "https://a.com/x").unwrap();
        assert_eq!(s.rule.id, 1);
    }

    #[test]
    fn match_source_by_url_ignores_query_and_hash() {
        // origin 比对不含 query / fragment；浏览器粘贴的 URL 几乎都带 `?utm_source=...` 或 `#chapter-N`，
        // 必须不影响匹配。
        let cfg = crate::config::AppConfig::default();
        let sources = vec![Source::from(rule(1, "https://a.com", false), &cfg)];
        assert_eq!(
            match_source_by_url(&sources, "https://a.com/p?q=1")
                .unwrap()
                .rule
                .id,
            1
        );
        assert_eq!(
            match_source_by_url(&sources, "https://a.com/p#fragment")
                .unwrap()
                .rule
                .id,
            1
        );
        assert_eq!(
            match_source_by_url(&sources, "https://a.com/p?q=1#fragment&x=y")
                .unwrap()
                .rule
                .id,
            1
        );
    }

    #[test]
    fn match_source_by_url_handles_port_difference() {
        // origin 含 port：`https://a.com:8080` 不会被 `https://a.com` 命中（防端口错配误命中）。
        let cfg = crate::config::AppConfig::default();
        let sources = vec![Source::from(rule(1, "https://a.com:8080", false), &cfg)];
        assert!(
            match_source_by_url(&sources, "https://a.com/foo").is_none(),
            "rule 带 :8080，URL 不带端口 → 不应命中"
        );
        assert!(
            match_source_by_url(&sources, "https://a.com:8080/foo").is_some(),
            "URL 带 :8080 → 应命中"
        );
        assert!(
            match_source_by_url(&sources, "https://a.com:9090/foo").is_none(),
            "URL 带不同端口 :9090 → 不应命中"
        );
    }

    #[test]
    fn empty_inputs_are_safe() {
        assert_eq!(rule_key(&Rule::default()), "");
        assert_eq!(disabled_url_key(""), "");
        assert!(find_rule_by_id(&[], 0).is_none());
        assert!(match_source_by_url(&[], "https://x").is_none());
        assert!(parse_rules_bytes(b"", Path::new("empty.json")).is_err());
    }
}
