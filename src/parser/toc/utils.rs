//! 目录解析工具 + 错误类型 (PR #17 拆分, 2026-07-08).
//!
//! 来自原 `parser/toc.rs`:
//! - [`TocError`] 错误枚举 (跨文件共享)
//! - [`extract_book_id`] / [`format_with_id`] / [`resolve_base_for_join`] 工具函数
//!
//! 主流程 [`parse_toc`] 在 [`super::single`], 分页收集在 [`super::paginated`]。

use thiserror::Error;

use crate::models::Rule;
use crate::parser::dom::SelectError;

/// 目录解析错误。
#[derive(Debug, Error)]
pub enum TocError {
    /// 书源没有 `toc` 段。
    #[error("书源没有 toc 规则")]
    TocRuleMissing,
    /// HTTP 请求失败 (含 cf_bypass 旁路后仍失败)。
    #[error("HTTP 错误: {0}")]
    Http(String),
    /// 命中 Cloudflare 验证页, 未配置 cf-bypass 或旁路失败。
    #[error(
        "命中 Cloudflare 验证页, 未配置 cf-bypass 旁路或旁路失败 (请在 config.toml [global] cf-bypass 填地址): {0}"
    )]
    Cloudflare(String),
    /// HTML 解析失败 (选择器 / 元素抽取 / 结构不符)。
    #[error("HTML 解析失败: {0}")]
    Parse(String),
    /// 选择器 / JS 执行失败 (来自 dom 模块的 SelectError)。
    #[error("选择器/JS 执行失败: {0}")]
    Selector(#[from] SelectError),
}

/// 用 `Book.url` 这个正则从详情页 URL 中提取书 ID。
/// 没配 / 不匹配时返回 `None`。
///
/// Java 端用 hutool `ReUtil.getGroup1`; 规则里 `Book.url` 一定含一个捕获组。
/// 这里允许规则形如 `https://(?:www\.)?69shuba\.com/book/(.*?)\.htm`。
pub(super) fn extract_book_id(rule: &Rule, book_url: &str) -> Option<String> {
    let book_rule = rule.book.as_ref()?;
    if book_rule.url.is_empty() {
        return None;
    }
    let re = crate::parser::cache::cached_regex(&book_rule.url).ok()?;
    let cap = re.captures(book_url)?;
    cap.get(1).map(|m| m.as_str().to_string())
}

/// 把 `template` 里的第一处 `%s` 用 `id` 替换;
/// `id` 为 None 或 template 为空时原样返回。
pub(super) fn format_with_id(template: &str, id: Option<&str>) -> String {
    if template.is_empty() {
        return String::new();
    }
    match id {
        Some(v) => template.replacen("%s", v, 1),
        None => template.to_string(),
    }
}

/// 计算 absUrl 的 base:
/// - 优先用 `toc.baseUri` (已经被 ID 模板格式化过),
/// - 否则用当前页 URL。
pub(super) fn resolve_base_for_join(toc_base_uri: &str, current_page_url: &str) -> String {
    if !toc_base_uri.trim().is_empty() {
        toc_base_uri.to_string()
    } else {
        current_page_url.to_string()
    }
}
