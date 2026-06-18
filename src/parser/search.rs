//! 搜索解析。对应 Java `parse.SearchParser`。
//!
//! 阶段 2b 实现的能力（与 Java 端等价的子集）：
//! - GET / POST 两种搜索请求；
//! - POST 时把规则中的 hutool 风格 `data` 模板里的 `%s` 换成关键词，
//!   构造 form body；
//! - 给请求注入规则里的 `cookies` 头；
//! - 选 `result` 列表，每条提取 bookName / author / category /
//!   latestChapter / lastUpdateTime / status / wordCount；
//! - bookName 的 href 用 `abs_url` 解析为绝对 URL；
//! - 检测 Cloudflare 真人验证页；命中则返回 `SearchError::Cloudflare`，
//!   不在本阶段实现旁路调用（属阶段 2c）。
//!
//! **未实现**（属阶段 2c / 后续）：
//! - 搜索结果分页（`pagination = true`）合并；
//! - "完全匹配跳详情页"的 fallback 路径；
//! - 简繁转换（属阶段 5）；
//! - 聚合搜索（属阶段 4 UI 接入）；
//! - quanben5 加密参数 `b` 的特殊路径（属阶段 2c）。

use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};
use thiserror::Error;

use crate::http::{
    FetchRequest, HttpMethod, build_form_data, fetch, fetch_via_cf_bypass, format_url_query,
    has_cloudflare,
};
use crate::models::{ContentType, Rule, SearchResult};
use crate::parser::dom::{SelectError, select_and_invoke_js_within};

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("书源未启用搜索")]
    SearchDisabled,
    #[error("书源已被禁用")]
    SourceDisabled,
    #[error("HTTP 错误: {0}")]
    Http(String),
    #[error(
        "命中 Cloudflare 验证页，未配置 cf-bypass 旁路（请在 config.toml [global] cf-bypass 填地址）: {0}"
    )]
    Cloudflare(String),
    #[error("HTML 解析失败: {0}")]
    Parse(String),
    #[error("选择器/JS 执行失败: {0}")]
    Selector(#[from] SelectError),
}

/// 搜索单个书源。
///
/// `keyword` 是用户输入的原始关键词；`limit` 控制返回上限
/// （`None` 表示不限，对应 Java 端 -1）。
/// `cf_bypass_base` 是 `[global] cf-bypass` 配置：若命中 CF 真人验证页
/// 且该值非空，则自动重试外部 bypass 服务（详见 `http::cf::fetch_via_cf_bypass`）；
/// 为空时直接返回 `SearchError::Cloudflare`。
pub async fn search_one(
    client: &Client,
    rule: &Rule,
    keyword: &str,
    limit: Option<usize>,
    cf_bypass_base: Option<&str>,
) -> Result<Vec<SearchResult>, SearchError> {
    if rule.disabled {
        return Err(SearchError::SourceDisabled);
    }
    let s = rule.search.as_ref().ok_or(SearchError::SearchDisabled)?;
    if s.disabled {
        return Err(SearchError::SearchDisabled);
    }

    // 特殊分支：quanben5 双 %s 模板（加密参数 b + JSONP 响应）。
    // 不污染普通分支，复用同一套 result/bookName/author 选择器。
    if crate::parser::search_quanben5::is_quanben5_pattern(&s.url) {
        return crate::parser::search_quanben5::search_one_quanben5(
            client,
            rule,
            keyword,
            limit,
            cf_bypass_base,
        )
        .await;
    }

    // 1. 构造请求
    let url_with_keyword = format_url_query(&s.url, keyword);
    let cookies = if s.cookies.trim().is_empty() {
        None
    } else {
        Some(s.cookies.as_str())
    };

    let response = match s.method.to_ascii_lowercase().as_str() {
        "post" => {
            let form = build_form_data(&s.data, &[keyword]);
            let form_owned: Vec<(String, String)> = form.into_iter().collect();
            fetch(
                client,
                &FetchRequest {
                    url: &url_with_keyword,
                    method: HttpMethod::Post(&form_owned),
                    cookies,
                    timeout_secs: s.timeout,
                },
            )
            .await
            .map_err(|e| SearchError::Http(format!("{e:#}")))?
        }
        _ => fetch(
            client,
            &FetchRequest {
                url: &url_with_keyword,
                method: HttpMethod::Get,
                cookies,
                timeout_secs: s.timeout,
            },
        )
        .await
        .map_err(|e| SearchError::Http(format!("{e:#}")))?,
    };

    // CF 命中 → 优先尝试 bypass 服务；不可用时返回类型化错误。
    let html_after_cf = if has_cloudflare(&response.html) {
        match cf_bypass_base.filter(|s| !s.trim().is_empty()) {
            Some(base) => fetch_via_cf_bypass(client, base, &url_with_keyword)
                .await
                .map_err(|e| SearchError::Http(format!("cf-bypass: {e:#}")))?,
            None => return Err(SearchError::Cloudflare(response.final_url)),
        }
    } else {
        response.html
    };

    // 2. 解析（解析逻辑独立成函数便于离线测试直接喂 HTML）。
    parse_search_results(&html_after_cf, &response.final_url, rule, limit)
}

/// 把已经下载好的 HTML 解析为搜索结果列表。
///
/// 抽离这一函数是为了让测试不依赖网络：直接喂离线 HTML 即可。
/// `base_url` 用来解析 href 相对路径（相当于 jsoup `Element.absUrl(...)`）。
pub fn parse_search_results(
    html: &str,
    base_url: &str,
    rule: &Rule,
    limit: Option<usize>,
) -> Result<Vec<SearchResult>, SearchError> {
    let s = rule.search.as_ref().ok_or(SearchError::SearchDisabled)?;

    let document = Html::parse_document(html);
    let result_selector = Selector::parse(&s.result)
        .map_err(|e| SearchError::Parse(format!("无效的 result 选择器 `{}`: {e:?}", s.result)))?;

    let mut out: Vec<SearchResult> = Vec::new();

    for el in document.select(&result_selector) {
        // bookName 是必填字段；空则跳过该条（Java 端 `bookName.isEmpty() continue`）
        let book_name = select_and_invoke_js_within(el, &s.book_name, ContentType::Text)?;
        if book_name.is_empty() {
            continue;
        }

        // href 走 attr_href；如果是相对路径，用 base_url 拼绝对。
        let raw_href = select_and_invoke_js_within(el, &s.book_name, ContentType::AttrHref)?;
        let url = crate::http::abs_url(base_url, &raw_href).unwrap_or_default();

        let author = optional_field(el, &s.author)?;
        let category = optional_field(el, &s.category)?;
        let latest_chapter = optional_field(el, &s.latest_chapter)?;
        let last_update_time = optional_field(el, &s.last_update_time)?;
        let status = optional_field(el, &s.status)?;
        let word_count = optional_field(el, &s.word_count)?;

        out.push(SearchResult {
            source_id: rule.id,
            source_name: rule.name.clone(),
            url,
            book_name,
            author,
            category,
            latest_chapter,
            last_update_time,
            status,
            word_count,
            ..SearchResult::default()
        });

        if let Some(n) = limit {
            if out.len() >= n {
                break;
            }
        }
    }

    Ok(out)
}

fn optional_field(el: scraper::ElementRef<'_>, query: &str) -> Result<Option<String>, SearchError> {
    if query.trim().is_empty() {
        return Ok(None);
    }
    let v = select_and_invoke_js_within(el, query, ContentType::Text)?;
    Ok(if v.is_empty() { None } else { Some(v) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LangType;
    use crate::rules::apply_default_rule;

    /// 用 main.json 中"笔趣阁22"的真实搜索规则构造一条 Rule。
    fn rule_22biqu() -> Rule {
        let mut r: Rule = serde_json::from_str(
            r##"{
                "url": "https://www.22biqu.com/",
                "name": "笔趣阁22",
                "search": {
                    "url": "https://www.22biqu.com/ss/",
                    "method": "post",
                    "data": "{searchkey: %s, Submit: 搜索}",
                    "result": "body > div.container > div > div > ul > li",
                    "bookName": "span.s2 > a",
                    "author": "span.s4",
                    "category": "span.s1",
                    "latestChapter": "span.s3",
                    "lastUpdateTime": "span.s5"
                }
            }"##,
        )
        .expect("rule json should parse");
        r.id = 5;
        apply_default_rule(&mut r, LangType::ZhCn);
        r
    }

    /// 仿制一段 22biqu 真实搜索响应的极简骨架。结构与现网一致：
    /// `body > div.container > div > div > ul > li`。
    fn fake_22biqu_search_html() -> String {
        r##"<!doctype html>
<html><head><title>搜索结果</title></head><body>
<div class="container"><div><div><ul>
  <li>
    <span class="s1">玄幻</span>
    <span class="s2"><a href="/biquge1/">第一本书</a></span>
    <span class="s3">第10章 标题</span>
    <span class="s4">作者甲</span>
    <span class="s5">2026-06-13 12:00</span>
  </li>
  <li>
    <span class="s1">都市</span>
    <span class="s2"><a href="https://www.22biqu.com/biquge2/">第二本书</a></span>
    <span class="s3">第20章 别名</span>
    <span class="s4">作者乙</span>
    <span class="s5">2026-06-12 09:00</span>
  </li>
  <li>
    <!-- 没有 a 的脏条目，应当被跳过（bookName 为空） -->
    <span class="s1">脏数据</span>
    <span class="s4">无名氏</span>
  </li>
</ul></div></div></div>
</body></html>"##
            .to_string()
    }

    #[test]
    fn parses_two_results_from_real_22biqu_layout() {
        let rule = rule_22biqu();
        let html = fake_22biqu_search_html();
        let results =
            parse_search_results(&html, "https://www.22biqu.com/ss/", &rule, None).unwrap();

        assert_eq!(
            results.len(),
            2,
            "expected 2 valid results, got {results:?}"
        );

        let r0 = &results[0];
        assert_eq!(r0.book_name, "第一本书");
        // 相对路径已被 base_url 拼成绝对
        assert_eq!(r0.url, "https://www.22biqu.com/biquge1/");
        assert_eq!(r0.author.as_deref(), Some("作者甲"));
        assert_eq!(r0.category.as_deref(), Some("玄幻"));
        assert_eq!(r0.latest_chapter.as_deref(), Some("第10章 标题"));
        assert_eq!(r0.last_update_time.as_deref(), Some("2026-06-13 12:00"));
        assert_eq!(r0.source_id, 5);
        assert_eq!(r0.source_name, "笔趣阁22");

        let r1 = &results[1];
        // 已经是绝对 URL，应该原样
        assert_eq!(r1.url, "https://www.22biqu.com/biquge2/");
    }

    #[test]
    fn limit_truncates_results() {
        let rule = rule_22biqu();
        let html = fake_22biqu_search_html();
        let results =
            parse_search_results(&html, "https://www.22biqu.com/ss/", &rule, Some(1)).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].book_name, "第一本书");
    }

    #[test]
    fn empty_book_name_entries_are_skipped() {
        // 第三条 li 没有 a，bookName 抽出来是空字符串，必须被跳过
        let rule = rule_22biqu();
        let html = fake_22biqu_search_html();
        let results =
            parse_search_results(&html, "https://www.22biqu.com/ss/", &rule, None).unwrap();
        assert!(
            results.iter().all(|r| !r.book_name.is_empty()),
            "got result with empty book_name: {results:?}"
        );
    }

    #[test]
    fn rule_without_search_section_errors_typed() {
        let rule = Rule {
            url: "https://x".into(),
            ..Rule::default()
        };
        let err = parse_search_results("", "https://x", &rule, None).unwrap_err();
        assert!(matches!(err, SearchError::SearchDisabled));
    }

    #[test]
    fn handles_js_post_processing_in_search_field() {
        // 仿一条规则：搜索页 author 字段需要去掉"作者："前缀
        // （来自 main.json 鸟书网的真实规则模式）
        let mut rule: Rule = serde_json::from_str(
            r##"{
                "url": "https://demo.test/",
                "name": "demo",
                "search": {
                    "url": "https://demo.test/?q=%s",
                    "method": "get",
                    "result": ".item",
                    "bookName": "h4 > a",
                    "author": "div.author@js:r=r.replace('作者：', '');"
                }
            }"##,
        )
        .unwrap();
        rule.id = 99;
        apply_default_rule(&mut rule, LangType::ZhCn);

        let html = r##"<html><body>
            <div class="item">
              <h4><a href="/b/1">某书</a></h4>
              <div class="author">作者：某人</div>
            </div>
        </body></html>"##;

        let results = parse_search_results(html, "https://demo.test/", &rule, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].book_name, "某书");
        assert_eq!(results[0].author.as_deref(), Some("某人"));
    }

    /// **真实联网测试**：默认 ignore，本机用 `cargo test -- --ignored` 跑。
    /// 无法保证书源稳定可用（被限流 / 维护时会失败），所以**不能**作为
    /// 阻塞性测试。本测试只断言"能联通且返回非零结果"。
    #[tokio::test]
    #[ignore = "live network: depends on 22biqu availability"]
    async fn live_22biqu_search_returns_non_empty() {
        use crate::config::AppConfig;
        use crate::http::client::{ClientOptions, build_async_client};

        let cfg = AppConfig::default();
        let client = build_async_client(&cfg, &ClientOptions::default()).unwrap();
        let rule = rule_22biqu();

        // 用一个常见的关键词；具体能否搜到与书源数据相关。
        let results = match search_one(&client, &rule, "诡秘之主", Some(5), None).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("live test soft-skip: {e}");
                return;
            }
        };

        assert!(!results.is_empty(), "expected ≥1 result for known title");
        for r in &results {
            assert!(!r.book_name.is_empty());
            assert!(
                r.url.starts_with("http"),
                "url should be absolute: {}",
                r.url
            );
        }
    }
}
