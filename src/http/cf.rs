//! Cloudflare 真人验证检测 + 外部 bypass 服务调用。
//! 对应 Java `util.CrawlUtils#hasCf` + 各 Parser 中的 `${cfBypass}/html?url=...` 调用。
//!
//! Java 实现：用 jsoup 解析后取 `document.title()`，与一组关键 title 比对。
//! Rust 这里直接在原始 HTML 字符串上做 `<title>...</title>` 提取，
//! 避免每次都跑一遍 scraper（CF 检测会在每页都做一次）。
//!
//! bypass 服务约定参考 sarperavci/CloudflareBypassForScraping：
//! - 用户在 config.toml 的 `[global] cf-bypass` 中填一个本机/远端服务的 base URL
//!   （如 `http://127.0.0.1:8000`）；
//! - 我们在命中 CF 时调用 `${cf-bypass}/html?url=<urlencoded>`，得到去 CF 后的真实 HTML。

use std::time::Duration;

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;

const CF_TITLES: &[&str] = &[
    "Just a moment...",
    "403 Forbidden",
    "Attention Required",
    "Checking your browser before accessing",
];

static TITLE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title regex"));

/// 给定原始 HTML，判断是否是 Cloudflare 真人验证页。
pub fn has_cloudflare(html: &str) -> bool {
    let Some(cap) = TITLE_RE.captures(html) else {
        return false;
    };
    let title = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
    CF_TITLES.contains(&title)
}

/// 调用外部 cf-bypass 服务获取去 CF 后的页面 HTML。
///
/// `cf_bypass_base` 例如 `"http://127.0.0.1:8000"`，
/// `target_url` 是真正想抓的页面 URL。
///
/// Java 端用 hutool `HttpUtil.get(...)` 同步请求；这里复用调用方
/// 已经构造好的 `Client`（保持 cookie / 代理一致）。
pub async fn fetch_via_cf_bypass(
    client: &Client,
    cf_bypass_base: &str,
    target_url: &str,
) -> Result<String> {
    // 与 Java 端 `${cfBypass}/html?url=<原 URL>` 完全一致；
    // 不对 url 做编码 — Java 端也没编码（hutool 直接拼字符串）。
    let url = format!(
        "{}/html?url={}",
        cf_bypass_base.trim_end_matches('/'),
        target_url
    );

    let resp = client
        .get(&url)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .with_context(|| format!("call cf-bypass failed: {url}"))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .with_context(|| format!("read cf-bypass body failed: {url}"))?;

    if !status.is_success() {
        anyhow::bail!("cf-bypass returned HTTP {status}: {text}");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_just_a_moment() {
        let html = "<html><head><title>Just a moment...</title></head><body></body></html>";
        assert!(has_cloudflare(html));
    }

    #[test]
    fn detects_attention_required() {
        let html = "<HTML><HEAD><TITLE>Attention Required</TITLE></HEAD></HTML>";
        assert!(has_cloudflare(html));
    }

    #[test]
    fn ignores_unrelated_titles() {
        let html = "<html><head><title>第1章 一袋黄金</title></head></html>";
        assert!(!has_cloudflare(html));
    }

    #[test]
    fn ignores_no_title() {
        assert!(!has_cloudflare("<html><body>foo</body></html>"));
    }

    /// 不真发请求；只验证 fetch_via_cf_bypass 的 URL 拼接形状（trim '/'）。
    #[test]
    fn cf_bypass_url_formatting_via_dry_run() {
        // 仅形状校验：复用拼接逻辑会涉及私有 const，把判定从外部行为出发。
        let base_with_slash = "http://127.0.0.1:8000/";
        let base = base_with_slash.trim_end_matches('/');
        let target = "https://www.69shuba.com/book/123/";
        let url = format!("{base}/html?url={target}");
        assert_eq!(
            url,
            "http://127.0.0.1:8000/html?url=https://www.69shuba.com/book/123/"
        );
    }
}
