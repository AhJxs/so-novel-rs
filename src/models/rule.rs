//! 书源规则。对应 Java `model.Rule` 及其内部静态类。
//!
//! 字段名沿用规则文件原有的驼峰命名（`bookName`、`lastUpdateTime` 等），
//! 通过 `#[serde(rename_all = "camelCase")]` 与现有 `bundle/rules/*.json` 兼容。
//!
//! 注意 Java 端 `Rule.Book` 既被用作"详情规则"也被用作"详情数据"。Rust 端
//! 拆分：本文件中的 `RuleBook` 仅是规则；`crate::models::book::Book` 是数据。
//!
//! Java/hutool 反序列化布尔时容忍字符串（`"paragraphTagClosed": "true"` 在
//! `bundle/rules/no-search.json` 中真实存在）。Rust serde 严格，所以本模块
//! 为所有 bool 字段统一用 `lenient_bool` 的 deserialize_with，接受 `true/false`
//! 与 `"true"/"false"/"1"/"0"`。

use serde::{Deserialize, Deserializer, Serialize};

/// 宽松反序列化布尔：接受 bool 字面量或字符串 `"true"/"false"/"1"/"0"`。
fn lenient_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    use serde::de::{self, Visitor};
    use std::fmt;

    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = bool;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("bool 或 \"true\"/\"false\"/\"1\"/\"0\"")
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<bool, E> {
            Ok(v)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<bool, E> {
            match v.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" | "on" => Ok(true),
                "false" | "0" | "no" | "off" | "" => Ok(false),
                _ => Err(de::Error::custom(format!("不能解析为 bool: {v:?}"))),
            }
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<bool, E> {
            self.visit_str(&v)
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<bool, E> {
            Ok(v != 0)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<bool, E> {
            Ok(v != 0)
        }
    }

    d.deserialize_any(V)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// 自增 ID，由加载器在加载时填充（与 Java 端一致：从 1 开始）。
    #[serde(default)]
    pub id: i32,

    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub language: String,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub need_proxy: bool,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub disabled: bool,
    /// rate-limit.json 中 0xs 书源出现，旧 Java 模型未声明但 JSON 中存在。
    /// 保留字段以避免反序列化丢失信息。
    #[serde(default, deserialize_with = "lenient_bool")]
    pub ignore_ssl: bool,

    pub search: Option<RuleSearch>,
    pub book: Option<RuleBook>,
    pub toc: Option<RuleToc>,
    pub chapter: Option<RuleChapter>,
    pub crawl: Option<RuleCrawl>,
}

impl Rule {
    /// 此书源能否加入聚合搜索。
    ///
    /// 判定与 `app::ops::search::spawn_search` 派发时完全一致：顶 `Rule.disabled` 为
    /// false 且 `RuleSearch.disabled` 也为 false（`RuleSearch` 不存在视为 false）。
    /// 搜索页书源下拉和派发共用此谓词，避免下拉里出现但实际不发请求的不一致。
    pub fn is_search_enabled(&self) -> bool {
        !self.disabled && self.search.as_ref().map(|s| !s.disabled).unwrap_or(false)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSearch {
    /// 是否纳入聚合搜索（true 表示禁用此书源的搜索）。
    #[serde(default, deserialize_with = "lenient_bool")]
    pub disabled: bool,
    #[serde(default)]
    pub base_uri: String,
    pub timeout: Option<u32>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub cookies: String,
    #[serde(default)]
    pub result: String,
    #[serde(default)]
    pub book_name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub latest_chapter: String,
    #[serde(default)]
    pub last_update_time: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub word_count: String,
    #[serde(default)]
    pub next_page: String,
    /// 自定义 Referer 头。非空时覆盖默认的 origin Referer。
    #[serde(default)]
    pub referer: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleBook {
    #[serde(default)]
    pub base_uri: String,
    pub timeout: Option<u32>,
    /// 详情页 URL 正则（含一个捕获组用于提取书 ID）。
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub book_name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub intro: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub cover_url: String,
    #[serde(default)]
    pub latest_chapter: String,
    #[serde(default)]
    pub latest_chapter_url: String,
    #[serde(default)]
    pub last_update_time: String,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleToc {
    #[serde(default)]
    pub base_uri: String,
    pub timeout: Option<u32>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub list: String,
    #[serde(default)]
    pub item: String,
    /// 是否倒序展示。注意 JSON 中字段名是 `isDesc`，
    /// 经 camelCase 反序列化后映射到本字段。
    #[serde(rename = "isDesc", default, deserialize_with = "lenient_bool")]
    pub is_desc: bool,
    #[serde(default)]
    pub next_page: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleChapter {
    #[serde(default)]
    pub base_uri: String,
    pub timeout: Option<u32>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default, deserialize_with = "lenient_bool")]
    pub paragraph_tag_closed: bool,
    #[serde(default)]
    pub paragraph_tag: String,
    #[serde(default)]
    pub filter_txt: String,
    #[serde(default)]
    pub filter_tag: String,
    #[serde(default)]
    pub next_page: String,
    #[serde(default)]
    pub next_page_in_js: String,
    #[serde(default)]
    pub next_chapter_link: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCrawl {
    pub concurrency: Option<u32>,
    pub min_interval: Option<u32>,
    pub max_interval: Option<u32>,
    pub max_attempts: Option<u32>,
    pub retry_min_interval: Option<u32>,
    pub retry_max_interval: Option<u32>,
}

// ---------- EffectiveCrawl + Source ----------
// 原本在 `crate::rules::source`，合并到这里是因为它本质上只是 `Rule` 的
// "派生视图" —— 跟 Rule 同模型层最自然，跨模块再 import 一层显得啰嗦。

use crate::config::{AppConfig, CookieCfg, CrawlCfg, DownloadCfg, GlobalCfg, ProxyCfg, SourceCfg};

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
            concurrency: cfg.crawl.concurrency,
            min_interval_ms: cfg.crawl.min_interval,
            max_interval_ms: cfg.crawl.max_interval,
            max_retries: cfg.crawl.max_retries,
            retry_min_interval_ms: cfg.crawl.retry_min_interval,
            retry_max_interval_ms: cfg.crawl.retry_max_interval,
            enable_retry: cfg.crawl.enable_retry,
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
mod source_tests {
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
            cfg.crawl.retry_min_interval
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
        assert_eq!(src.effective_crawl.min_interval_ms, cfg.crawl.min_interval);
        assert_eq!(src.effective_crawl.max_interval_ms, cfg.crawl.max_interval);
    }
}
