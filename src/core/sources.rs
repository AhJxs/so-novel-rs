//! 三端共用的书源（Rule）查找 + 解析 + URL 键规范化。
//!
//! 之前多处各自写：
//!
//! ```ignore
//! // desktop/model/ops/sources.rs:22-29 — toggle_source_disabled
//! // desktop/model/ops/sources.rs:105-115 — delete_source
//! // desktop/model/sources_state.rs:95 — filtered_rules
//! // db/rules/loader.rs:174 — apply_disabled_urls
//! // cli/sources.rs:72 — run_set_disabled
//! // web/handlers/sources.rs:67,87 — source_toggle
//! // web/handlers/book.rs:30 — extract_config_and_rule
//! ```
//!
//! 全是同一套 `iter().find(|r| r.id == id)` 或
//! `iter().find(|r| r.url.trim().to_lowercase() == key)` 的字面量重复。
//!
//! 抽出后调用方收敛为单行 `find_rule_by_id(rules, id)` / `find_rule_by_url(rules, url)` /
//! `rule_key(rule)` / `disabled_url_key(url)` —— 不再需要在多处保持 "url key 怎么算" 的语义同步。
//!
//! ## 关于 key 规范化
//!
//! `SourcesConfig::toggle_disabled` 内部已经做 `url.trim().to_lowercase()`，所以
//! 调用 `toggle_disabled("  HTTPS://X  ")` 写入 set 的 key 与
//! `disabled_url_key("  HTTPS://X  ")` 完全一致。
//!
//! 同样 `Rule.url` 来自 JSON 解析（无 trim），但内存里查找时都按 `rule.url.trim().to_lowercase()`
//! 归一后比对 —— 这就是 `rule_key` 与 `find_rule_by_url` 之间的契约。
//!
//! ## 关于 `parse_rules_bytes`
//!
//! 桌面端导入书源文件时已经做了"先解析一次确认有效再 copy"的预检（`add_sources_from_file`）。
//! 解析链是 4 步 fallback：严格 JSON `Vec<Rule>` → 严格 JSON 单 `Rule` → json5 `Vec<Rule>`
//! → json5 单 `Rule`。**这里不复用** `db::rules::loader::parse_one_file`：那个函数
//! 还按 `.json5` 后缀做分流 + 走 `RulesError` 类型；这里要的是一个"纯字节 → Vec<Rule>"
//! 的薄壳，能 throw `anyhow::Error`，适合前端 handler / CLI 导入。

use std::path::Path;

use crate::db::{SourcesConfig, load_active_rules};
use crate::models::{Rule, Source};

/// 把 `Rule.url` 标准化为书源查找键（`trim` + `to_lowercase`）。
///
/// 用于跨文件 / 跨内存的 Rule 查找 —— ID 在不同 `sources_config.active_file` 间不复用，
/// 必须按 URL 归一后比对。`SourcesConfig::toggle_disabled` 内部用同样的规范化，
/// 所以 `disabled_urls.contains(&rule_key(r))` 一定等于
/// `disabled_urls.contains(&r.url.trim().to_lowercase())`。
pub fn rule_key(rule: &Rule) -> String {
    rule.url.trim().to_lowercase()
}

/// 把任意 URL 字符串标准化为 `SourcesConfig.disabled_urls` 用的键。
///
/// 等价于 `SourcesConfig::toggle_disabled` 内部归一逻辑；导出给三端调用方，
/// 避免桌面 / web / CLI 各自再写一遍 `url.trim().to_lowercase()`。
pub fn disabled_url_key(url: &str) -> String {
    url.trim().to_lowercase()
}

/// 在规则列表里按 ID 找（返回借用，生命周期绑到 `rules`）。
pub fn find_rule_by_id(rules: &[Rule], id: i32) -> Option<&Rule> {
    rules.iter().find(|r| r.id == id)
}

/// 在规则列表里按 ID 找（返回 owned `Rule`，用于跨锁边界 / `Send`）。
pub fn find_rule_by_id_cloned(rules: &[Rule], id: i32) -> Option<Rule> {
    rules.iter().find(|r| r.id == id).cloned()
}

/// 在规则列表里按 URL 键（`disabled_url_key` 归一）找。
///
/// 找不到时返回 `None`；规则 URL 包含前后空白 / 大小写不一致不影响。
pub fn find_rule_by_url<'a>(rules: &'a [Rule], url: &str) -> Option<&'a Rule> {
    let key = disabled_url_key(url);
    rules.iter().find(|r| rule_key(r) == key)
}

/// 解析规则文件字节 —— 支持严格 JSON / JSON5，单 Rule 或 Vec<Rule>。
///
/// 4 步 fallback：
/// 1. `serde_json::from_str::<Vec<Rule>>`
/// 2. `serde_json::from_str::<Rule>` 包成单元素 vec
/// 3. `json5::from_str::<Vec<Rule>>`
/// 4. `json5::from_str::<Rule>` 包成单元素 vec
///
/// **不做** `apply_default_rule` + 分配 ID —— 那是 `db::load_rules_from_path` 的事。
/// 这里只做"字节能解析成 Rule"的内容校验（桌面 `add_sources_from_file` 预检用）。
///
/// # Errors
///
/// - 4 步解析全失败 → `anyhow::Error`，错误信息附每一步的根因。
pub fn parse_rules_bytes(bytes: &[u8], path: &Path) -> anyhow::Result<Vec<Rule>> {
    let text = String::from_utf8_lossy(bytes);
    let ctx = format!("解析规则文件失败: {}", path.display());

    serde_json::from_str::<Vec<Rule>>(&text)
        .or_else(|_| serde_json::from_str::<Rule>(&text).map(|r| vec![r]))
        .or_else(|_| json5::from_str::<Vec<Rule>>(&text))
        .or_else(|_| json5::from_str::<Rule>(&text).map(|r| vec![r]))
        .map_err(|e| anyhow::anyhow!("{ctx}: {e}"))
}

/// 加载活跃规则 + 合并 `SourcesConfig.disabled_urls`。
///
/// `db::load_active_rules` 的薄壳，提供稳定的 core 层入口；将来如果
/// DB 层换实现（异步 / 索引），只改这里一处。
///
/// # Errors
///
/// - `RulesError::*` → `db` 层 IO / 解析 / 资源不存在错误。
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
/// 遍历 `sources`（**不**依赖 `is_search_enabled` —— 下载场景无视 `search_disabled`），
/// 找第一个 rule URL 与目标 URL origin 相同的 source。多个匹配时取第一个（按列表顺序，
/// 桌面端按 id 升序，CLI 端按 load 顺序）。
///
/// 行为：
/// - `url` 解析失败 → `None`
/// - 任意一方 URL 解析失败（畸形 rule URL）→ 该 rule 跳过
/// - 没匹配 → `None`
pub fn match_source_by_url<'a>(sources: &'a [Source], url: &str) -> Option<&'a Source> {
    let parsed = url::Url::parse(url).ok()?;
    let origin = parsed.origin();
    sources.iter().find(|s| {
        url::Url::parse(&s.rule.url)
            .ok()
            .is_some_and(|u| u.origin() == origin)
    })
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

    // ── rule_key / disabled_url_key ─────────────────────────

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
        // 关键一致性：rule_key(r) 必须 == SourcesConfig::toggle_disabled 写入的 key，
        // 否则 `disabled_urls.contains(&rule_key(r))` 会假阴性 → 禁用状态读不回。
        let r = rule(1, "  HTTPS://Example.com/foo  ", false);
        let mut cfg = SourcesConfig::default();
        let stored_key = {
            // 模拟 toggle_disabled 内部的归一（不能直接调，因为 set 不可克隆；
            // 走 disabled_url_key 等价）
            cfg.toggle_disabled(&r.url);
            // 再从 set 里拿出来比对
            cfg.disabled_urls.iter().next().cloned().unwrap()
        };
        assert_eq!(stored_key, rule_key(&r));
    }

    // ── find_rule_by_id ──────────────────────────────────────

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

    // ── find_rule_by_id_cloned ───────────────────────────────

    #[test]
    fn find_rule_by_id_cloned_returns_owned() {
        let rules = vec![rule(1, "https://a", true)];
        let r = find_rule_by_id_cloned(&rules, 1).unwrap();
        assert_eq!(r.id, 1);
        assert!(r.disabled);
    }

    // ── find_rule_by_url ─────────────────────────────────────

    #[test]
    fn find_rule_by_url_matches_case_insensitively() {
        let rules = vec![rule(1, "https://Example.com/Path", false)];
        let r = find_rule_by_url(&rules, "https://example.com/path").unwrap();
        assert_eq!(r.id, 1);
    }

    #[test]
    fn find_rule_by_url_matches_with_surrounding_whitespace() {
        let rules = vec![rule(1, "https://example.com", false)];
        // 调用方传过来的 url 可能带前后空白 —— 应该也能命中
        let r = find_rule_by_url(&rules, "  https://example.com  ").unwrap();
        assert_eq!(r.id, 1);
    }

    #[test]
    fn find_rule_by_url_returns_none_when_missing() {
        let rules = vec![rule(1, "https://a.com", false)];
        assert!(find_rule_by_url(&rules, "https://b.com").is_none());
    }

    #[test]
    fn find_rule_by_url_does_not_falsely_match_disabled() {
        // rule_key 不看 disabled —— find 只按 url 比对；disabled 由调用方判断
        let rules = vec![rule(1, "https://a.com", true)];
        assert!(find_rule_by_url(&rules, "https://a.com").is_some());
    }

    // ── parse_rules_bytes ────────────────────────────────────

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
        // parse_rules_bytes 只做内容校验；ID 留给 db::load_rules_from_path 分配
        let json = r#"[{"url": "https://a"}, {"url": "https://b"}]"#;
        let rules = parse_rules_bytes(json.as_bytes(), Path::new("a.json")).unwrap();
        for r in &rules {
            assert_eq!(r.id, 0, "parse_rules_bytes must not assign IDs");
        }
    }

    // ── load_active ──────────────────────────────────────────

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

    // ── match_source_by_url ──────────────────────────────────

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
        // rule URL 畸形（不是合法 url::Url）→ 跳过该 rule
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
        // origin 比对不含 query / fragment —— `ignores_path` 已覆盖 path，
        // 这里补全 query (`?`) 和 hash (`#`) 边界。浏览器粘贴的 URL 几乎都带 `?utm_source=...`
        // 或 `#chapter-N`，必须不影响匹配。
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
        // origin 含 port —— rule `https://a.com:8080` 不会被 `https://a.com` 命中，
        // 反之亦然（防用户粘贴端口错配的 URL 误命中）。
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

    // ── sanity: helper 不会因为空 input 炸 ───────────────────

    #[test]
    fn empty_inputs_are_safe() {
        assert_eq!(rule_key(&Rule::default()), "");
        assert_eq!(disabled_url_key(""), "");
        assert!(find_rule_by_id(&[], 0).is_none());
        assert!(find_rule_by_url(&[], "https://x").is_none());
        assert!(match_source_by_url(&[], "https://x").is_none());
        assert!(parse_rules_bytes(b"", Path::new("empty.json")).is_err());
    }
}
