//! 单次 HTTP 请求封装。对应 Java `util.CrawlUtils#request` + 编码兜底。
//!
//! **async 版本**（`fetch`）：用 `reqwest::Client`（非 blocking），配合
//! `tokio::select!` 可以让外部 cancel 立刻中断 in-flight 请求。
//! 之前用 `reqwest::blocking::Client` + `tokio::task::spawn_blocking` 那条
//! 路线在 cancel 时只能等 HTTP 自己超时（最坏 10s），用户感知就是"没反应"。
//!
//! 单次抓取的责任：
//! 1. 加 UA / Referer / Cookie 头；
//! 2. 区分 GET / POST，POST 时把 form 数据填进 body；
//! 3. 用 `decode_response_bytes` 兜底解码；
//! 4. 调用方按需检测 CF（`http::cf::has_cloudflare`）—— 不在本函数里
//!    做 CF 旁路调用，旁路属阶段 2c 的 `cf-bypass` 服务集成。

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

/// 执行一次抓取。
///
/// Async：调用方在 `tokio::select!` 里 race 这个 future 和 cancel 信号，
/// 取消时 in-flight HTTP 立刻被 drop（reqwest 关闭底层连接），无超时等待。
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
        .map(|s| s.to_string())
        .unwrap_or_else(|| origin_or_self(req.url));
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
        .map(|s| s.to_string());

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
    use super::*;
    use crate::config::AppConfig;
    use crate::http::client::{ClientOptions, build_async_client};

    /// 这条测试只验证 fetch 函数能编译、能用 builder 模式调用；
    /// 不真发请求。真实联网测试在 search/book 模块下用 `#[ignore]` 标记。
    #[tokio::test]
    async fn fetch_request_struct_compiles() {
        let cfg = AppConfig::default();
        let _client = build_async_client(&cfg, &ClientOptions::default()).unwrap();
        let _req = FetchRequest {
            url: "https://example.com/",
            method: HttpMethod::Get,
            cookies: None,
            timeout_secs: Some(15),
            referer: None,
        };
        // 不调用 send；只确保 API 形状稳定。
    }

    #[test]
    fn post_form_compiles() {
        let form: Vec<(String, String)> =
            vec![("k".into(), "v".into()), ("submit".into(), "Search".into())];
        let _req = FetchRequest {
            url: "https://example.com/s/",
            method: HttpMethod::Post(&form),
            cookies: Some("a=1; b=2"),
            timeout_secs: Some(15),
            referer: None,
        };
    }
}

/// 带 CF 真人验证旁路的 GET 请求。
///
/// 先发普通请求；若命中 Cloudflare 验证页且 `cf_bypass_base` 非空，
/// 则通过外部 bypass 服务重试。返回最终 HTML。
///
/// `chapter.rs` / `toc.rs` 各有一份几乎相同的实现；这里统一为
/// `Result<String, CfFallbackError>`，调用方 `.map_err()` 转为自己的错误类型。
///
/// # Examples
///
/// ```ignore
/// let html = fetch_with_cf_fallback(&client, "https://x.com/", Some(10), None).await?;
/// ```
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
