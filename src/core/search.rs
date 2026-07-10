//! 三端共用的"搜索前准备"逻辑：选书源 / 算 `cf_bypass` / 算 limit。
//!
//! 之前 cli / web / desktop 三处近乎字面量重复同一套：
//!
//! ```ignore
//! // cli/search.rs:29-52
//! // web/handlers/search.rs:80-100
//! // desktop/model/ops/search.rs:42-91
//! ```
//!
//! 抽出后调用方收敛为单行：
//!
//! ```ignore
//! let sources = core::search::select_sources(&rules, &cfg, source_id);
//! let cf = core::config_helpers::cf_bypass(&cfg);
//! let limit = core::search::effective_limit(params_limit, &cfg);
//! ```
//!
//! ## 关于 cli 跟 web/desktop 的入参差异
//!
//! - cli 之前拿的是 `Vec<Source>`（`cli/util::load_active_sources` 已过滤 `!disabled`）
//! - web / desktop 拿的是原始 `Vec<Rule>`，自己判断 `is_search_enabled`
//!
//! `select_sources` 统一以 `&[Rule]` 入参 + 内部判断，**要求**调用方传入完整规则列表。
//! cli 调用前自己先 `filter(!disabled)`；web / desktop 直接传全量。
//! 这避免了 core 模块需要知道"调用方有没有预过滤"的歧义。

use crate::config::AppConfig;
use crate::models::{Rule, Source};

pub use super::config_helpers::cf_bypass;

/// 选书源。
///
/// - `source_id = Some(id)`：精确按 id 找（**不**检查 disabled —— 用户显式传 id 时
///   总是返回它对应的 Source，让上层决定要不要报 "该书源已禁用" 错）。
///   找不到时返回空 vec，调用方决定是 404 还是 warn。
/// - `source_id = None`：按 [`Rule::is_search_enabled`] 过滤 → 转 [`Source`]。
///   `Source::from(rule, cfg)` 内部 derive EffectiveCrawl，所以传入全量 `cfg`。
///
/// ## 入参约定
///
/// 调用方传入**完整**的 `&[Rule]`（不过滤）。cli 之前用 `Vec<Source>` 是因为
/// `cli/util::load_active_sources` 已做了 `!r.disabled` 过滤 —— 那是 cli 启动期的快捷路径。
/// 现在 cli 也改传 `&[Rule]`，内部一致。
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

/// 计算最终搜索结果上限。
///
/// 优先级：
/// 1. 调用方显式传入的 `explicit`（**已经**做过 `max(0)` + `> 0` 校验的 `Option<usize>`）
/// 2. `cfg.source.search_limit`（`Option<i32>`，≤0 视作未设）
/// 3. `None` —— 由调用方决定兜底（书源自带 / 不限）
///
/// **web 那一道** `params.limit.map(|v| v.max(0) as usize).filter(|v| *v > 0)` 仍在
/// web handler 里做 —— 那是 query param 校验（HTTP 层），不属于 core。web 把结果
/// 传进来时已经保证 `Some(>0)` 或 `None`。
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

    // ── select_sources ───────────────────────────────────────

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

    // ── effective_limit ──────────────────────────────────────

    #[test]
    fn effective_limit_explicit_takes_priority() {
        let cfg = cfg_with_limit(10);
        assert_eq!(effective_limit(Some(99), &cfg), Some(99));
    }

    #[test]
    fn effective_limit_explicit_zero_still_wins_when_present() {
        // 显式 Some(0) 视作 None（caller 已经做过 filter；这里仅是 fallback 链的副带验证）
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
        // cfg.source.search_limit = 0/负数 → 视作 None
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
        // 显式传 50，config 是 0：explicit 优先
        let cfg = cfg_with_limit(0);
        assert_eq!(effective_limit(Some(50), &cfg), Some(50));
    }
}
