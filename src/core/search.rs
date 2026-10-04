//! CLI / desktop 共用的"搜索前准备"逻辑：选书源 / 算 `cf_bypass` / 算 limit。
//!
//! 之前 cli / desktop 两处近乎字面量重复同一套实现，抽出来后调用方收敛为
//! `select_sources(&rules, &cfg, source_id)` + `cf_bypass(&cfg)` + `effective_limit(limit, &cfg)`。
//!
//! `select_sources` 以 `&[Rule]` 入参并要求传入**完整**规则列表（desktop 直接传全量，
//! cli 自己先 `filter(!disabled)`），内部再判断 `is_search_enabled`；这样 core 不需要知道
//! "调用方有没有预过滤"的歧义。

use crate::config::AppConfig;
use crate::models::{Rule, Source};

pub use super::config_helpers::cf_bypass;

/// 选书源。
///
/// `source_id = Some(id)` → 精确按 id 找且**不**检查 disabled（上层决定报"已禁用"还是 404，找不到返回空 vec）；
/// `None` → 按 [`Rule::is_search_enabled`] 过滤后转 [`Source`]（`Source::from` 内部 derive EffectiveCrawl，故传全量 `cfg`）。
pub fn select_sources(rules: &[Rule], cfg: &AppConfig, source_id: Option<i32>) -> Vec<Source> {
    source_id.map_or_else(
        || {
            rules
                .iter()
                .filter(|r| r.is_search_enabled())
                .cloned()
                .map(|r| Source::from(r, cfg))
                .collect()
        },
        |id| {
            rules
                .iter()
                .find(|r| r.id == id)
                .cloned()
                .map(|r| Source::from(r, cfg))
                .into_iter()
                .collect()
        },
    )
}

/// 计算最终搜索结果上限，优先级：显式 `explicit`（caller 已做过 `max(0)` + `>0` 校验）→
/// `cfg.source.search_limit`（`Option<i32>`，≤0 视作未设）→ `None`（调用方兜底：书源自带 / 不限）。
///
/// query param 的校验属于各前端自己的输入层，不在此处做。
pub fn effective_limit(explicit: Option<usize>, cfg: &AppConfig) -> Option<usize> {
    explicit.or_else(|| {
        cfg.source
            .search_limit
            .map(|v| v.max(0) as usize)
            .filter(|v| *v > 0)
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::models::{Rule, RuleSearch};

    fn rule(id: i32, disabled: bool, search_disabled: bool) -> Rule {
        Rule {
            id,
            url: format!("https://example-{id}.com"),
            name: format!("src-{id}"),
            disabled,
            search: Some(RuleSearch {
                disabled: search_disabled,
                ..RuleSearch::default()
            }),
            ..Rule::default()
        }
    }

    fn cfg_no_limit() -> AppConfig {
        AppConfig::default()
    }

    fn cfg_with_limit(n: i32) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.source.search_limit = Some(n);
        cfg
    }

    #[test]
    fn select_sources_none_returns_all_enabled() {
        let rules = vec![
            rule(1, false, false), // enabled
            rule(2, true, false),  // top-level disabled → not enabled
            rule(3, false, true),  // search.disabled → not enabled
            rule(4, false, false), // enabled
        ];
        let cfg = cfg_no_limit();
        let sources = select_sources(&rules, &cfg, None);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].rule.id, 1);
        assert_eq!(sources[1].rule.id, 4);
    }

    #[test]
    fn select_sources_some_id_filters_by_id() {
        let rules = vec![
            rule(1, false, false),
            rule(2, false, false),
            rule(3, false, false),
        ];
        let cfg = cfg_no_limit();
        let sources = select_sources(&rules, &cfg, Some(2));
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].rule.id, 2);
    }

    #[test]
    fn select_sources_some_id_returns_disabled_too() {
        // 用户显式传 id 时不管 disabled，调用方决定怎么处理（4xx 还是 warn）
        let rules = vec![rule(1, true, true)];
        let cfg = cfg_no_limit();
        let sources = select_sources(&rules, &cfg, Some(1));
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn select_sources_some_id_missing_returns_empty_vec() {
        let rules = vec![rule(1, false, false)];
        let cfg = cfg_no_limit();
        let sources = select_sources(&rules, &cfg, Some(99));
        assert!(sources.is_empty());
    }

    #[test]
    fn select_sources_empty_rules_returns_empty_vec() {
        let rules: Vec<Rule> = vec![];
        let cfg = cfg_no_limit();
        assert!(select_sources(&rules, &cfg, None).is_empty());
        assert!(select_sources(&rules, &cfg, Some(1)).is_empty());
    }

    #[test]
    fn select_sources_all_disabled_returns_empty() {
        let rules = vec![
            rule(1, true, true),
            rule(2, true, false),
            rule(3, false, true),
        ];
        let cfg = cfg_no_limit();
        assert!(select_sources(&rules, &cfg, None).is_empty());
    }

    #[test]
    fn effective_limit_explicit_takes_priority() {
        let cfg = cfg_with_limit(10);
        assert_eq!(effective_limit(Some(99), &cfg), Some(99));
    }

    #[test]
    fn effective_limit_explicit_zero_still_wins_when_present() {
        // 显式 Some(0) 原样返回（不做归一）—— caller 已自己 filter。
        let cfg = cfg_no_limit();
        assert_eq!(effective_limit(Some(0), &cfg), Some(0));
    }

    #[test]
    fn effective_limit_falls_back_to_config() {
        let cfg = cfg_with_limit(20);
        assert_eq!(effective_limit(None, &cfg), Some(20));
    }

    #[test]
    fn effective_limit_config_zero_treated_as_unset() {
        assert_eq!(effective_limit(None, &cfg_with_limit(0)), None);
        assert_eq!(effective_limit(None, &cfg_with_limit(-5)), None);
    }

    #[test]
    fn effective_limit_no_config_no_explicit_returns_none() {
        let cfg = cfg_no_limit();
        assert_eq!(effective_limit(None, &cfg), None);
    }

    #[test]
    fn effective_limit_explicit_overrides_zero_config() {
        let cfg = cfg_with_limit(0);
        assert_eq!(effective_limit(Some(50), &cfg), Some(50));
    }
}
