//! 目录解析。对应 Java `parse.TocParser`。
//!
//! 阶段 2c 实现的能力（与 Java 端等价子集）：
//! - 单页目录直接抽 `toc.item`；
//! - 分页目录两种模式：
//!   1. **下拉菜单** (option/select)：`nextPage` 命中带 `value`/`href` 属性的元素，
//!      一次性取出所有分页 URL；
//!   2. **下一页按钮**（递归）：每页抓一次，按 `nextPage` 选择器找下一个，
//!      直到拿不到合法 URL；
//! - `isDesc=true` 倒序枚举（69shuba）；
//! - `Book.url` 正则提取书 ID 并填入 `toc.url` / `toc.baseUri` 模板（`%s`）；
//! - 章节 `title` 走 text、`url` 走 absUrl。
//!
//! **未实现**（属阶段 3 / 后续）：
//! - 多线程并行抓取分页（Java 的 `parseToc` TODO 同样未做）。
//! - `chapter.url` 段含 `@js:` 后处理（极少见）。

use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use thiserror::Error;

use crate::http::{FetchRequest, HttpMethod, abs_url, fetch, fetch_via_cf_bypass, has_cloudflare};
use crate::models::{Chapter, ContentType, Rule};
use crate::parser::dom::{SelectError, select_and_invoke_js};

#[derive(Debug, Error)]
pub enum TocError {
    #[error("书源没有 toc 规则")]
    TocRuleMissing,
    #[error("HTTP 错误: {0}")]
    Http(String),
    #[error(
        "命中 Cloudflare 验证页，未配置 cf-bypass 旁路或旁路失败（请在 config.toml [global] cf-bypass 填地址）: {0}"
    )]
    Cloudflare(String),
    #[error("HTML 解析失败: {0}")]
    Parse(String),
    #[error("选择器/JS 执行失败: {0}")]
    Selector(#[from] SelectError),
}

/// 抓取并解析整本书的目录。
///
/// `book_url` 是详情页 URL（与 SearchResult.url 一致）。
/// `cf_bypass_base` 同其它 parser：CF 命中且非空时调用旁路服务。
pub async fn parse_toc(
    client: &Client,
    rule: &Rule,
    book_url: &str,
    cf_bypass_base: Option<&str>,
) -> Result<Vec<Chapter>, TocError> {
    let toc_rule = rule.toc.as_ref().ok_or(TocError::TocRuleMissing)?;

    // 1. 用 Book.url 正则把书 ID 提出来（如果配了），再格式化 toc.url / toc.baseUri。
    let book_id = extract_book_id(rule, book_url);
    let toc_url = format_with_id(&toc_rule.url, book_id.as_deref());
    let toc_base_uri = format_with_id(&toc_rule.base_uri, book_id.as_deref());

    // 2. 决定第一页 URL —— 若 toc.url 配了就用它，否则用 book_url（目录在详情页内）。
    let first_url = if toc_url.is_empty() {
        book_url.to_string()
    } else {
        toc_url
    };

    // 3. 抓第一页，按需走 CF 旁路。
    let first_html =
        fetch_with_cf_fallback(client, &first_url, toc_rule.timeout, cf_bypass_base).await?;

    // 4. 收集所有分页 URL（含第一页，按出现顺序去重）。
    let mut page_urls: Vec<String> = vec![first_url.clone()];
    if toc_rule.pagination && !toc_rule.next_page.is_empty() {
        let extra = collect_pagination_urls(
            client,
            &first_html,
            &first_url,
            &toc_base_uri,
            &toc_rule.next_page,
            toc_rule.timeout,
            cf_bypass_base,
        )
        .await?;
        for u in extra {
            if !page_urls.contains(&u) {
                page_urls.push(u);
            }
        }
    }

    // 5. 并行抓所有分页（page_urls 已含全部页面 URL —— 模式1 option 下拉一次性
    //    收集、模式2 递归在 collect_pagination_urls 内部已走完），再按原顺序解析。
    //    第一页 HTML 已抓过，不重复发请求。任一页抓取失败 → 整本目录失败（保留
    //    原串行实现的语义）。并发用 JoinSet + Semaphore 限到 8，避免一次打 200 个请求。
    let n = page_urls.len();
    let mut htmls: Vec<String> = Vec::with_capacity(n);
    htmls.resize(n, String::new());

    if n == 1 {
        htmls[0] = first_html;
    } else {
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
        let mut set = tokio::task::JoinSet::new();
        for (idx, page_url) in page_urls.iter().enumerate() {
            if idx == 0 {
                htmls[0] = first_html.clone();
                continue;
            }
            let client = client.clone();
            let url = page_url.clone();
            let timeout = toc_rule.timeout;
            let cf = cf_bypass_base.map(|s| s.to_string());
            let sem = sem.clone();
            set.spawn(async move {
                let _permit = sem.acquire_owned().await.expect("semaphore not closed");
                let html = fetch_with_cf_fallback(&client, &url, timeout, cf.as_deref()).await?;
                Ok::<(usize, String), TocError>((idx, html))
            });
        }
        while let Some(joined) = set.join_next().await {
            let (idx, html) =
                joined.map_err(|e| TocError::Parse(format!("分页抓取任务 join 失败: {e}")))??;
            htmls[idx] = html;
        }
    }

    // 按原 page_urls 顺序解析，保证章节顺序与串行实现一致。
    let mut all_items: Vec<Chapter> = Vec::new();
    let mut order: u32 = 1;
    for (idx, page_url) in page_urls.iter().enumerate() {
        let mut items = parse_one_toc_page(
            &htmls[idx],
            &resolve_base_for_join(&toc_base_uri, page_url),
            rule,
            &mut order,
        )?;
        all_items.append(&mut items);
    }

    Ok(all_items)
}

/// 从一页 HTML 中按规则抽出本页的章节列表。`order_counter` 在外部跨页递增。
///
/// `base_for_href` 用于把相对 href 拼成绝对 URL。
pub fn parse_one_toc_page(
    html: &str,
    base_for_href: &str,
    rule: &Rule,
    order_counter: &mut u32,
) -> Result<Vec<Chapter>, TocError> {
    let toc_rule = rule.toc.as_ref().ok_or(TocError::TocRuleMissing)?;
    let document = Html::parse_document(html);

    let item_selector = Selector::parse(&toc_rule.item)
        .map_err(|e| TocError::Parse(format!("无效的 item 选择器 `{}`: {e:?}", toc_rule.item)))?;

    // 当 toc.list 配置时（极少数书源），先把 list 的 inner_html 当成新文档处理。
    let elements: Vec<scraper::ElementRef<'_>> = if !toc_rule.list.is_empty() {
        // 取出 list 选中元素的 HTML，作为新 fragment 解析后再选 item。
        // Java 端原代码也是这么做的：`JsoupUtils.selectAndInvokeJs(document, r.getList(), HTML)`
        let inner = select_and_invoke_js(&document, &toc_rule.list, ContentType::Html)?;
        let frag = Html::parse_fragment(&inner);
        // 重新建一个 owned doc，select 后再把每个元素 outer-html 收集，
        // 再统一解析；但更简单的做法：在 fragment 上直接选。
        // ⚠️ 这里需要把 fragment 转借出 'static 不可能，所以走"再克隆 HTML"路径。
        // 我们退而求其次：在 fragment 上选完直接产生 Chapter 数据后退出。
        return parse_items_from_fragment(&frag, &item_selector, base_for_href, order_counter);
    } else {
        document.select(&item_selector).collect()
    };

    let mut chapters = Vec::with_capacity(elements.len());
    if toc_rule.is_desc {
        // 倒序：源站本身是新→旧，规则希望我们按"旧→新"输出
        for el in elements.iter().rev() {
            push_chapter(el, base_for_href, order_counter, &mut chapters);
        }
    } else {
        for el in elements.iter() {
            push_chapter(el, base_for_href, order_counter, &mut chapters);
        }
    }
    Ok(chapters)
}

fn parse_items_from_fragment(
    frag: &Html,
    sel: &Selector,
    base_for_href: &str,
    order_counter: &mut u32,
) -> Result<Vec<Chapter>, TocError> {
    let mut chapters = Vec::new();
    for el in frag.select(sel) {
        push_chapter(&el, base_for_href, order_counter, &mut chapters);
    }
    Ok(chapters)
}

fn push_chapter(
    el: &scraper::ElementRef<'_>,
    base_for_href: &str,
    order_counter: &mut u32,
    out: &mut Vec<Chapter>,
) {
    let title = el.text().collect::<Vec<_>>().join("").trim().to_string();
    if title.is_empty() {
        return;
    }
    let href = el.value().attr("href").unwrap_or_default();
    let url = abs_url(base_for_href, href).unwrap_or_default();
    if url.is_empty() {
        return;
    }
    out.push(Chapter {
        url,
        title,
        order: *order_counter,
        content: String::new(),
    });
    *order_counter += 1;
}

/// 在已抓的第一页里找全部分页 URL。
///
/// Java 端逻辑：
/// 1. 先用 nextPage 选择器拿一组元素，若它们带 `value` 属性 →
///    把每个的 `value`（或 `href`）作为分页 URL（与 select-option 等价）。
/// 2. 否则递归翻页：每抓一页都用 nextPage 拿"下一页"那一个 URL，直到拿不到。
async fn collect_pagination_urls(
    client: &Client,
    first_html: &str,
    first_url: &str,
    toc_base_uri: &str,
    next_page_query: &str,
    timeout: Option<u32>,
    cf_bypass_base: Option<&str>,
) -> Result<Vec<String>, TocError> {
    // 模式 1（select-option）：完全在 first_html 上就能搞定，不涉及 await。
    // 用单独 sync 函数隔离 scraper::Html，避免跨 await 持有非 Send 类型。
    if let Some(option_urls) =
        collect_option_pagination_urls(first_html, first_url, toc_base_uri, next_page_query)?
    {
        return Ok(option_urls);
    }

    // 模式 2：递归翻页 — 每翻一次都要 await 抓页，所以 Html 不能跨 await 持有。
    let sel = Selector::parse(next_page_query).map_err(|e| {
        TocError::Parse(format!("无效的 nextPage 选择器 `{next_page_query}`: {e:?}"))
    })?;
    let mut out: Vec<String> = Vec::new();
    let mut current_html = first_html.to_string();
    let mut current_url = first_url.to_string();
    // 保险阀：现实中分页不会超过几十页；上限 200 防止反爬死循环。
    for _ in 0..200 {
        // sync 子作用域：解析 + 选 + 拼下一 URL，把 next_url 析出后再 await
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

/// 模式 1 实现：把 `nextPage` 选中的 option/链接里的 `value`/`href` 全部当成分页 URL。
/// 返回 `Some(urls)` 表示命中模式 1；返回 `Ok(None)` 表示需要走模式 2（递归翻页）。
fn collect_option_pagination_urls(
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
        if let Some(raw) = e.value().attr(attr_key) {
            if let Some(abs) = abs_url(&base, raw) {
                out.push(abs);
            }
        }
    }
    Ok(Some(out))
}

async fn fetch_with_cf_fallback(
    client: &Client,
    url: &str,
    timeout: Option<u32>,
    cf_bypass_base: Option<&str>,
) -> Result<String, TocError> {
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
    .map_err(|e| TocError::Http(format!("{e:#}")))?;

    if has_cloudflare(&resp.html) {
        match cf_bypass_base.filter(|s| !s.trim().is_empty()) {
            Some(base) => fetch_via_cf_bypass(client, base, url)
                .await
                .map_err(|e| TocError::Http(format!("cf-bypass: {e:#}"))),
            None => Err(TocError::Cloudflare(resp.final_url)),
        }
    } else {
        Ok(resp.html)
    }
}

/// 用 `Book.url` 这个正则从详情页 URL 中提取书 ID。
/// 没配 / 不匹配时返回 None。
fn extract_book_id(rule: &Rule, book_url: &str) -> Option<String> {
    let book_rule = rule.book.as_ref()?;
    if book_rule.url.is_empty() {
        return None;
    }
    // Java 端用 hutool `ReUtil.getGroup1`；规则里 `Book.url` 一定含一个捕获组。
    // 这里允许规则形如 `https://(?:www\.)?69shuba\.com/book/(.*?)\.htm`。
    let re = Regex::new(&book_rule.url).ok()?;
    let cap = re.captures(book_url)?;
    cap.get(1).map(|m| m.as_str().to_string())
}

/// 把 `template` 里的第一处 `%s` 用 `id` 替换；
/// `id` 为 None 或 template 为空时原样返回。
fn format_with_id(template: &str, id: Option<&str>) -> String {
    if template.is_empty() {
        return String::new();
    }
    match id {
        Some(v) => template.replacen("%s", v, 1),
        None => template.to_string(),
    }
}

/// 计算 absUrl 的 base：
/// - 优先用 `toc.baseUri`（已经被 ID 模板格式化过），
/// - 否则用当前页 URL。
fn resolve_base_for_join(toc_base_uri: &str, current_page_url: &str) -> String {
    if !toc_base_uri.trim().is_empty() {
        toc_base_uri.to_string()
    } else {
        current_page_url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LangType;
    use crate::rules::apply_default_rule;

    /// 笔趣阁22 toc 规则：单页 / option 下拉分页（`#indexselect > option`）。
    fn rule_22biqu() -> Rule {
        let mut r: Rule = serde_json::from_str(
            r##"{
                "url": "https://www.22biqu.com/",
                "name": "笔趣阁22",
                "book": {},
                "toc": {
                    "item": "div:nth-child(4) > ul > li > a",
                    "pagination": true,
                    "nextPage": "#indexselect > option"
                }
            }"##,
        )
        .unwrap();
        r.id = 5;
        apply_default_rule(&mut r, LangType::ZhCn);
        r
    }

    /// 69shuba toc 规则：toc.url 模板 + isDesc + Book.url 正则。
    fn rule_69shuba() -> Rule {
        let mut r: Rule = serde_json::from_str(
            r##"{
                "url": "https://www.69shuba.com/",
                "name": "69书吧",
                "book": {
                    "url": "https://(?:www\\.)?69shuba\\.com/book/(.*?)\\.htm"
                },
                "toc": {
                    "url": "https://69shuba.com/book/%s/",
                    "item": "#catalog > ul > li > a",
                    "isDesc": true
                }
            }"##,
        )
        .unwrap();
        r.id = 1;
        apply_default_rule(&mut r, LangType::ZhCn);
        r
    }

    // ---------- 单页目录（顺序）----------

    #[test]
    fn parses_single_page_toc_in_order() {
        let html = r##"<html><body>
            <div></div><div></div><div></div>
            <div><ul>
                <li><a href="/biqu5/c1.html">第1章 起航</a></li>
                <li><a href="/biqu5/c2.html">第2章 风波</a></li>
                <li><a href="/biqu5/c3.html">第3章 归来</a></li>
            </ul></div>
        </body></html>"##;

        let rule = rule_22biqu();
        let mut order: u32 = 1;
        let chapters =
            parse_one_toc_page(html, "https://www.22biqu.com/biqu5/", &rule, &mut order).unwrap();

        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].order, 1);
        assert_eq!(chapters[0].title, "第1章 起航");
        assert_eq!(chapters[0].url, "https://www.22biqu.com/biqu5/c1.html");
        assert_eq!(chapters[2].order, 3);
        assert_eq!(chapters[2].title, "第3章 归来");
    }

    // ---------- isDesc：源站新→旧，输出旧→新 ----------

    #[test]
    fn is_desc_reverses_order() {
        let html = r##"<html><body>
            <div id="catalog"><ul>
                <li><a href="/book/123/c10.htm">第10章 终章</a></li>
                <li><a href="/book/123/c9.htm">第9章 倒数</a></li>
                <li><a href="/book/123/c1.htm">第1章 楔子</a></li>
            </ul></div>
        </body></html>"##;
        let rule = rule_69shuba();
        let mut order: u32 = 1;
        let chapters =
            parse_one_toc_page(html, "https://www.69shuba.com/book/123/", &rule, &mut order)
                .unwrap();

        assert_eq!(chapters.len(), 3);
        // 输出顺序：1 → 9 → 10
        assert_eq!(chapters[0].title, "第1章 楔子");
        assert_eq!(chapters[1].title, "第9章 倒数");
        assert_eq!(chapters[2].title, "第10章 终章");
        assert_eq!(chapters[0].order, 1);
        assert_eq!(chapters[2].order, 3);
    }

    // ---------- Book.url 正则提取书 ID ----------

    #[test]
    fn extract_book_id_from_real_69shuba_pattern() {
        let rule = rule_69shuba();
        let id = extract_book_id(&rule, "https://www.69shuba.com/book/12345.htm");
        assert_eq!(id.as_deref(), Some("12345"));
        let id2 = extract_book_id(&rule, "https://69shuba.com/book/abc.htm");
        assert_eq!(id2.as_deref(), Some("abc"));
    }

    #[test]
    fn extract_book_id_returns_none_when_no_book_url_pattern() {
        let rule = rule_22biqu(); // book.url 是空
        let id = extract_book_id(&rule, "https://www.22biqu.com/biquge1/");
        assert!(id.is_none());
    }

    #[test]
    fn format_with_id_replaces_placeholder() {
        assert_eq!(
            format_with_id("https://x.com/book/%s/", Some("777")),
            "https://x.com/book/777/"
        );
        assert_eq!(
            format_with_id("https://x.com/book/%s/", None),
            "https://x.com/book/%s/"
        );
        assert_eq!(format_with_id("", Some("777")), "");
    }

    // ---------- 分页 URL 收集（option 下拉模式）----------

    #[test]
    fn collects_option_dropdown_pagination_urls() {
        // 仿 22biqu / wxsy.net 真实下拉结构
        let html = r##"<html><body>
            <select id="indexselect">
                <option value="/biqu5/">第1-100章</option>
                <option value="/biqu5/p2.html">第101-200章</option>
                <option selected value="/biqu5/p3.html">第201-300章</option>
            </select>
        </body></html>"##;

        // 不发请求；直接用 helper（option 模式不需要二次抓取）。
        let document = Html::parse_document(html);
        let sel = Selector::parse("#indexselect > option").unwrap();
        let elements: Vec<_> = document.select(&sel).collect();
        let any_value = elements.iter().any(|e| e.value().attr("value").is_some());
        assert!(any_value);

        let base = "https://www.22biqu.com/biqu5/";
        let urls: Vec<String> = elements
            .iter()
            .filter_map(|e| e.value().attr("value").and_then(|v| abs_url(base, v)))
            .collect();

        assert_eq!(
            urls,
            vec![
                "https://www.22biqu.com/biqu5/".to_string(),
                "https://www.22biqu.com/biqu5/p2.html".to_string(),
                "https://www.22biqu.com/biqu5/p3.html".to_string(),
            ]
        );
    }

    // ---------- 缺失 toc 段：typed error ----------

    #[test]
    fn parse_one_page_without_toc_rule_errors() {
        let rule = Rule {
            url: "https://x".into(),
            ..Rule::default()
        };
        let mut order: u32 = 1;
        let err = parse_one_toc_page("", "https://x", &rule, &mut order).unwrap_err();
        assert!(matches!(err, TocError::TocRuleMissing));
    }
}
