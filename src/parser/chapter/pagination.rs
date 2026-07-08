//! 单章分页正文抓取 (PR #17 拆分, 2026-07-08).
//!
//! 来自原 `parser/chapter.rs`:
//! - [`fetch_paginated_content`] 循环抓 + 拼 + 判定终止
//! - [`NextStep`] 把 Html 析出 await 之外用的辅助 enum
//! - [`resolve_next_url`] 找下一页 URL (nextPageInJs / nextPage 二选一)
//! - [`is_last_page`] 终止判定 (nextChapterLink 正则 / 通用文本兜底)
//! - [`PAGINATION_URL_RE`] / [`NEXT_CHAPTER_TEXT_RE`] LazyLock 静态正则
//!
//! 入口 [`super::parse::parse_chapter`]。

use regex::Regex;
use reqwest::Client;
use scraper::Html;
use std::sync::LazyLock;

use crate::http::abs_url;
use crate::models::{ContentType, Rule, RuleChapter};
use crate::parser::dom::select_and_invoke_js;

use super::parse::{ChapterError, fetch_with_cf_fallback};

/// 循环抓分页正文 + 拼接 + 终止判定。
///
/// 防御性上限 50 页: 单章超过 50 页基本是反爬死循环。`Html` 不 `Send`，
/// 所以"解析 + 选 + 拼"全部塞进 sync 子作用域 (`let (content, next_step) = { ... }`),
/// 子作用域结束时 `Html` drop, 再带 `String` 出 await 边界。
pub(super) async fn fetch_paginated_content(
    client: &Client,
    rule: &Rule,
    start_url: &str,
    cf_bypass_base: Option<&str>,
) -> Result<String, ChapterError> {
    let chapter_rule = rule
        .chapter
        .as_ref()
        .ok_or(ChapterError::ChapterRuleMissing)?;

    let mut buf = String::new();
    let mut current_url = start_url.to_string();
    let mut pages = 0usize;
    let started = std::time::Instant::now();

    // 防御性上限：单章超过 50 页基本是反爬死循环
    for _ in 0..50 {
        let html =
            fetch_with_cf_fallback(client, &current_url, chapter_rule.timeout, cf_bypass_base)
                .await?;
        // ⚠️ scraper 的 Html 不是 Send（包含 Rc），不能跨 await 持有。
        // 把"解析 + 选 + 拼"全部放在一个 sync 子作用域里，先用 `let next_url = { ... }`
        // 把需要带出 await 之外的值（next_url / 是否终止）抽完，Html 就在子作用域结束时 drop。
        let (content, next_step) = {
            let document = Html::parse_document(&html);

            let content =
                select_and_invoke_js(&document, &chapter_rule.content, ContentType::Html)?;
            if content.is_empty() {
                return Err(ChapterError::EmptyContent(format!(
                    "{} returned empty at {current_url}",
                    chapter_rule.content
                )));
            }

            // 找下一页元素，解析候选 URL
            let next_sel = if chapter_rule.next_page.is_empty() {
                None
            } else {
                crate::parser::cache::cached_selector(&chapter_rule.next_page).ok()
            };
            let next_els: Vec<scraper::ElementRef<'_>> = match &next_sel {
                Some(s) => document.select(s).collect(),
                None => Vec::new(),
            };

            let candidate_next =
                resolve_next_url(&document, &next_els, chapter_rule, &current_url)?;
            let step = if is_last_page(&candidate_next, &next_els, chapter_rule) {
                NextStep::Stop
            } else {
                match candidate_next {
                    Some(next_url) if next_url != current_url => NextStep::Goto(next_url),
                    _ => NextStep::Stop,
                }
            };
            (content, step)
        };

        buf.push_str(&content);
        pages += 1;

        match next_step {
            NextStep::Stop => break,
            NextStep::Goto(next_url) => current_url = next_url,
        }
    }

    tracing::debug!(
        pages = pages,
        bytes = buf.len(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "分页正文抓取完成"
    );
    Ok(buf)
}

/// 分页正文抓取的下一步动作。把 Html 析出 await 之外用的辅助 enum。
enum NextStep {
    Stop,
    Goto(String),
}

/// 在已解析的页面里找下一页 URL。
///
/// 优先级: `nextPageInJs` (从某段 script 内部用 JS 抽 URL, 如 96读书) →
/// `chapter.nextPage` 选元素的 `href`。
pub(super) fn resolve_next_url(
    document: &Html,
    next_els: &[scraper::ElementRef<'_>],
    chapter_rule: &RuleChapter,
    current_url: &str,
) -> Result<Option<String>, ChapterError> {
    if !chapter_rule.next_page_in_js.is_empty() {
        // 从某段 script 内部用 JS 抽 URL（96读书 的 nextpage 模式）。
        let v = select_and_invoke_js(document, &chapter_rule.next_page_in_js, ContentType::Html)?;
        let v = v.trim().to_string();
        if v.is_empty() {
            return Ok(None);
        }
        // 可能是相对路径
        return Ok(abs_url(current_url, &v));
    }
    let Some(first) = next_els.first() else {
        return Ok(None);
    };
    let href = first.value().attr("href").unwrap_or_default();
    Ok(abs_url(current_url, href))
}

/// 终止判定。两层:
/// 1. `chapter.next_chapter_link` 正则命中 → 终止 (说明已经跳到下一章);
/// 2. 兜底: URL 不再像 `*[-_]数字.html`, 且按钮文本含 `下一章/没有了/>>/书末页`。
pub(super) fn is_last_page(
    candidate: &Option<String>,
    next_els: &[scraper::ElementRef<'_>],
    chapter_rule: &RuleChapter,
) -> bool {
    let Some(next_url) = candidate else {
        return true;
    };

    if !chapter_rule.next_chapter_link.is_empty() {
        if let Ok(re) = crate::parser::cache::cached_regex(&chapter_rule.next_chapter_link) {
            if re.is_match(next_url) {
                return true;
            }
        }
    }

    // 通用兜底：URL 不再像 *_数字.html 形式，且按钮文本含"下一章"等关键字
    let url_is_pagination = PAGINATION_URL_RE.is_match(next_url);
    let texts: String = next_els
        .iter()
        .map(|e| e.text().collect::<Vec<_>>().join(""))
        .collect::<Vec<_>>()
        .join(" ");
    let mentions_next_chapter = NEXT_CHAPTER_TEXT_RE.is_match(&texts);

    !url_is_pagination && mentions_next_chapter
}

static PAGINATION_URL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#".*[-_]\d\.html"#).expect("pagination url re"));
static NEXT_CHAPTER_TEXT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(下一章|没有了|>>|书末页)").expect("next chapter text re"));

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LangType;
    use crate::db::apply_default_rule;
    use scraper::Selector;

    fn rule_22biqu_chapter() -> Rule {
        let mut r: Rule = serde_json::from_str(
            r##"{
                "url": "https://www.22biqu.com/",
                "name": "笔趣阁22",
                "chapter": {
                    "title": ".title",
                    "content": "#content",
                    "paragraphTagClosed": true,
                    "filterTxt": "\\(本章完\\)",
                    "filterTag": ""
                }
            }"##,
        )
        .unwrap();
        r.id = 5;
        apply_default_rule(&mut r, LangType::ZhCn);
        r
    }

    #[test]
    fn is_last_page_when_no_candidate() {
        let chapter_rule = rule_22biqu_chapter().chapter.unwrap();
        assert!(is_last_page(&None, &[], &chapter_rule));
    }

    #[test]
    fn is_last_page_when_candidate_matches_next_chapter_regex() {
        let mut chapter_rule = rule_22biqu_chapter().chapter.unwrap();
        chapter_rule.next_chapter_link = r"^https://demo\.test/n/\d+\.html$".to_string();
        let candidate = Some("https://demo.test/n/2.html".to_string());
        assert!(is_last_page(&candidate, &[], &chapter_rule));
    }

    #[test]
    fn not_last_page_when_url_is_pagination() {
        let chapter_rule = rule_22biqu_chapter().chapter.unwrap();
        // 形如 https://x/c_2.html 视为还在分页内
        let candidate = Some("https://demo.test/c_2.html".to_string());
        assert!(!is_last_page(&candidate, &[], &chapter_rule));
    }

    #[test]
    fn last_page_via_generic_text_rule() {
        // 下一页元素文本含"下一章"，且 URL 不像分页
        let chapter_rule = rule_22biqu_chapter().chapter.unwrap();
        let html = r#"<html><body><a href="/n/3.html">下一章</a></body></html>"#;
        let doc = Html::parse_document(html);
        let sel = Selector::parse("a").unwrap();
        let els: Vec<_> = doc.select(&sel).collect();
        let candidate = Some("https://demo.test/n/3.html".to_string());
        assert!(is_last_page(&candidate, &els, &chapter_rule));
    }

    // ---------- nextPageInJs 模拟 ----------
    //
    // 96读书规则使用 XPath `//*[@id="readbg"]/script[4]` 从一段 script 文本里
    // 用 JS `r.match(/nextpage = "(.*?)"/)[1]` 提取下一页 URL；
    // 我们的 dom 模块已支持把这条 XPath 改写为 `#readbg > script:nth-of-type(4)`。
    //
    // 这里只验证：在 resolve_next_url 中走 nextPageInJs 路径能拿到正确 URL。
    #[test]
    fn next_page_in_js_extracts_url_from_script() {
        let html = r##"<html><body>
            <div id="readbg">
                <script>var a = 1;</script>
                <script>var b = 2;</script>
                <script>var c = 3;</script>
                <script>var nextpage = "/n/123/2.html";</script>
            </div>
        </body></html>"##;
        let document = Html::parse_document(html);

        let mut chapter_rule = rule_22biqu_chapter().chapter.unwrap();
        chapter_rule.next_page_in_js =
            r##"//*[@id="readbg"]/script[4]@js:r=r.match(/nextpage = "(.*?)"/)[1];"##.into();

        let candidate = resolve_next_url(
            &document,
            &[],
            &chapter_rule,
            "https://www.96dushu.com/n/123/1.html",
        )
        .unwrap();
        assert_eq!(
            candidate.as_deref(),
            Some("https://www.96dushu.com/n/123/2.html")
        );
    }
}