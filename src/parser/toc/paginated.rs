//! 分页 URL 收集: 模式 1 在 `first_html` 上一次解析, 模式 2 循环抓下一页。主流程在 [`super::single`]。

use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::http::abs_url;

use super::utils::{TocError, resolve_base_for_join};

/// 在已抓的第一页里找全部分页 URL。
///
/// 1. `nextPage` 选出的元素若带 `value` 属性 → 把每个 `value` (或 `href`) 当分页 URL
///    (与 select-option 等价), 一次拿全;
/// 2. 否则递归翻页: 每抓一页都用 `nextPage` 取"下一页"那一个 URL, 直到拿不到。
///
/// # Examples
///
/// ```ignore
/// let urls = collect_pagination_urls(
///     &client, &html, &first_url,
///     "https://example.com/book/",
///     "#pages > option", None, None,
/// ).await?;
/// ```
///
/// # Errors
///
/// - `TocError::Parse` — 选择器无效
/// - `TocError::Http` / `TocError::Cloudflare` — 抓取失败 (来自 `fetch_with_cf_fallback`)
pub(super) async fn collect_pagination_urls(
    client: &Client,
    first_html: &str,
    first_url: &str,
    toc_base_uri: &str,
    next_page_query: &str,
    timeout: Option<u32>,
    cf_bypass_base: Option<&str>,
) -> Result<Vec<String>, TocError> {
    // 模式 1 (select-option): 完全在 first_html 上就能搞定, 不涉及 await;
    // 用单独 sync 函数隔离 scraper::Html, 避免跨 await 持有非 Send 类型。
    if let Some(option_urls) =
        collect_option_pagination_urls(first_html, first_url, toc_base_uri, next_page_query)?
    {
        return Ok(option_urls);
    }

    let sel = Selector::parse(next_page_query).map_err(|e| {
        TocError::Parse(format!("无效的 nextPage 选择器 `{next_page_query}`: {e:?}"))
    })?;
    let mut out: Vec<String> = Vec::new();
    let mut current_html = first_html.to_string();
    let mut current_url = first_url.to_string();
    // 保险阀: 现实中分页不会超过几十页; 上限 200 防止反爬死循环。
    for _ in 0..200 {
        let next_url_opt: Option<String> = {
            let doc = Html::parse_document(&current_html);
            let elements: Vec<scraper::ElementRef<'_>> = doc.select(&sel).collect();
            (|| {
                let next_el = elements.first()?;
                let href = next_el.value().attr("href")?;
                let next = abs_url(&resolve_base_for_join(toc_base_uri, &current_url), href)?;
                if next == current_url || out.contains(&next) {
                    return None;
                }
                Some(next)
            })()
        };
        let Some(next_url) = next_url_opt else {
            break;
        };
        out.push(next_url.clone());
        current_html = fetch_with_cf_fallback(client, &next_url, timeout, cf_bypass_base).await?;
        current_url = next_url;
    }

    Ok(out)
}

/// 模式 1: 把 `nextPage` 选中的 option/链接里的 `value`/`href` 全当分页 URL。
/// 返回 `Some(urls)` 表示命中模式 1; `Ok(None)` 表示需要走模式 2 (递归翻页)。
pub(super) fn collect_option_pagination_urls(
    first_html: &str,
    first_url: &str,
    toc_base_uri: &str,
    next_page_query: &str,
) -> Result<Option<Vec<String>>, TocError> {
    let document = Html::parse_document(first_html);
    let sel = Selector::parse(next_page_query).map_err(|e| {
        TocError::Parse(format!("无效的 nextPage 选择器 `{next_page_query}`: {e:?}"))
    })?;

    let elements: Vec<scraper::ElementRef<'_>> = document.select(&sel).collect();
    let base = resolve_base_for_join(toc_base_uri, first_url);

    let any_value = elements.iter().any(|e| e.value().attr("value").is_some());
    if !any_value {
        return Ok(None);
    }
    let attr_key = if elements.iter().all(|e| e.value().attr("href").is_none()) {
        "value"
    } else {
        "href"
    };
    let mut out = Vec::new();
    for e in &elements {
        if let Some(raw) = e.value().attr(attr_key)
            && let Some(abs) = abs_url(&base, raw)
        {
            out.push(abs);
        }
    }
    Ok(Some(out))
}

pub(super) async fn fetch_with_cf_fallback(
    client: &Client,
    url: &str,
    timeout: Option<u32>,
    cf_bypass_base: Option<&str>,
) -> Result<String, TocError> {
    crate::http::fetch_with_cf_fallback(client, url, timeout, cf_bypass_base)
        .await
        .map_err(|e| match e {
            crate::http::CfFallbackError::Http(msg) => TocError::Http(msg),
            crate::http::CfFallbackError::Cloudflare(final_url) => TocError::Cloudflare(final_url),
        })
}
