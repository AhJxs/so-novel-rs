//! 详情页解析。对应 Java `parse.BookParser`。
//!
//! 阶段 2b 实现的能力（Java 端等价子集）：
//! - GET 详情页（编码兜底已由 fetch 层完成）；
//! - 检测 Cloudflare（命中返回 `BookError::Cloudflare`，不在本阶段做旁路）；
//! - bookName / author 必填，否则报错；
//! - 其余字段（intro / category / coverUrl / latestChapter / lastUpdateTime /
//!   status）的字段查询字符串如果以 `meta[` 开头，按 `ATTR_CONTENT` 抽，否则按 `TEXT` 抽，
//!   与 Java `BookParser#getContentType` 等价；
//! - 选 coverUrl 时 attr_content 是相对路径的话，用 `abs_url` 拼绝对（Java 用 `absUrl`）。
//!
//! **未实现**（后续阶段）：
//! - `CoverUpdater`（起点 cookie 取最新封面），属阶段 4 / 阶段 5；
//! - 简繁转换（属阶段 5）；
//! - CF bypass 旁路（属阶段 2c）。

use anyhow::Result;
use reqwest::Client;
use scraper::Html;
use thiserror::Error;

use crate::crawler::cover_updater;
use crate::http::{FetchRequest, HttpMethod, fetch, fetch_via_cf_bypass, has_cloudflare};
use crate::models::{Book, ContentType, Rule};
use crate::parser::dom::{SelectError, select_and_invoke_js};

#[derive(Debug, Error)]
pub enum BookError {
    #[error("书源没有 book 规则")]
    BookRuleMissing,
    #[error("HTTP 错误: {0}")]
    Http(String),
    #[error(
        "命中 Cloudflare 验证页，未配置 cf-bypass 旁路（请在 config.toml [global] cf-bypass 填地址）: {0}"
    )]
    Cloudflare(String),
    #[error("详情页书名或作者为空")]
    MissingTitleOrAuthor,
    #[error("HTML 解析失败: {0}")]
    Parse(String),
    #[error("选择器/JS 执行失败: {0}")]
    Selector(#[from] SelectError),
}

/// 抓取 + 解析详情页。
///
/// `cf_bypass_base` 同 `search_one`：CF 命中时若非空则自动重试 bypass 服务。
/// `qidian_cookie` 是全局 `AppConfig.qidian_cookie` —— **仅供 CoverUpdater 使用**，
/// 详情页 fetch 本身**不附** Cookie 头（与 Java 端语义一致；cookie 只在 CoverUpdater
/// 跑起点站搜索时才用得上）。
///
/// 末尾 `!rule.need_proxy` 时调 3 站 CoverUpdater 拿更高清封面（与 Java
/// `BookParser.parse()` line 71 行为对齐）。
pub async fn parse_book_detail(
    client: &Client,
    rule: &Rule,
    url: &str,
    cf_bypass_base: Option<&str>,
    qidian_cookie: Option<&str>,
) -> Result<Book, BookError> {
    let started = std::time::Instant::now();
    let book_rule = rule.book.as_ref().ok_or(BookError::BookRuleMissing)?;

    let response = fetch(
        client,
        &FetchRequest {
            url,
            method: HttpMethod::Get,
            cookies: None,
            timeout_secs: book_rule.timeout,
            referer: None,
        },
    )
    .await
    .map_err(|e| BookError::Http(format!("{e:#}")))?;

    let cf_hit = has_cloudflare(&response.html);
    let html_after_cf = if cf_hit {
        match cf_bypass_base.filter(|s| !s.trim().is_empty()) {
            Some(base) => {
                tracing::info!(source_id = rule.id, book_url = %url, "详情页命中 Cloudflare，尝试 cf-bypass");
                fetch_via_cf_bypass(client, base, url)
                    .await
                    .map_err(|e| BookError::Http(format!("cf-bypass: {e:#}")))?
            }
            None => {
                tracing::warn!(source_id = rule.id, book_url = %url, "详情页命中 Cloudflare 但未配置 cf-bypass");
                return Err(BookError::Cloudflare(response.final_url.clone()));
            }
        }
    } else {
        response.html
    };

    let mut book = parse_book_html(&html_after_cf, &response.final_url, rule)?;

    // 3 站 CoverUpdater：仅 `!rule.need_proxy` 时跑（与 Java `BookParser.parse()`
    // line 71 一致 —— 代理 IP 会被起点等网站屏蔽，故代理时不使用源站封面）。
    // 失败/无可用候选时 `cover_updater::fetch_cover` 内部已经返回原 fallback，
    // 这里无脑赋值即可。
    if !rule.need_proxy {
        tracing::debug!(source_id = rule.id, book = %book.book_name, has_qidian_cookie = qidian_cookie.map(|s| !s.trim().is_empty()).unwrap_or(false), "触发 CoverUpdater（3 站 fan-out）");
        let new_cover = cover_updater::fetch_cover(
            client,
            &book,
            book.cover_url.as_deref(),
            qidian_cookie.unwrap_or(""),
        )
        .await;
        if !new_cover.is_empty() && book.cover_url.as_deref() != Some(new_cover.as_str()) {
            tracing::info!(source_id = rule.id, book = %book.book_name, "CoverUpdater 替换封面");
            book.cover_url = Some(new_cover);
        }
    }

    tracing::info!(
        source_id = rule.id,
        book = %book.book_name,
        author = %book.author,
        cf_hit = cf_hit,
        cover_url = ?book.cover_url,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "parse_book_detail: 完成",
    );
    Ok(book)
}

/// 仅做 HTML → Book 的解析，不抓网络。便于离线测试。
pub fn parse_book_html(html: &str, base_url: &str, rule: &Rule) -> Result<Book, BookError> {
    let book_rule = rule.book.as_ref().ok_or(BookError::BookRuleMissing)?;
    let document = Html::parse_document(html);

    let book_name = select_and_invoke_js(
        &document,
        &book_rule.book_name,
        content_type_for(&book_rule.book_name),
    )?;
    let author = select_and_invoke_js(
        &document,
        &book_rule.author,
        content_type_for(&book_rule.author),
    )?;
    // Java 端 BookParser: author.replace("作者：", "")
    let author = author.replace("作者：", "").replace("作者:", "");
    if book_name.is_empty() || author.is_empty() {
        return Err(BookError::MissingTitleOrAuthor);
    }

    let intro = optional_field(&document, &book_rule.intro)?;
    let category = optional_field(&document, &book_rule.category)?;
    let latest_chapter = optional_field(&document, &book_rule.latest_chapter)?;
    let latest_chapter_url = optional_field(&document, &book_rule.latest_chapter_url)?
        .and_then(|u| crate::http::abs_url(base_url, &u).or(Some(u)));
    let last_update_time = optional_field(&document, &book_rule.last_update_time)?.map(|s| {
        // Java 端 BookParser: lastUpdateTime.replaceAll("(更新时间|最后更新)：", "")
        use once_cell::sync::Lazy;
        use regex::Regex;
        static RE: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^(更新时间|最后更新)[：:]?\s*").expect("lastUpdateTime prefix re")
        });
        RE.replace(&s, "").into_owned()
    });
    let status = optional_field(&document, &book_rule.status)?;

    // coverUrl 抽出来如果是相对路径，按 baseUri 拼成绝对（Java 端 jsoup `absUrl("content")`
    // 会自动做这件事）。
    let raw_cover = select_and_invoke_js(
        &document,
        &book_rule.cover_url,
        content_type_for(&book_rule.cover_url),
    )?;
    let cover_url = if raw_cover.is_empty() {
        None
    } else {
        crate::http::abs_url(base_url, &raw_cover).or(Some(raw_cover))
    };

    Ok(Book {
        url: base_url.to_string(),
        book_name,
        author,
        intro,
        category,
        cover_url,
        latest_chapter,
        latest_chapter_url,
        last_update_time,
        status,
        language: rule.language.clone(),
    })
}

/// 等价 Java `BookParser#getContentType`：以 `meta[` 开头的查询走 `attr=content`，
/// 否则走文本。这条规则只对 book 的字段成立（search/toc/chapter 不需要这层判断）。
fn content_type_for(query: &str) -> ContentType {
    if query.trim_start().starts_with("meta[") {
        ContentType::AttrContent
    } else {
        ContentType::Text
    }
}

fn optional_field(document: &Html, query: &str) -> Result<Option<String>, BookError> {
    if query.trim().is_empty() {
        return Ok(None);
    }
    let v = select_and_invoke_js(document, query, content_type_for(query))?;
    Ok(if v.is_empty() { None } else { Some(v) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LangType;
    use crate::rules::apply_default_rule;

    /// 笔趣阁22 真实详情规则。注意 Java 注释提到 meta 字段名拼错（`lastest_chapter_name`），
    /// 我们保留这个拼写以保持兼容。
    fn rule_22biqu() -> Rule {
        let mut r: Rule = serde_json::from_str(
            r##"{
                "url": "https://www.22biqu.com/",
                "name": "笔趣阁22",
                "book": {
                    "latestChapter": "meta[property=\"og:novel:lastest_chapter_name\"]"
                }
            }"##,
        )
        .expect("rule should parse");
        r.id = 5;
        apply_default_rule(&mut r, LangType::ZhCn);
        r
    }

    /// 一个仿真的详情页 HTML：包含 og:novel:* meta 标签 + 简介 div。
    /// 22biqu 的详情页字段 99% 来自 meta，配规则里 book 段几乎是空的，
    /// 所以默认填充会把 bookName / author / intro / category / coverUrl 等
    /// 全部回退到 meta 查询。
    fn fake_book_html() -> String {
        r##"<!doctype html>
<html><head>
<title>书名 - 笔趣阁22</title>
<meta property="og:novel:book_name" content="测试书名">
<meta property="og:novel:author" content="测试作者">
<meta property="og:novel:category" content="玄幻">
<meta property="og:image" content="/cover/1.jpg">
<meta property="og:novel:lastest_chapter_name" content="第99章 标题">
<meta property="og:novel:update_time" content="2026-06-13 12:00">
<meta property="og:novel:status" content="连载">
<meta name="description" content="一段简介">
</head><body>
<div id="info"><h1>测试书名</h1></div>
</body></html>"##
            .to_string()
    }

    #[test]
    fn parses_book_via_meta_defaults() {
        let rule = rule_22biqu();
        let book = parse_book_html(&fake_book_html(), "https://www.22biqu.com/biqu123/", &rule)
            .expect("should parse");

        assert_eq!(book.book_name, "测试书名");
        assert_eq!(book.author, "测试作者");
        assert_eq!(book.category.as_deref(), Some("玄幻"));
        assert_eq!(book.intro.as_deref(), Some("一段简介"));
        assert_eq!(book.latest_chapter.as_deref(), Some("第99章 标题"));
        assert_eq!(book.last_update_time.as_deref(), Some("2026-06-13 12:00"));
        assert_eq!(book.status.as_deref(), Some("连载"));
        // 相对 cover URL 应被拼为绝对
        assert_eq!(
            book.cover_url.as_deref(),
            Some("https://www.22biqu.com/cover/1.jpg")
        );
        assert_eq!(book.url, "https://www.22biqu.com/biqu123/");
    }

    #[test]
    fn missing_book_name_or_author_returns_typed_error() {
        let rule = rule_22biqu();
        // 没有 og:novel:book_name 的 HTML
        let html = r##"<html><head>
            <meta property="og:novel:author" content="某人">
            </head><body></body></html>"##;
        let err = parse_book_html(html, "https://www.22biqu.com/x/", &rule).unwrap_err();
        assert!(matches!(err, BookError::MissingTitleOrAuthor), "{err}");
    }

    #[test]
    fn book_name_via_explicit_selector_overrides_meta_default() {
        // 模拟一条规则：bookName 不走 meta，而走显式 CSS 选择器
        let mut rule: Rule = serde_json::from_str(
            r##"{
                "url": "https://demo.test/",
                "name": "demo",
                "book": {
                    "bookName": "h1.book-title",
                    "author": ".info .author"
                }
            }"##,
        )
        .unwrap();
        rule.id = 99;
        apply_default_rule(&mut rule, LangType::ZhCn);

        let html = r##"<html><head>
            <meta property="og:novel:book_name" content="META书名">
            <meta property="og:novel:author" content="META作者">
        </head><body>
            <h1 class="book-title">真书名</h1>
            <div class="info"><span class="author">真作者</span></div>
        </body></html>"##;

        let book = parse_book_html(html, "https://demo.test/", &rule).expect("should parse");
        // 显式选择器优先于 meta
        assert_eq!(book.book_name, "真书名");
        assert_eq!(book.author, "真作者");
    }

    #[test]
    fn cover_url_with_js_postprocess_concats_host() {
        // main.json mcxs 真实规则：
        //   "coverUrl": "meta[property=\"og:image\"]@js:r='http://www.mcxs.info'+r"
        let mut rule: Rule = serde_json::from_str(
            r##"{
                "url": "http://www.mcxs.info/",
                "name": "mcxs",
                "book": {
                    "coverUrl": "meta[property=\"og:image\"]@js:r='http://www.mcxs.info'+r"
                }
            }"##,
        )
        .unwrap();
        rule.id = 3;
        apply_default_rule(&mut rule, LangType::ZhCn);

        let html = r##"<html><head>
            <meta property="og:novel:book_name" content="书">
            <meta property="og:novel:author" content="人">
            <meta property="og:image" content="/img/123.jpg">
        </head><body></body></html>"##;
        let book = parse_book_html(html, "http://www.mcxs.info/n/123/", &rule).unwrap();
        assert_eq!(
            book.cover_url.as_deref(),
            Some("http://www.mcxs.info/img/123.jpg")
        );
    }

    #[test]
    fn rule_without_book_section_errors_typed() {
        let rule = Rule {
            url: "https://x".into(),
            ..Rule::default()
        };
        let err = parse_book_html("", "https://x", &rule).unwrap_err();
        assert!(matches!(err, BookError::BookRuleMissing));
    }

    /// 真实联网测试：默认 ignore。
    #[tokio::test]
    #[ignore = "live network: depends on 22biqu availability"]
    async fn live_22biqu_book_detail_parses() {
        use crate::config::AppConfig;
        use crate::http::client::{ClientOptions, build_async_client};
        use crate::parser::search::search_one;

        let cfg = AppConfig::default();
        let client = build_async_client(&cfg, &ClientOptions::default()).unwrap();

        // 先搜一下，拿到第一个结果的 URL。
        let mut search_rule: Rule = serde_json::from_str(
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
                },
                "book": {
                    "latestChapter": "meta[property=\"og:novel:lastest_chapter_name\"]"
                }
            }"##,
        )
        .unwrap();
        search_rule.id = 5;
        apply_default_rule(&mut search_rule, LangType::ZhCn);

        let results = match search_one(&client, &search_rule, "诡秘之主", Some(1), None).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("live search soft-skip: {e}");
                return;
            }
        };
        let Some(first) = results.first() else {
            eprintln!("no search results, skipping live book test");
            return;
        };

        let book = match parse_book_detail(&client, &search_rule, &first.url, None, None).await {
            Ok(b) => b,
            Err(e) => {
                eprintln!("live book detail soft-skip: {e}");
                return;
            }
        };
        assert!(!book.book_name.is_empty(), "book_name empty");
        assert!(!book.author.is_empty(), "author empty");
    }
}
