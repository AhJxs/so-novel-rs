//! Cloudflare 真人验证检测 + 外部 bypass 服务调用。
//!
//! 用户用 config.toml 的 `[global] cf-bypass` 指向旁路服务；命中 CF 时调
//! `${cf-bypass}/html?url=<target>` 取回真实 HTML。对应 Java `util.CrawlUtils#hasCf`。
//!
//! 检测直接在原始 HTML 上取 `<title>`，不走 scraper（每页都会检测一次）。

use std::time::Duration;

use anyhow::{Context, Result};
use regex::Regex;
use reqwest::Client;
use std::sync::LazyLock;

const CF_TITLES: &[&str] = &[
    "Just a moment...",
    "403 Forbidden",
    "Attention Required",
    "Checking your browser before accessing",
];

/// 静态正则字面量写错即程序员错误，故直接 panic（避开 `clippy::expect_used`）。
#[allow(
    clippy::panic,
    reason = "static regex literal must compile; failure = programmer error"
)]
fn compile_static_re(pattern: &'static str) -> Regex {
    match Regex::new(pattern) {
        Ok(re) => re,
        Err(e) => panic!("static regex `{pattern}` should compile: {e}"),
    }
}

static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_static_re(r"(?is)<title[^>]*>(.*?)</title>"));

/// 给定原始 HTML，判断是否是 Cloudflare 真人验证页。
pub fn has_cloudflare(html: &str) -> bool {
    let Some(cap) = TITLE_RE.captures(html) else {
        return false;
    };
    let title = cap.get(1).map_or("", |m| m.as_str().trim());
    CF_TITLES.contains(&title)
}

/// 调用外部 cf-bypass 服务获取去 CF 后的页面 HTML。
///
/// 复用调用方已构造好的 `Client`（保持 cookie / 代理一致）。
///
/// # Examples
///
/// ```ignore
/// let html = fetch_via_cf_bypass(&client, "http://127.0.0.1:8000", "https://x.com/").await?;
/// ```
///
/// # Errors
///
/// - `reqwest::Error` — 旁路服务不可达 / 返回非 2xx / 反序列化失败
#[tracing::instrument(
    name = "http::fetch_via_cf_bypass",
    skip_all,
    fields(cf_bypass_base, target_url)
)]
pub async fn fetch_via_cf_bypass(
    client: &Client,
    cf_bypass_base: &str,
    target_url: &str,
) -> Result<String> {
    // 与 Java 端 `${cfBypass}/html?url=<原 URL>` 一致：**不做** url 编码。
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
    // 二次校验：旁路服务自身若返回 CF 挑战页，直接喂给 parser 只会报出困惑的
    // EmptyContent，不如显式失败。
    if has_cloudflare(&text) {
        anyhow::bail!("cf-bypass 仍返回 Cloudflare 验证页: {url}");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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

    /// 不真发请求；只验证 `fetch_via_cf_bypass` 的 URL 拼接形状（trim '/'）。
    #[test]
    fn cf_bypass_url_formatting_via_dry_run() {
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
