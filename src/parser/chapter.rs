//! 单章正文解析。对应 Java `parse.ChapterParser`。
//!
//! 阶段 2c 实现：
//! - 单页：抓一页，按 `chapter.content` 取 HTML 字符串；
//! - 分页：循环抓 → 拼接，下一页 URL 顺序：
//!   1. 配了 `nextPageInJs` → 用 `select_and_invoke_js` 从某段 script
//!      内容里执行 JS 抽取 URL；
//!   2. 否则按 `chapter.nextPage` 选元素，取 `first.href`；
//! - 终止条件：
//!   - `nextChapterLink` 配置且 candidate 命中正则 → 终止（说明已经跳到下一章）；
//!   - 兜底：URL 不像分页（`!matches(".*[-_]\\d\\.html")`）且下一页元素文本含
//!     `下一章/没有了/>>/书末页` → 终止；
//! - CF 命中 → 走 cf-bypass 兜底。
//!
//! **不在本阶段做**：
//! - 正文清洗（filterTxt 正则替换、filterTag 节点删除、不可见字符清理、HTML 实体清理、
//!   重复标题去除、HTML 模板渲染）— 全部归阶段 3 `ChapterFilter` + `ChapterFormatter`。
//! - 重试（配置在 `enable-retry`）— 归阶段 3 调度层。
//! - 简繁转换 — 归阶段 5。

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use thiserror::Error;

use crate::http::{abs_url, fetch, fetch_via_cf_bypass, has_cloudflare, FetchRequest, HttpMethod};
use crate::models::{Chapter, ContentType, Rule};
use crate::parser::dom::{select_and_invoke_js, SelectError};

#[derive(Debug, Error)]
pub enum ChapterError {
    #[error("书源没有 chapter 规则")]
    ChapterRuleMissing,
    #[error("HTTP 错误: {0}")]
    Http(String),
    #[error("命中 Cloudflare 验证页，未配置 cf-bypass 旁路或旁路失败（请在 config.toml [global] cf-bypass 填地址）: {0}")]
    Cloudflare(String),
    #[error("正文为空: {0}")]
    EmptyContent(String),
    #[error("HTML 解析失败: {0}")]
    Parse(String),
    #[error("选择器/JS 执行失败: {0}")]
    Selector(#[from] SelectError),
}

/// 抓取并解析单章正文。
///
/// `chapter` 入参里只有 url/title/order 是有效的；本函数填回 `content`（原始 HTML，
/// 未做清洗 / 模板渲染 — 那些在阶段 3 的 ChapterFilter/Formatter 里做）。
///
/// `cf_bypass_base` 同其它 parser。
pub async fn parse_chapter(
    client: &Client,
    rule: &Rule,
    chapter: &Chapter,
    cf_bypass_base: Option<&str>,
) -> Result<Chapter, ChapterError> {
    let chapter_rule = rule
        .chapter
        .as_ref()
        .ok_or(ChapterError::ChapterRuleMissing)?;

    let content = if chapter_rule.pagination {
        fetch_paginated_content(client, rule, &chapter.url, cf_bypass_base).await?
    } else {
        fetch_single_page_content(client, rule, &chapter.url, cf_bypass_base).await?
    };

    Ok(Chapter {
        url: chapter.url.clone(),
        title: chapter.title.clone(),
        order: chapter.order,
        content,
    })
}

/// 仅做"已知 HTML → 正文 HTML 字符串"的纯解析；便于离线测试。
pub fn parse_chapter_html(html: &str, rule: &Rule) -> Result<String, ChapterError> {
    let chapter_rule = rule
        .chapter
        .as_ref()
        .ok_or(ChapterError::ChapterRuleMissing)?;
    let document = Html::parse_document(html);
    let content = select_and_invoke_js(&document, &chapter_rule.content, ContentType::Html)?;
    if content.is_empty() {
        return Err(ChapterError::EmptyContent(format!(
            "{} returned empty",
            chapter_rule.content
        )));
    }
    Ok(content)
}

async fn fetch_single_page_content(
    client: &Client,
    rule: &Rule,
    url: &str,
    cf_bypass_base: Option<&str>,
) -> Result<String, ChapterError> {
    let chapter_rule = rule
        .chapter
        .as_ref()
        .ok_or(ChapterError::ChapterRuleMissing)?;

    let html = fetch_with_cf_fallback(client, url, chapter_rule.timeout, cf_bypass_base).await?;
    parse_chapter_html(&html, rule)
}

async fn fetch_paginated_content(
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
                Selector::parse(&chapter_rule.next_page).ok()
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

        match next_step {
            NextStep::Stop => break,
            NextStep::Goto(next_url) => current_url = next_url,
        }
    }

    Ok(buf)
}

/// 分页正文抓取的下一步动作。把 Html 析出 await 之外用的辅助 enum。
enum NextStep {
    Stop,
    Goto(String),
}

/// 在已解析的页面里找下一页 URL。
fn resolve_next_url(
    document: &Html,
    next_els: &[scraper::ElementRef<'_>],
    chapter_rule: &crate::models::RuleChapter,
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

fn is_last_page(
    candidate: &Option<String>,
    next_els: &[scraper::ElementRef<'_>],
    chapter_rule: &crate::models::RuleChapter,
) -> bool {
    let Some(next_url) = candidate else {
        return true;
    };

    if !chapter_rule.next_chapter_link.is_empty() {
        if let Ok(re) = Regex::new(&chapter_rule.next_chapter_link) {
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

static PAGINATION_URL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#".*[-_]\d\.html"#).expect("pagination url re"));
static NEXT_CHAPTER_TEXT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(下一章|没有了|>>|书末页)").expect("next chapter text re"));

async fn fetch_with_cf_fallback(
    client: &Client,
    url: &str,
    timeout: Option<u32>,
    cf_bypass_base: Option<&str>,
) -> Result<String, ChapterError> {
    let resp = fetch(
        client,
        &FetchRequest {
            url,
            method: HttpMethod::Get,
            cookies: None,
            timeout_secs: timeout,
        },
    )
    .await
    .map_err(|e| ChapterError::Http(format!("{e:#}")))?;

    if has_cloudflare(&resp.html) {
        match cf_bypass_base.filter(|s| !s.trim().is_empty()) {
            Some(base) => fetch_via_cf_bypass(client, base, url)
                .await
                .map_err(|e| ChapterError::Http(format!("cf-bypass: {e:#}"))),
            None => Err(ChapterError::Cloudflare(resp.final_url)),
        }
    } else {
        Ok(resp.html)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LangType;
    use crate::rules::apply_default_rule;

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
    fn parses_single_page_chapter_content() {
        let html = r##"<html><body>
            <div class="title">第1章 起航</div>
            <div id="content">
                <p>第一段</p>
                <p>第二段</p>
            </div>
        </body></html>"##;
        let rule = rule_22biqu_chapter();
        let content = parse_chapter_html(html, &rule).unwrap();
        // content 走 HTML，内含两段 <p>
        assert!(content.contains("第一段"));
        assert!(content.contains("第二段"));
        // 不含 .title 的内容（说明 #content 选对了）
        assert!(!content.contains("第1章 起航"));
    }

    #[test]
    fn empty_content_returns_typed_error() {
        let html = r##"<html><body>
            <div class="title">无正文</div>
        </body></html>"##;
        let rule = rule_22biqu_chapter();
        let err = parse_chapter_html(html, &rule).unwrap_err();
        assert!(matches!(err, ChapterError::EmptyContent(_)));
    }

    #[test]
    fn no_chapter_rule_errors() {
        let rule = Rule {
            url: "https://x".into(),
            ..Rule::default()
        };
        let err = parse_chapter_html("", &rule).unwrap_err();
        assert!(matches!(err, ChapterError::ChapterRuleMissing));
    }

    // ---------- 终止判定 ----------

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
