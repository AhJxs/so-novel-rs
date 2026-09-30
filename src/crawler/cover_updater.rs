//! 3 站 `CoverUpdater` —— 对应 Java `core.CoverUpdater`。
//!
//! 起点详情页抽出的 `coverUrl` 常是 150×200 占位小图; 这里并行去 起点 / 纵横 / 七猫 三个"原始书源"按书名+作者
//! 搜同一条, 各抽候选封面 URL, 再并发 GET 字节测 `width*height`, 挑最大的作为最终 cover。
//!
//! 关键差异: 起点需要 `Cookie: <user_pasted>` 才能拿到未登录态被截断的搜索结果 (整段粘贴, 原样当 `Cookie:` 头; 空则跳过该站),
//! 纵横 / 七猫 是公开搜索 API **不附 Cookie**, 封面字节下载也**不附 Cookie**。触发点在 `parser::book::parse_book_detail` 末尾
//! (`!rule.need_proxy` 时); 失败策略 **soft-skip**: 3 站都没拿到有效候选时原样返回详情页的 `coverUrl` (空则 `DEFAULT_COVER`), 不会让整本解析失败。

use std::io::Cursor;
use std::time::Duration;

use image::ImageReader;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use tracing::warn;
use url::Url;

use crate::http::ua::random_ua;
use crate::models::Book;

/// 起点 CDN 的占位默认封面，规则里没给 coverUrl / `CoverUpdater` 全部失败时用。
const DEFAULT_COVER: &str = "https://bookcover.yuewen.com/qdbimg/no-cover";

/// 单次抓取超时；比 Java 端的 3s 放宽到 5s，给慢站一些缓冲。
const TIMEOUT_SECS: u64 = 5;

/// 入口：3 站 fan-out 找更高清封面，返回最佳 URL。
///
/// `fallback_cover_url` 是详情页抽出的原始 cover URL；3 站都没拿到有效候选时返回 fallback
/// （fallback 自身为空则用 `DEFAULT_COVER`）。**永远返回非空 String**，调用方无脑赋值给 `book.cover_url` 即可。
///
/// # Examples
///
/// ```ignore
/// let cover = fetch_cover(&client, &book, book.cover_url.as_deref(), qidian_cookie).await;
/// book.cover_url = Some(cover);
/// ```
#[tracing::instrument(
    name = "fetch_cover",
    skip_all,
    fields(
        book = %book.book_name,
        author = %book.author,
        has_qidian_cookie = !qidian_cookie.trim().is_empty(),
    )
)]
pub async fn fetch_cover(
    client: &Client,
    book: &Book,
    fallback_cover_url: Option<&str>,
    qidian_cookie: &str,
) -> String {
    let started = std::time::Instant::now();
    let fallback = normalize_fallback(fallback_cover_url);
    let qidian_enabled = !qidian_cookie.trim().is_empty();

    tracing::debug!(book = %book.book_name, author = %book.author, qidian_enabled = qidian_enabled, "CoverUpdater: 启动 3 站 fan-out");

    if book.book_name.trim().is_empty() {
        // 书名空 → 无匹配依据，3 站都跑也是浪费。
        tracing::debug!(book = %book.book_name, "CoverUpdater: 书名空，跳过所有源");
        return fallback;
    }

    // cookie 为空时 qidian 退化为 None（不附 Cookie 拿不到结果，不如不跑）。
    let cookie_trim = qidian_cookie.trim();
    let (qd, zh, qm) = tokio::join!(
        async {
            if cookie_trim.is_empty() {
                tracing::debug!("CoverUpdater: 起点 cookie 空，跳过");
                None
            } else {
                let r = fetch_qidian(client, book, cookie_trim).await;
                tracing::debug!(hit = r.is_some(), "CoverUpdater: 起点结果");
                r
            }
        },
        async {
            let r = fetch_zongheng(client, book).await;
            tracing::debug!(hit = r.is_some(), "CoverUpdater: 纵横结果");
            r
        },
        async {
            let r = fetch_qimao(client, book).await;
            tracing::debug!(hit = r.is_some(), "CoverUpdater: 七猫结果");
            r
        },
    );

    // 收集候选 + 测分辨率（同步串行，避免 3 张大图同时打满带宽）。
    let mut best: Option<(String, u64)> = None;
    let mut measured = 0usize;
    let candidates: [Option<String>; 3] = (qd, zh, qm).into();
    for url in candidates.into_iter().flatten() {
        if !is_valid_cover(&url) {
            continue;
        }
        if let Some(area) = measure_resolution(client, &url).await {
            measured += 1;
            if best.as_ref().is_none_or(|(_, a)| area > *a) {
                best = Some((url.clone(), area));
            }
            tracing::debug!(url = %url, area = area, "CoverUpdater: 候选测得分辨率");
        } else {
            warn!("CoverUpdater: {} 下载/解码失败", url);
        }
    }

    let had_fanout = best.is_some();
    let chosen = best.map_or_else(|| fallback.clone(), |(u, _)| u);
    let source_label = if had_fanout { "fanout" } else { "fallback" };
    tracing::info!(
        book = %book.book_name,
        candidates = measured,
        chosen = source_label,
        url = %chosen,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "CoverUpdater: 完成",
    );
    chosen
}

/// 兜底 URL 标准化：None / 空 → `DEFAULT_COVER`，否则 trim 后原样返回。
fn normalize_fallback(raw: Option<&str>) -> String {
    let s = raw.unwrap_or("").trim();
    if s.is_empty() {
        DEFAULT_COVER.to_string()
    } else {
        s.to_string()
    }
}

/// URL 合法性：能 parse 才算合法。空串 / 非法字符直接 false。
fn is_valid_cover(url: &str) -> bool {
    if url.trim().is_empty() {
        return false;
    }
    Url::parse(url).is_ok()
}

/// 起点：用 cookie 拿搜索结果，从 `.res-book-item` 抽 cover URL，
/// 去 `/150(\.webp)?` 缩略图后缀拿全分辨率。
async fn fetch_qidian(client: &Client, book: &Book, cookie: &str) -> Option<String> {
    let url = format!("https://www.qidian.com/so/{}.html", book.book_name);
    let resp = client
        .get(&url)
        .header(reqwest::header::USER_AGENT, random_ua())
        .header(reqwest::header::COOKIE, cookie)
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await
        .ok()?;
    let body = resp.text().await.ok()?;
    let doc = Html::parse_document(&body);
    let sel = Selector::parse(".res-book-item").ok()?;
    for el in doc.select(&sel) {
        let name = extract_text(&el, ".book-mid-info > .book-info-title > a");
        let author1 = extract_text(&el, ".book-mid-info > .author > .name");
        let author2 = extract_text(&el, ".book-mid-info > .author > i");
        let author = if author1.is_empty() { author2 } else { author1 };
        if match_book(book, &name, &author) {
            let cover = extract_attr(&el, ".book-img-box > a > img", "src");
            if cover.is_empty() {
                continue;
            }
            // Java 端 `URLUtil.normalize` + `replaceAll("/150(\\.webp)?", "")` 的等价物
            let normalized = cover.replace("/150.webp", "").replace("/150", "");
            return Some(normalized);
        }
    }
    None
}

/// 纵横：GET `search.zongheng.com`，form 字段当 query string（Hutool 同款）。响应
/// `data.datas.list[].{name, authorName, coverUrl}`，`coverUrl` 是相对路径，要拼 `https://static.zongheng.com/upload` 前缀。
async fn fetch_zongheng(client: &Client, book: &Book) -> Option<String> {
    let form = [
        ("keyword", book.book_name.as_str()),
        ("pageNo", "1"),
        ("pageNum", "20"),
        ("isFromHuayu", "0"),
    ];
    let resp = client
        .get("https://search.zongheng.com/search/book")
        .header(reqwest::header::USER_AGENT, random_ua())
        .query(&form)
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await
        .ok()?;
    let body = resp.text().await.ok()?;
    let v: Value = serde_json::from_str(&body).ok()?;
    let list = v.get("data")?.get("datas")?.get("list")?.as_array()?;
    for item in list {
        let name = item.get("name")?.as_str()?;
        let author = item.get("authorName")?.as_str()?;
        if match_book(book, name, author) {
            let cover = item.get("coverUrl")?.as_str()?;
            return Some(format!("https://static.zongheng.com/upload{cover}"));
        }
    }
    None
}

/// 七猫：GET `qimao.com/qimaoapi/api/search/result`，form 当 query。
/// 响应 `data.search_list[].{title, author, image_link}`，`image_link` 已经是完整 URL。
async fn fetch_qimao(client: &Client, book: &Book) -> Option<String> {
    let form = [
        ("keyword", book.book_name.as_str()),
        ("count", "0"),
        ("page", "1"),
        ("page_size", "15"),
    ];
    let resp = client
        .get("https://www.qimao.com/qimaoapi/api/search/result")
        .header(reqwest::header::USER_AGENT, random_ua())
        .query(&form)
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await
        .ok()?;
    let body = resp.text().await.ok()?;
    let v: Value = serde_json::from_str(&body).ok()?;
    let list = v.get("data")?.get("search_list")?.as_array()?;
    for item in list {
        let name = item.get("title")?.as_str()?;
        let author = item.get("author")?.as_str()?;
        if match_book(book, name, author) {
            return item
                .get("image_link")?
                .as_str()
                .map(std::string::ToString::to_string);
        }
    }
    None
}

/// 同步从选中元素抽文本（多 text node 自动拼）。`Selector` 解析失败 → 空串。
fn extract_text(el: &scraper::ElementRef, sel: &str) -> String {
    let Ok(s) = Selector::parse(sel) else {
        return String::new();
    };
    el.select(&s)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string())
        .unwrap_or_default()
}

/// 同步从选中元素抽属性值。`Selector` 解析失败 / 元素无该属性 → 空串。
fn extract_attr(el: &scraper::ElementRef, sel: &str, attr: &str) -> String {
    let Ok(s) = Selector::parse(sel) else {
        return String::new();
    };
    el.select(&s)
        .next()
        .and_then(|e| e.value().attr(attr))
        .unwrap_or("")
        .to_string()
}

/// 简化版同名同作者匹配（都先 `strip_tags` 防 HTML 标签污染）。
///
/// Java 端先用 `HanLP` 转简体再比；我们按 `==` 直比 —— 99% 情况源站搜索结果与详情页来源一致，
/// 出现简繁漏匹配再扩（`util::zhconv::convert_text` 已有）。
fn match_book(book: &Book, name: &str, author: &str) -> bool {
    let src_name = book.book_name.trim();
    let src_author = book.author.trim();
    if src_name.is_empty() || src_author.is_empty() {
        return false;
    }
    let clean_name = strip_tags(name);
    let clean_author = strip_tags(author);
    src_name == clean_name && src_author == clean_author
}

/// 简易 HTML 标签剥离：去掉 `<...>` 包夹区段；不做实体解码（书名作者里基本没有 `&xxx;`）。
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// 裸 GET 下载封面字节（**不附 Cookie** —— 起点封面 CDN 用 token 鉴权，不在 Cookie 头里），
/// `image::ImageReader` 解码后算 `width * height`；任一步失败返回 None。
async fn measure_resolution(client: &Client, url: &str) -> Option<u64> {
    let resp = client
        .get(url)
        .header(reqwest::header::USER_AGENT, random_ua())
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .send()
        .await
        .ok()?;
    let bytes = resp.bytes().await.ok()?;
    let img = ImageReader::new(Cursor::new(bytes.as_ref()))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    Some((img.width() as u64) * (img.height() as u64))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::config::AppConfig;
    use crate::http::client::{ClientOptions, build_async_client};

    fn book(name: &str, author: &str) -> Book {
        Book {
            book_name: name.into(),
            author: author.into(),
            ..Book::default()
        }
    }

    #[test]
    fn normalize_fallback_uses_default_when_none() {
        assert_eq!(normalize_fallback(None), DEFAULT_COVER);
    }

    #[test]
    fn normalize_fallback_uses_default_when_empty() {
        assert_eq!(normalize_fallback(Some("")), DEFAULT_COVER);
        assert_eq!(normalize_fallback(Some("   ")), DEFAULT_COVER);
    }

    #[test]
    fn normalize_fallback_passes_through() {
        assert_eq!(
            normalize_fallback(Some("https://x.com/c.jpg")),
            "https://x.com/c.jpg"
        );
    }

    #[test]
    fn is_valid_cover_rejects_garbage() {
        assert!(!is_valid_cover(""));
        assert!(!is_valid_cover("not a url"));
        assert!(!is_valid_cover("   "));
    }

    #[test]
    fn is_valid_cover_accepts_https() {
        assert!(is_valid_cover("https://example.com/c.jpg"));
        assert!(is_valid_cover("http://x.com/c.png"));
    }

    #[test]
    fn match_book_exact_match() {
        let b = book("软件工程师", "苹果");
        assert!(match_book(&b, "软件工程师", "苹果"));
    }

    #[test]
    fn match_book_different_name_returns_false() {
        let b = book("软件工程师", "苹果");
        assert!(!match_book(&b, "硬件工程师", "苹果"));
    }

    #[test]
    fn match_book_different_author_returns_false() {
        let b = book("软件工程师", "苹果");
        assert!(!match_book(&b, "软件工程师", "梨"));
    }

    #[test]
    fn match_book_strips_html_tags() {
        let b = book("软件工程师", "苹果");
        assert!(match_book(&b, "<em>软件工程师</em>", "<b>苹果</b>"));
    }

    #[test]
    fn match_book_empty_source_returns_false() {
        let b = book("", "苹果");
        assert!(!match_book(&b, "x", "苹果"));
    }

    #[test]
    fn strip_tags_basic() {
        assert_eq!(strip_tags("hello"), "hello");
        assert_eq!(strip_tags("<b>x</b>"), "x");
        assert_eq!(strip_tags("a<b>b</b>c"), "abc");
        assert_eq!(strip_tags("<a href='x'>link</a>"), "link");
    }

    #[tokio::test]
    async fn fetch_cover_returns_fallback_when_bookname_empty() {
        let cfg = AppConfig::default();
        let client = build_async_client(&cfg, &ClientOptions::default()).unwrap();
        let b = book("", "");
        let got = fetch_cover(&client, &b, Some("https://orig.com/c.jpg"), "").await;
        assert_eq!(got, "https://orig.com/c.jpg");
    }

    #[tokio::test]
    async fn fetch_cover_empty_bookname_falls_back_to_default() {
        let cfg = AppConfig::default();
        let client = build_async_client(&cfg, &ClientOptions::default()).unwrap();
        let b = book("", "");
        let got = fetch_cover(&client, &b, None, "").await;
        assert_eq!(got, DEFAULT_COVER);
    }

    /// cookie 空 + 正常书名：qidian 不跑，纵横/七猫 联网失败 → 走 fallback（live 网络测试，`#[ignore]`）。
    #[tokio::test]
    #[ignore = "live network: depends on zongheng / qimao availability"]
    async fn fetch_cover_returns_fallback_when_all_sites_fail() {
        let cfg = AppConfig::default();
        let client = build_async_client(&cfg, &ClientOptions::default()).unwrap();
        let b = book("xxx_definitely_not_exists_zzz", "xxx");
        let got = fetch_cover(&client, &b, Some("https://orig.com/c.jpg"), "").await;
        // 3 站都失败 → 走 fallback（这里给的就是原 URL）
        assert_eq!(got, "https://orig.com/c.jpg");
    }
}
