//! `Source = Rule + 派生的有效抓取参数`。
//!
//! 与 Java 端的 `core.Source` 区别：Java 把 rule.crawl 字段写回了全局 `AppConfig`，
//! 是数据竞争隐患（详见审计 §6.8）。Rust 端不回写，而是派生一个独立的
//! `EffectiveCrawl` 结构体由调用方使用。

use crate::config::AppConfig;
use crate::models::Rule;

/// 由 `AppConfig` 与 `Rule.crawl` 派生的有效抓取参数。
/// 单位与 Java 端一致：interval 毫秒。
#[derive(Debug, Clone)]
pub struct EffectiveCrawl {
    pub concurrency: Option<i32>,
    pub min_interval_ms: u32,
    pub max_interval_ms: u32,
    pub max_retries: u32,
    pub retry_min_interval_ms: u32,
    pub retry_max_interval_ms: u32,
    pub enable_retry: bool,
}

impl EffectiveCrawl {
    pub fn derive(cfg: &AppConfig, rule: &Rule) -> Self {
        let mut eff = EffectiveCrawl {
            concurrency: cfg.concurrency,
            min_interval_ms: cfg.min_interval,
            max_interval_ms: cfg.max_interval,
            max_retries: cfg.max_retries,
            retry_min_interval_ms: cfg.retry_min_interval,
            retry_max_interval_ms: cfg.retry_max_interval,
            enable_retry: cfg.enable_retry,
        };

        if let Some(c) = rule.crawl.as_ref() {
            if let Some(v) = c.concurrency {
                eff.concurrency = Some(v as i32);
            }
            if let Some(v) = c.min_interval {
                eff.min_interval_ms = v;
            }
            if let Some(v) = c.max_interval {
                eff.max_interval_ms = v;
            }
            if let Some(v) = c.max_attempts {
                eff.max_retries = v;
            }
            if let Some(v) = c.retry_min_interval {
                eff.retry_min_interval_ms = v;
            }
            if let Some(v) = c.retry_max_interval {
                eff.retry_max_interval_ms = v;
            }
        }
        eff
    }
}

/// 一个书源 = 规则 + 派生的有效抓取参数。
#[derive(Debug, Clone)]
pub struct Source {
    pub rule: Rule,
    pub effective_crawl: EffectiveCrawl,
}

impl Source {
    pub fn from(rule: Rule, cfg: &AppConfig) -> Self {
        let effective_crawl = EffectiveCrawl::derive(cfg, &rule);
        Source {
            rule,
            effective_crawl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Rule, RuleCrawl};

    #[test]
    fn rule_crawl_overrides_global() {
        let cfg = AppConfig::default();
        let rule = Rule {
            url: "https://x".into(),
            crawl: Some(RuleCrawl {
                concurrency: Some(5),
                min_interval: Some(1000),
                max_interval: Some(2000),
                max_attempts: Some(7),
                retry_min_interval: None,
                retry_max_interval: None,
            }),
            ..Rule::default()
        };

        let src = Source::from(rule, &cfg);
        assert_eq!(src.effective_crawl.concurrency, Some(5));
        assert_eq!(src.effective_crawl.min_interval_ms, 1000);
        assert_eq!(src.effective_crawl.max_interval_ms, 2000);
        assert_eq!(src.effective_crawl.max_retries, 7);
        // 未覆盖的字段保留全局值
        assert_eq!(
            src.effective_crawl.retry_min_interval_ms,
            cfg.retry_min_interval
        );
    }

    #[test]
    fn no_rule_crawl_uses_global() {
        let cfg = AppConfig::default();
        let rule = Rule {
            url: "https://x".into(),
            ..Rule::default()
        };
        let src = Source::from(rule, &cfg);
        assert_eq!(src.effective_crawl.min_interval_ms, cfg.min_interval);
        assert_eq!(src.effective_crawl.max_interval_ms, cfg.max_interval);
    }
}
