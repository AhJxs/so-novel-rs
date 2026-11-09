//! 书源规则。对应 Java `model.Rule` 及其内部静态类。
//!
//! 字段名沿用规则文件原有的驼峰命名 (`bookName` 等), 靠 `#[serde(rename_all = "camelCase")]` 与
//! `assets/rules/*.json` 兼容。Java 端 `Rule.Book` 既当"详情规则"又当"详情数据", Rust 端拆开:
//! `RuleBook` 仅是规则, `crate::models::book::Book` 是数据。
//!
//! 所有 bool 字段统一走 `lenient_bool`: Java/hutool 容忍字符串 (`no-search.json` 里真实存在
//! `"paragraphTagClosed": "true"`), 而 serde 严格, 不宽松会直接解析失败。

use serde::{Deserialize, Deserializer, Serialize};

/// 宽松反序列化布尔：接受 bool 字面量或字符串 `"true"/"false"/"1"/"0"`。
fn lenient_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    use std::fmt;

    use serde::de::{self, Visitor};

    struct V;
    impl Visitor<'_> for V {
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

/// 单个书源规则的根对象：站点入口地址加上各页面的解析规则。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    /// 自增 ID，由加载器在加载时填充（与 Java 端一致：从 1 开始）。
    #[serde(default)]
    pub id: i32,

    /// 书源首页地址；其它相对 URL 以其为基准拼接。
    pub url: String,
    /// 书源显示名，用于搜索页下拉与日志。
    #[serde(default)]
    pub name: String,
    /// 备注，通常是来源说明或维护者标记。
    #[serde(default)]
    pub comment: String,
    /// 书源正文语言标记（如 `"zh"`），加载时据此筛选。
    #[serde(default)]
    pub language: String,
    /// 请求是否需走代理，沿用 Java 端的兼容字段。
    #[serde(default, deserialize_with = "lenient_bool")]
    pub need_proxy: bool,
    /// 整个书源是否停用；停用后既不出现在下拉，也不参与搜索。
    #[serde(default, deserialize_with = "lenient_bool")]
    pub disabled: bool,
    /// `rate-limit.json` 中 0xs 书源出现, 旧 Java 模型未声明但 JSON 中存在, 保留以免丢字段。
    #[serde(default, deserialize_with = "lenient_bool")]
    pub ignore_ssl: bool,

    /// 搜索页规则；`None` 表示该书源不支持搜索。
    pub search: Option<RuleSearch>,
    /// 详情页规则；`None` 表示无法抓取书籍元信息。
    pub book: Option<RuleBook>,
    /// 目录页规则。
    pub toc: Option<RuleToc>,
    /// 章节正文规则。
    pub chapter: Option<RuleChapter>,
    /// 抓取节奏覆盖（并发 / 间隔 / 重试），缺省时用全局配置。
    pub crawl: Option<RuleCrawl>,
}

impl Rule {
    /// 此书源能否加入聚合搜索。判定与 `app::ops::search::spawn_search` 派发时完全一致: 顶
    /// `Rule.disabled` 为 false 且 `RuleSearch.disabled` 也为 false (`RuleSearch` 不存在视为 false)。
    /// 搜索页书源下拉和派发共用此谓词, 避免"下拉里有但实际不发请求"的不一致。
    pub fn is_search_enabled(&self) -> bool {
        !self.disabled && self.search.as_ref().is_some_and(|s| !s.disabled)
    }
}

/// 搜索页规则：请求模板与结果列表的字段选择器。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSearch {
    /// 是否纳入聚合搜索（true 表示禁用此书源的搜索）。
    #[serde(default, deserialize_with = "lenient_bool")]
    pub disabled: bool,
    /// 拼接相对 URL 的基址；为空时回落到 `Rule.url`。
    #[serde(default)]
    pub base_uri: String,
    /// 单次请求超时（毫秒）；`None` 用全局配置。
    pub timeout: Option<u32>,
    /// 搜索请求的 URL 模板，可含关键词与页码占位符。
    #[serde(default)]
    pub url: String,
    /// HTTP 方法，`GET` 或 `POST`。
    #[serde(default)]
    pub method: String,
    /// POST 请求体模板。
    #[serde(default)]
    pub data: String,
    /// 附加到请求的 Cookie 串。
    #[serde(default)]
    pub cookies: String,
    /// 结果列表容器的 CSS 选择器。
    #[serde(default)]
    pub result: String,
    /// 结果项内书名的选择器。
    #[serde(default)]
    pub book_name: String,
    /// 结果项内作者的选择器。
    #[serde(default)]
    pub author: String,
    /// 结果项内分类的选择器。
    #[serde(default)]
    pub category: String,
    /// 结果项内最新章节名的选择器。
    #[serde(default)]
    pub latest_chapter: String,
    /// 结果项内最后更新时间的选择器。
    #[serde(default)]
    pub last_update_time: String,
    /// 结果项内连载状态的选择器。
    #[serde(default)]
    pub status: String,
    /// 结果项内字数的选择器。
    #[serde(default)]
    pub word_count: String,
    /// 翻页搜索时下一页链接的选择器。
    #[serde(default)]
    pub next_page: String,
    /// 自定义 Referer 头。非空时覆盖默认的 origin Referer。
    #[serde(default)]
    pub referer: String,
}

/// 详情页规则：书籍元信息各字段的选择器。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleBook {
    /// 拼接相对 URL 的基址；为空时回落到 `Rule.url`。
    #[serde(default)]
    pub base_uri: String,
    /// 单次请求超时（毫秒）；`None` 用全局配置。
    pub timeout: Option<u32>,
    /// 详情页 URL 正则（含一个捕获组用于提取书 ID）。
    #[serde(default)]
    pub url: String,
    /// 书名选择器。
    #[serde(default)]
    pub book_name: String,
    /// 作者选择器。
    #[serde(default)]
    pub author: String,
    /// 简介选择器。
    #[serde(default)]
    pub intro: String,
    /// 分类选择器。
    #[serde(default)]
    pub category: String,
    /// 封面图地址选择器。
    #[serde(default)]
    pub cover_url: String,
    /// 最新章节名选择器。
    #[serde(default)]
    pub latest_chapter: String,
    /// 最新章节链接选择器。
    #[serde(default)]
    pub latest_chapter_url: String,
    /// 最后更新时间选择器。
    #[serde(default)]
    pub last_update_time: String,
    /// 连载状态选择器。
    #[serde(default)]
    pub status: String,
}

/// 目录页规则：章节列表的容器与条目选择器。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleToc {
    /// 拼接相对 URL 的基址；为空时回落到 `Rule.url`。
    #[serde(default)]
    pub base_uri: String,
    /// 单次请求超时（毫秒）；`None` 用全局配置。
    pub timeout: Option<u32>,
    /// 目录页 URL，可为模板或正则。
    #[serde(default)]
    pub url: String,
    /// 章节列表容器的 CSS 选择器。
    #[serde(default)]
    pub list: String,
    /// 列表内单个章节项的选择器。
    #[serde(default)]
    pub item: String,
    /// 是否倒序展示。JSON 中字段名是 `isDesc`, 经 `#[serde(rename)]` 映射到本字段。
    #[serde(rename = "isDesc", default, deserialize_with = "lenient_bool")]
    pub is_desc: bool,
    /// 目录分页的下一页链接选择器。
    #[serde(default)]
    pub next_page: String,
}

/// 章节正文规则：标题、正文及分页/过滤处理方式。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleChapter {
    /// 拼接相对 URL 的基址；为空时回落到 `Rule.url`。
    #[serde(default)]
    pub base_uri: String,
    /// 单次请求超时（毫秒）；`None` 用全局配置。
    pub timeout: Option<u32>,
    /// 章节标题选择器。
    #[serde(default)]
    pub title: String,
    /// 正文容器选择器。
    #[serde(default)]
    pub content: String,
    /// 正文是否已自带段落标签；为 false 时才按 `paragraph_tag` 分区。
    #[serde(default, deserialize_with = "lenient_bool")]
    pub paragraph_tag_closed: bool,
    /// 生成段落时使用的标签名（如 `p`）。
    #[serde(default)]
    pub paragraph_tag: String,
    /// 需从正文剔除的文本正则。
    #[serde(default)]
    pub filter_txt: String,
    /// 需从正文剔除的标签选择器。
    #[serde(default)]
    pub filter_tag: String,
    /// 章节内分页的下一页链接选择器。
    #[serde(default)]
    pub next_page: String,
    /// 以 JS 表达式计算下一页地址的规则。
    #[serde(default)]
    pub next_page_in_js: String,
    /// 下一章链接选择器。
    #[serde(default)]
    pub next_chapter_link: String,
}

/// 单书源对全局抓取节奏的覆盖值；`None` 表示沿用全局配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleCrawl {
    /// 并发下载数。
    pub concurrency: Option<u32>,
    /// 相邻请求的最小间隔（毫秒）。
    pub min_interval: Option<u32>,
    /// 相邻请求的最大间隔（毫秒）。
    pub max_interval: Option<u32>,
    /// 单章最大尝试次数。
    pub max_attempts: Option<u32>,
    /// 重试起始间隔（毫秒）。
    pub retry_min_interval: Option<u32>,
    /// 重试间隔上限（毫秒）。
    pub retry_max_interval: Option<u32>,
}

// ---------- EffectiveCrawl + Source (Rule 的派生视图) ----------

use crate::config::AppConfig;

/// 由 `AppConfig` 与 `Rule.crawl` 派生的有效抓取参数; interval 单位毫秒 (与 Java 端一致)。
#[derive(Debug, Clone)]
pub struct EffectiveCrawl {
    /// 并发下载数；`None` 表示按 CPU 核数自动决定。
    pub concurrency: Option<i32>,
    /// 相邻请求的最小间隔（毫秒）。
    pub min_interval_ms: u32,
    /// 相邻请求的最大间隔（毫秒）。
    pub max_interval_ms: u32,
    /// 单章最大尝试次数。
    pub max_retries: u32,
    /// 重试起始间隔（毫秒）。
    pub retry_min_interval_ms: u32,
    /// 重试间隔上限（毫秒）。
    pub retry_max_interval_ms: u32,
    /// 是否启用失败重试。
    pub enable_retry: bool,
}

impl EffectiveCrawl {
    /// 以全局 `crawl` 配置为底，用书源自身的 `Rule.crawl` 覆盖非 `None` 字段。
    pub const fn derive(cfg: &AppConfig, rule: &Rule) -> Self {
        let mut eff = Self {
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
                // const fn 里不能用 `i32::try_from` (尚非 const trait), 只能 `u32 → i64 → i32`;
                // i64 截断成 i32 命中已全局 allow 的 `cast_possible_truncation`。
                eff.concurrency = Some((v as i64) as i32);
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
    /// 原始书源规则。
    pub rule: Rule,
    /// 规则与全局配置合并后的抓取参数。
    pub effective_crawl: EffectiveCrawl,
}

impl Source {
    /// 用规则与全局配置构造书源，并即时算出有效抓取参数。
    pub const fn from(rule: Rule, cfg: &AppConfig) -> Self {
        let effective_crawl = EffectiveCrawl::derive(cfg, &rule);
        Self {
            rule,
            effective_crawl,
        }
    }
}

#[cfg(test)]
mod source_tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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
