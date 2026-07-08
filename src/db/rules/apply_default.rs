//! 默认值填充 (PR #17 拆分, 2026-07-08).
//!
//! 给一条 `Rule` 填默认值。等价于 Java `util.SourceUtils#applyDefaultRule`:
//! - `language` 空 → 用系统检测到的 locale;
//! - `search/book/toc/chapter.base_uri` 空 → 用 `rule.url`;
//! - 各 section 的 `timeout` 空 → 15s (toc 60s);
//! - `book.*` 字段空 → 用 `META_*` 常量 (让 scraper 走浏览器 meta 解析)。

use crate::config::LangType;
use crate::models::Rule;

use super::constants::{
    META_AUTHOR, META_BOOK_NAME, META_CATEGORY, META_COVER_URL, META_INTRO, META_LATEST_CHAPTER,
    META_LATEST_CHAPTER_URL, META_LAST_UPDATE_TIME, META_STATUS,
};

/// 给一条 `Rule` 填默认值。
///
/// # Examples
///
/// ```ignore
/// let mut rule = Rule::default();
/// rule.url = "https://example.com".into();
/// apply_default_rule(&mut rule, LangType::ZhCn);
/// assert_eq!(rule.language, "zh-CN");
/// assert!(rule.book.as_ref().unwrap().base_uri == "https://example.com");
/// ```
pub fn apply_default_rule(rule: &mut Rule, system_lang: LangType) {
    if rule.language.trim().is_empty() {
        rule.language = system_lang.as_str().to_string();
    }

    let url = rule.url.clone();

    if let Some(s) = rule.search.as_mut() {
        if s.base_uri.is_empty() {
            s.base_uri = url.clone();
        }
        if s.timeout.is_none() {
            s.timeout = Some(15);
        }
    }
    if let Some(b) = rule.book.as_mut() {
        if b.base_uri.is_empty() {
            b.base_uri = url.clone();
        }
        if b.timeout.is_none() {
            b.timeout = Some(15);
        }
        // book 字段缺失时回落到 meta 查询 (与 Java 端 `StrUtil.emptyToDefault` 等价)。
        if b.book_name.is_empty() {
            b.book_name = META_BOOK_NAME.to_string();
        }
        if b.author.is_empty() {
            b.author = META_AUTHOR.to_string();
        }
        if b.intro.is_empty() {
            b.intro = META_INTRO.to_string();
        }
        if b.cover_url.is_empty() {
            b.cover_url = META_COVER_URL.to_string();
        }
        if b.category.is_empty() {
            b.category = META_CATEGORY.to_string();
        }
        if b.latest_chapter.is_empty() {
            b.latest_chapter = META_LATEST_CHAPTER.to_string();
        }
        if b.latest_chapter_url.is_empty() {
            b.latest_chapter_url = META_LATEST_CHAPTER_URL.to_string();
        }
        if b.last_update_time.is_empty() {
            b.last_update_time = META_LAST_UPDATE_TIME.to_string();
        }
        if b.status.is_empty() {
            b.status = META_STATUS.to_string();
        }
    }
    if let Some(t) = rule.toc.as_mut() {
        if t.base_uri.is_empty() {
            t.base_uri = url.clone();
        }
        if t.timeout.is_none() {
            t.timeout = Some(60);
        }
    }
    if let Some(c) = rule.chapter.as_mut() {
        if c.base_uri.is_empty() {
            c.base_uri = url.clone();
        }
        if c.timeout.is_none() {
            c.timeout = Some(15);
        }
    }
}
