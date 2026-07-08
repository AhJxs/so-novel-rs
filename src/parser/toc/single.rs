//! 目录解析主流程 + 单页抽取 (PR #17 拆分, 2026-07-08).
//!
//! 来自原 `parser/toc.rs`:
//! - [`parse_toc`] 公共入口: 抓分页 + 抽章节
//! - [`parse_one_toc_page`] 从一页 HTML 抽章节 (含 is_desc 倒序逻辑)
//! - [`parse_items_from_fragment`] / [`push_chapter`] 内部 helper
//!
//! 分页收集在 [`super::paginated`], 工具 + TocError 在 [`super::utils`]。

use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};

use crate::http::abs_url;
use crate::models::{Chapter, ContentType, Rule};
use crate::parser::dom::select_and_invoke_js;

use super::paginated::{collect_pagination_urls, fetch_with_cf_fallback};
use super::utils::{TocError, extract_book_id, format_with_id, resolve_base_for_join};

/// 抓取并解析整本书的目录。
///
/// `book_url` 是详情页 URL (与 SearchResult.url 一致)。
/// `cf_bypass_base` 同其它 parser: CF 命中且非空时调用旁路服务。
///
/// # Examples
///
/// ```ignore
/// let chapters = parse_toc(&client, &rule, &book_url, cf_bypass).await?;
/// println!("共 {} 章", chapters.len());
/// ```
///
/// # Errors
///
/// - `TocError::TocRuleMissing` — 规则没有 `toc` 段
/// - `TocError::Http` / `TocError::Cloudflare` — 抓取失败
/// - `TocError::Parse` / `TocError::Selector` — HTML 解析失败
#[tracing::instrument(
    name = "parse_toc",
    skip_all,
    fields(
        source_id = rule.id,
        source = %rule.name,
        %book_url,
    )
)]
pub async fn parse_toc(
    client: &Client,
    rule: &Rule,
    book_url: &str,
    cf_bypass_base: Option<&str>,
) -> Result<Vec<Chapter>, TocError> {
    let toc_rule = rule.toc.as_ref().ok_or(TocError::TocRuleMissing)?;

    // 1. 用 Book.url 正则把书 ID 提出来 (如果配了), 再格式化 toc.url / toc.baseUri。
    let book_id = extract_book_id(rule, book_url);
    let toc_url = format_with_id(&toc_rule.url, book_id.as_deref());
    let toc_base_uri = format_with_id(&toc_rule.base_uri, book_id.as_deref());

    // 2. 决定第一页 URL — 若 toc.url 配了就用它, 否则用 book_url (目录在详情页内)。
    let first_url = if toc_url.is_empty() {
        book_url.to_string()
    } else {
        toc_url
    };

    // 3. 抓第一页, 按需走 CF 旁路。
    let first_html =
        fetch_with_cf_fallback(client, &first_url, toc_rule.timeout, cf_bypass_base).await?;

    // 4. 收集所有分页 URL (含第一页, 按出现顺序去重)。
    let mut page_urls: Vec<String> = vec![first_url.clone()];
    if !toc_rule.next_page.is_empty() {
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

    // 5. 并行抓所有分页 (page_urls 已含全部页面 URL — 模式1 option 下拉一次性
    //    收集、模式2 递归在 collect_pagination_urls 内部已走完), 再按原顺序解析。
    //    第一页 HTML 已抓过, 不重复发请求。任一页抓取失败 → 整本目录失败 (保留
    //    原串行实现的语义)。并发用 JoinSet + Semaphore 限到 8, 避免一次打 200 个请求。
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
                // `acquire_owned` 在 Semaphore 不被 close 的情况下永远成功;
                // 防御性: 万一未来切换实现 / close 信号进来, 转换成 TocError::Parse
                // 让上层知道分页抓取出问题, 而不是 panic 把整个目录解析任务搞炸。
                let _permit = match sem.acquire_owned().await {
                    Ok(p) => p,
                    Err(e) => {
                        return Err(TocError::Parse(format!(
                            "分页抓取 semaphore acquire 失败: {e}"
                        )));
                    }
                };
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

    // 按原 page_urls 顺序解析, 保证章节顺序与串行实现一致。
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
///
/// # Examples
///
/// ```ignore
/// let mut order = 1u32;
/// let chapters = parse_one_toc_page(&html, "https://x.com/book/", &rule, &mut order)?;
/// ```
///
/// # Errors
///
/// - `TocError::TocRuleMissing` — 规则没有 `toc` 段
/// - `TocError::Selector` — item 选择器无效
pub fn parse_one_toc_page(
    html: &str,
    base_for_href: &str,
    rule: &Rule,
    order_counter: &mut u32,
) -> Result<Vec<Chapter>, TocError> {
    let toc_rule = rule.toc.as_ref().ok_or(TocError::TocRuleMissing)?;
    let document = Html::parse_document(html);

    let item_selector = crate::parser::cache::cached_selector(&toc_rule.item)
        .map_err(|e| TocError::Parse(format!("无效的 item 选择器 `{}`: {e:?}", toc_rule.item)))?;

    // 当 toc.list 配置时 (极少数书源), 先把 list 的 inner_html 当成新文档处理。
    let elements: Vec<scraper::ElementRef<'_>> = if !toc_rule.list.is_empty() {
        // 取出 list 选中元素的 HTML, 作为新 fragment 解析后再选 item。
        // Java 端原代码也是这么做的: `JsoupUtils.selectAndInvokeJs(document, r.getList(), HTML)`
        let inner = select_and_invoke_js(&document, &toc_rule.list, ContentType::Html)?;
        let frag = Html::parse_fragment(&inner);
        // 重新建一个 owned doc, select 后再把每个元素 outer-html 收集,
        // 再统一解析; 但更简单的做法: 在 fragment 上直接选。
        // ⚠️ 这里需要把 fragment 转借出 'static 不可能, 所以走"再克隆 HTML"路径。
        // 我们退而求其次: 在 fragment 上选完直接产生 Chapter 数据后退出。
        return parse_items_from_fragment(&frag, &item_selector, base_for_href, order_counter);
    } else {
        document.select(&item_selector).collect()
    };

    let mut chapters = Vec::with_capacity(elements.len());
    if toc_rule.is_desc {
        // 倒序: 源站本身是新→旧, 规则希望我们按"旧→新"输出
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
    sel: &std::sync::Arc<Selector>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LangType;
    use crate::db::apply_default_rule;

    /// 笔趣阁22 toc 规则: 单页 / option 下拉分页 (`#indexselect > option`)。
    fn rule_22biqu() -> Rule {
        let mut r: Rule = serde_json::from_str(
            r##"{
                "url": "https://www.22biqu.com/",
                "name": "笔趣阁22",
                "book": {},
                "toc": {
                    "item": "div:nth-child(4) > ul > li > a",
                    "nextPage": "#indexselect > option"
                }
            }"##,
        )
        .unwrap();
        r.id = 5;
        apply_default_rule(&mut r, LangType::ZhCn);
        r
    }

    /// 69shuba toc 规则: toc.url 模板 + isDesc + Book.url 正则。
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

    // ---------- 单页目录 (顺序) ----------

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

    // ---------- isDesc: 源站新→旧, 输出旧→新 ----------

    #[test]
    fn is_desc_reverses_order() {
        let html = r##"<html><body>
            <div id="catalog"><ul>
                <li><a href="/book/123/c10.htm">第10章 终章</a></li>
                <li><a href="/book/123/c9.htm">第9章 倒数</a></li>
                <li><a href="/book/123/c1.htm">第1章 楔子</a></li>
            </ul></div></body></html>"##;
        let rule = rule_69shuba();
        let mut order: u32 = 1;
        let chapters =
            parse_one_toc_page(html, "https://www.69shuba.com/book/123/", &rule, &mut order)
                .unwrap();

        assert_eq!(chapters.len(), 3);
        // 输出顺序: 1 → 9 → 10
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

    // ---------- 分页 URL 收集 (option 下拉模式) ----------

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

        // 不发请求; 直接用 helper (option 模式不需要二次抓取)。
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

    // ---------- 缺失 toc 段: typed error ----------

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

    // ---------- resolve_base_for_join ----------

    #[test]
    fn resolve_base_prefers_toc_base_uri() {
        let base = resolve_base_for_join("https://example.com/base/", "https://other.com/page");
        assert_eq!(base, "https://example.com/base/");
    }

    #[test]
    fn resolve_base_falls_back_to_current_page_url() {
        let base = resolve_base_for_join("", "https://other.com/page");
        assert_eq!(base, "https://other.com/page");
    }

    #[test]
    fn resolve_base_whitespace_only_toc_uri_falls_back() {
        let base = resolve_base_for_join("   ", "https://other.com/page");
        assert_eq!(base, "https://other.com/page");
    }

    #[test]
    fn resolve_base_preserves_untrimmed_toc_uri() {
        // 代码 trim 检查但返回原串 — 记录这个行为
        let base = resolve_base_for_join("  https://x.com/  ", "https://y.com/");
        assert_eq!(base, "  https://x.com/  ");
    }
}
