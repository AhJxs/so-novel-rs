//! 单次 HTTP 请求封装。对应 Java `util.CrawlUtils#request` + 编码兜底。
//!
//! **必须是 async**：调用方在 `tokio::select!` 里 race future 与 cancel 信号，
//! 取消时 reqwest 立刻 drop 底层连接。blocking 版只能等 HTTP 自己超时（最坏 10s），
//! 用户感知就是"没反应"。
//!
//! 单次抓取只负责：加 UA / Referer / Cookie 头；GET/POST 与 form body；用
//! `decode_response_bytes` 兜底解码。**不做** CF 旁路调用（见 `fetch_with_cf_fallback`）。

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;
use reqwest::header::{ACCEPT, COOKIE, REFERER, USER_AGENT};

use crate::http::encoding::decode_response_bytes;
use crate::http::ua::random_ua;
use crate::http::url_join::origin_or_self;

/// 一次抓取的入参。
pub struct FetchRequest<'a> {
    pub url: &'a str,
    pub method: HttpMethod<'a>,
    /// 形如 `"k=v;k2=v2"` 的 cookie 字符串（与 Java 端规则字段直接拼）。
    pub cookies: Option<&'a str>,
    /// 单次请求超时（秒）。规则里以秒为单位；None 时用 client 默认。
    pub timeout_secs: Option<u32>,
    /// 自定义 Referer 头。非空时覆盖默认的 origin Referer。
    pub referer: Option<&'a str>,
}

pub enum HttpMethod<'a> {
    Get,
    Post(&'a [(String, String)]),
}

/// 抓取结果：解码后的 HTML、最终 URL（处理重定向后的）、状态码。
pub struct FetchResponse {
    pub html: String,
    pub final_url: String,
    pub status: u16,
}

/// 执行一次抓取。取消语义见模块头（in-flight 请求会被立刻 drop，无超时等待）。
///
/// # Examples
///
/// ```ignore
/// let resp = fetch(&client, &FetchRequest {
///     url: "https://example.com/",
///     method: HttpMethod::Get,
///     cookies: None,
///     timeout_secs: Some(10),
///     referer: None,
/// }).await?;
/// println!("{} bytes, final {}", resp.html.len(), resp.final_url);
/// ```
///
/// # Errors
///
/// - `reqwest::Error` — 网络 / 超时 / TLS / 重定向失败
/// - `decode_response_bytes` 失败 —— 由 `anyhow::Context` 包装
#[tracing::instrument(
    name = "http::fetch",
    skip_all,
    fields(
        url = %req.url,
        method = match req.method {
            HttpMethod::Get => "GET",
            HttpMethod::Post(_) => "POST",
        },
        timeout_secs = ?req.timeout_secs,
    )
)]
pub async fn fetch(client: &Client, req: &FetchRequest<'_>) -> Result<FetchResponse> {
    let referer = req
        .referer
        .filter(|s| !s.trim().is_empty())
        .map_or_else(|| origin_or_self(req.url), std::string::ToString::to_string);
    let ua = random_ua();

    let mut builder = match req.method {
        HttpMethod::Get => client.get(req.url),
        HttpMethod::Post(form) => client.post(req.url).form(form),
    };

    builder = builder
        .header(USER_AGENT, ua)
        .header(REFERER, referer)
        .header(
            ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        );

    if let Some(cookie_value) = req.cookies.filter(|s| !s.trim().is_empty()) {
        builder = builder.header(COOKIE, cookie_value);
    }
    if let Some(t) = req.timeout_secs {
        builder = builder.timeout(Duration::from_secs(t as u64));
    }

    let resp = builder
        .send()
        .await
        .with_context(|| format!("HTTP send failed: {}", req.url))?;

    let status = resp.status().as_u16();
    let final_url = resp.url().to_string();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(std::string::ToString::to_string);

    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("read body failed: {}", req.url))?;

    let html = decode_response_bytes(&bytes, content_type.as_deref());

    Ok(FetchResponse {
        html,
        final_url,
        status,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::config::AppConfig;
    use crate::http::client::{ClientOptions, build_async_client};

    /// 只验证构造与 builder 形状，不真发请求（联网测试在 search/book 模块用 `#[ignore]`）。
    #[tokio::test]
    async fn fetch_request_struct_compiles() {
        let cfg = AppConfig::default();
        let _client = build_async_client(&cfg, &ClientOptions::default()).unwrap();
        // 用 `req` 命名 —— `_req` 会触 `clippy::no_effect_underscore_binding`。
        let req = FetchRequest {
            url: "https://example.com/",
            method: HttpMethod::Get,
            cookies: None,
            timeout_secs: Some(15),
            referer: None,
        };
        // 消费 req 一次以避免 unused_assignments（即便仅构造也要参与类型推导/单态化）。
        let _ = req.url;
    }

    #[test]
    fn post_form_compiles() {
        let form: Vec<(String, String)> =
            vec![("k".into(), "v".into()), ("submit".into(), "Search".into())];
        // 同上：构造即验证 builder 形状。
        let req = FetchRequest {
            url: "https://example.com/s/",
            method: HttpMethod::Post(&form),
            cookies: Some("a=1; b=2"),
            timeout_secs: Some(15),
            referer: None,
        };
        let _ = req.url;
    }
}

/// 带 CF 真人验证旁路的 GET 请求：先发普通请求，命中 Cloudflare 验证页且
/// `cf_bypass_base` 非空时改走外部 bypass 服务，返回最终 HTML。
///
/// 统一错误类型为 `CfFallbackError`，调用方 `.map_err()` 转成自己的错误类型。
///
/// # Examples
///
/// 调用点在 `parser::toc` / `parser::chapter`：
/// `fetch_with_cf_fallback(client, url, rule.timeout, cf_bypass_base)`。
///
/// # Errors
///
/// - `CfFallbackError::Http` — 普通请求 / cf-bypass 请求失败
/// - `CfFallbackError::Cloudflare` — 命中 CF 但未配置 cf-bypass
#[tracing::instrument(
    name = "http::fetch_with_cf_fallback",
    skip_all,
    fields(url, has_bypass = cf_bypass_base.is_some())
)]
pub async fn fetch_with_cf_fallback(
    client: &reqwest::Client,
    url: &str,
    timeout: Option<u32>,
    cf_bypass_base: Option<&str>,
) -> Result<String, CfFallbackError> {
    let resp = super::fetch(
        client,
        &FetchRequest {
            url,
            method: HttpMethod::Get,
            cookies: None,
            timeout_secs: timeout,
            referer: None,
        },
    )
    .await
    .map_err(|e| CfFallbackError::Http(format!("{e:#}")))?;

    if super::has_cloudflare(&resp.html) {
        match cf_bypass_base.filter(|s| !s.trim().is_empty()) {
            Some(base) => {
                tracing::info!(url, "命中 Cloudflare，尝试 cf-bypass");
                super::fetch_via_cf_bypass(client, base, url)
                    .await
                    .map_err(|e| CfFallbackError::Http(format!("cf-bypass: {e:#}")))
            }
            None => Err(CfFallbackError::Cloudflare(resp.final_url)),
        }
    } else {
        Ok(resp.html)
    }
}

/// `fetch_with_cf_fallback` 的错误类型。
#[derive(Debug)]
pub enum CfFallbackError {
    Http(String),
    Cloudflare(String),
}
