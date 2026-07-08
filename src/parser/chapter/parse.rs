//! 单章正文解析主流程
//!
//! 来自原 `parser/chapter.rs`:
//! - [`ChapterError`] 错误枚举
//! - [`parse_chapter`] 公共异步入口: 抓 + 解析 + 派发单页/分页
//! - [`parse_chapter_html`] 离线同步解析 (便于测试)
//! - [`fetch_single_page_content`] 单页抓取 (无 `nextPage` 规则时)
//! - [`fetch_with_cf_fallback`] `http::fetch_with_cf_fallback` 的 typed-error 包装
//!
//! 分页循环 + 终止判定在 [`super::pagination`]。

use anyhow::Result;
use reqwest::Client;
use scraper::Html;
use thiserror::Error;

use crate::models::{Chapter, ContentType, Rule};
use crate::parser::dom::{SelectError, select_and_invoke_js};

use super::pagination::fetch_paginated_content;

#[derive(Debug, Error)]
pub enum ChapterError {
    #[error("书源没有 chapter 规则")]
    ChapterRuleMissing,
    #[error("HTTP 错误: {0}")]
    Http(String),
    #[error(
        "命中 Cloudflare 验证页，未配置 cf-bypass 旁路或旁路失败（请在 config.toml [global] cf-bypass 填地址）: {0}"
    )]
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
///
/// # Examples
///
/// ```ignore
/// let ch = parse_chapter(&client, &rule, &chapter, None).await?;
/// println!("{} 字", ch.content.len());
/// ```
///
/// # Errors
///
/// - `ChapterError::ChapterRuleMissing` — 规则没有 `chapter` 段
/// - `ChapterError::Http` / `ChapterError::Cloudflare` — 抓取失败
/// - `ChapterError::EmptyContent` — `chapter.content` 选择器拿到空字符串
/// - `ChapterError::Parse` / `ChapterError::Selector` — HTML 解析失败
#[tracing::instrument(
    name = "parse_chapter",
    skip_all,
    fields(
        source_id = rule.id,
        source = %rule.name,
        order = chapter.order,
        title = %chapter.title,
        url = %chapter.url,
    )
)]
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
    let pagination = !chapter_rule.next_page.is_empty();

    let content = if pagination {
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
///
/// # Examples
///
/// ```ignore
/// let html = parse_chapter_html(&raw_html, &rule)?;
/// ```
///
/// # Errors
///
/// - `ChapterError::ChapterRuleMissing` — 规则没有 `chapter` 段
/// - `ChapterError::EmptyContent` — `chapter.content` 选择器拿到空字符串
/// - `ChapterError::Selector` — 选择器/JS 执行失败
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

pub(super) async fn fetch_single_page_content(
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

/// typed-error 包装的 cf-fallback 抓取。pagination 与 `single_page` 都通过这一层
/// 走 `crate::http::fetch_with_cf_fallback`。
pub(super) async fn fetch_with_cf_fallback(
    client: &Client,
    url: &str,
    timeout: Option<u32>,
    cf_bypass_base: Option<&str>,
) -> Result<String, ChapterError> {
    crate::http::fetch_with_cf_fallback(client, url, timeout, cf_bypass_base)
        .await
        .map_err(|e| match e {
            crate::http::CfFallbackError::Http(msg) => ChapterError::Http(msg),
            crate::http::CfFallbackError::Cloudflare(final_url) => {
                ChapterError::Cloudflare(final_url)
            }
        })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::config::LangType;
    use crate::db::apply_default_rule;

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
        let html = r#"<html><body>
            <div class="title">第1章 起航</div>
            <div id="content">
                <p>第一段</p>
                <p>第二段</p>
            </div>
        </body></html>"#;
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
        let html = r#"<html><body>
            <div class="title">无正文</div>
        </body></html>"#;
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
}
