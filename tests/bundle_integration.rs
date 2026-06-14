//! 集成测试：以仓库根 `bundle/` 真实文件为输入，验证 config + rules 端到端。

use std::path::PathBuf;

use so_novel_rs::config::load_config;
use so_novel_rs::rules::load_rules_from_path;

fn bundle_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("bundle")
}

#[test]
fn config_and_rules_load_together() {
    let cfg_path = bundle_dir().join("config.ini");
    let rules_dir = bundle_dir().join("rules");
    assert!(cfg_path.exists(), "missing {}", cfg_path.display());
    assert!(rules_dir.exists(), "missing {}", rules_dir.display());

    let cfg = load_config(&cfg_path).expect("config loads");
    assert_eq!(cfg.active_rules, "main.json");

    // 用 cfg.active_rules 拼到 rules_dir，加载与 UI 启动一致的规则集。
    let active = rules_dir.join(&cfg.active_rules);
    let rules = load_rules_from_path(&active).expect("main.json loads");
    assert!(!rules.is_empty());

    // 找到第一个有 search 规则的书源。
    let with_search = rules
        .iter()
        .find(|r| r.search.is_some())
        .expect("at least one searchable rule");
    let s = with_search.search.as_ref().unwrap();
    assert!(!s.url.is_empty());
}

#[test]
fn loads_every_real_rule_file_individually() {
    // 单独加载每个规则文件都不应出错（除模板）。
    let rules_dir = bundle_dir().join("rules");
    let names = [
        "main.json",
        "proxy-required.json",
        "rate-limit.json",
        "no-search.json",
        "cloudflare.json",
    ];
    for n in names {
        let p = rules_dir.join(n);
        assert!(p.exists(), "missing {}", p.display());
        let rs = load_rules_from_path(&p).unwrap_or_else(|e| panic!("load {n} failed: {e}"));
        assert!(!rs.is_empty(), "{n} produced 0 rules");
    }
}

#[test]
fn rule_template_is_skipped_by_directory_walk() {
    // 模板单文件本身不是 [Rule] 数组，单独加载会出错——这与 Java 端
    // 只通过 readJSONArray 加载规则一致：模板从不被当成生产规则。
    let p = bundle_dir().join("rules").join("rule-template.json5");
    assert!(p.exists(), "missing {}", p.display());
    let res = load_rules_from_path(&p);
    assert!(res.is_err(), "rule-template should not parse as Vec<Rule>");

    // 同时确认目录加载会跳过模板：rules/ 中有 27 条左右真实书源，
    // 但 rule-template.json5 不应贡献条目。
    let dir = bundle_dir().join("rules");
    let all = load_rules_from_path(&dir).unwrap();
    assert!(
        all.iter().all(|r| !r.url.is_empty()),
        "no rule should have empty url after default-skip"
    );
}
