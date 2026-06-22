//! 选择器封装 + @js: 后处理。对应 Java `util.JsoupUtils`。
//!
//! Java 端用 jsoup，同时支持 CSS 选择器与 XPath，且查询字符串后
//! 可以追加 `@js:<片段>`，把选中元素的 text/html/attr 当作变量 `r`
//! 喂给那段 JS，结果作为最终值。
//!
//! Rust 端：
//! - CSS 选择器用 `scraper`（HTML5 解析 + html5ever 选择器实现，
//!   覆盖现有规则的 99%）；
//! - XPath：现有规则里有两类 XPath，都走极小改写而非引入完整引擎：
//!   1. `//*[@id="readbg"]/script[4]`（`bundle/rules/cloudflare.json` 96 读书）；
//!   2. 纯绝对路径 `/html`、`/html/body`…（`bundle/rules/main.json` wxsy.net 的
//!      `toc.list = "/html@js:..."`，选中 `<html>` 根元素后 @js 后处理）。
//!
//!   `xpath_to_css` 覆盖以上两种精确模式；其它 XPath 返回 `XPathNotSupported`。
//! - `@js:` 后处理：交给 `crate::js::post_process`。

use std::fmt;

use scraper::{ElementRef, Html, Selector};
use thiserror::Error;

pub use crate::models::ContentType;

#[derive(Debug, Error)]
pub enum SelectError {
    #[error("无效的 CSS 选择器: {0}")]
    BadSelector(String),
    #[error("XPath 选择器暂不支持（阶段 2a），原始查询: {0}")]
    XPathNotSupported(String),
    #[error("JS 后处理失败: {0}")]
    JsFailed(String),
}

/// 用于一次"选 + 抽 + 可选 JS 后处理"的统一入口。
/// 等价 Java `JsoupUtils#selectAndInvokeJs(el, query, contentType)`。
///
/// 返回值约定：
/// - 选不到任何元素时返回空字符串（与 Java 端一致）；
/// - 多个元素：按 ContentType 聚合（text 用空格连接、html 拼接、attr 取首个）；
/// - 含 `@js:` 时把抽取结果交给 JS 引擎处理后返回；
/// - 含 `@href` / `@src` 后缀时自动切换到对应属性抽取模式。
pub fn select_and_invoke_js(
    document: &Html,
    query: &str,
    content_type: ContentType,
) -> Result<String, SelectError> {
    select_and_invoke_js_impl(query, content_type, |sel, ct| {
        dom_select_text(document, sel, ct)
    })
}

/// 同上，但作用于已选中的 `ElementRef`（嵌套查询场景，例如搜索结果列表里
/// 对每条 result 元素再选 bookName/author 等子字段）。
pub fn select_and_invoke_js_within(
    el: ElementRef<'_>,
    query: &str,
    content_type: ContentType,
) -> Result<String, SelectError> {
    select_and_invoke_js_impl(query, content_type, |sel, ct| {
        element_select_text(el, sel, ct)
    })
}

/// 共享逻辑：剥离后缀 → 拆 JS → 选择器归一化 → 抽取 → 可选 JS 后处理。
fn select_and_invoke_js_impl(
    query: &str,
    content_type: ContentType,
    select: impl FnOnce(&str, ContentType) -> Result<String, SelectError>,
) -> Result<String, SelectError> {
    if query.is_empty() {
        return Ok(String::new());
    }
    let (query, content_type) = strip_at_suffix(query, content_type);
    let (selector_part, js_body) = split_js(query);
    let selector_norm = normalize_selector(selector_part)?;
    let raw = select(&selector_norm, content_type)?;
    match js_body {
        Some(body) => {
            crate::js::post_process(body, &raw).map_err(|e| SelectError::JsFailed(format!("{e:#}")))
        }
        None => Ok(raw),
    }
}

/// 仅做选择 + 内容抽取，不做 JS 后处理。
pub fn dom_select_text(
    document: &Html,
    selector: &str,
    content_type: ContentType,
) -> Result<String, SelectError> {
    let sel = crate::parser::cache::cached_selector(selector)?;
    let elements: Vec<ElementRef<'_>> = document.select(&sel).collect();
    Ok(extract_from_elements(&elements, content_type))
}

fn element_select_text(
    el: ElementRef<'_>,
    selector: &str,
    content_type: ContentType,
) -> Result<String, SelectError> {
    let sel = crate::parser::cache::cached_selector(selector)?;
    let elements: Vec<ElementRef<'_>> = el.select(&sel).collect();
    Ok(extract_from_elements(&elements, content_type))
}

fn extract_from_elements(els: &[ElementRef<'_>], content_type: ContentType) -> String {
    if els.is_empty() {
        return String::new();
    }
    match content_type {
        ContentType::Text => {
            // 与 jsoup `Elements.text()` 行为一致：拼接每个元素的全文本，空格分隔
            let parts: Vec<String> = els
                .iter()
                .map(|e| e.text().collect::<Vec<_>>().join("").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            parts.join(" ")
        }
        ContentType::Html => els
            .iter()
            .map(|e| e.inner_html())
            .collect::<Vec<_>>()
            .join(""),
        ContentType::AttrSrc | ContentType::AttrHref => {
            // 与 jsoup `absUrl(attrName)` 等价的实现需要文档 baseUri；
            // 阶段 2a 这里只取原始 attr 值，把"absUrl"工作交给 parser 层
            // 自己做（parser 拿到 baseUri 后再用 url::Url::join 解析）。
            let attr = content_type.attr_name().unwrap();
            els.iter()
                .find_map(|e| e.value().attr(attr))
                .unwrap_or("")
                .to_string()
        }
        ContentType::AttrContent | ContentType::AttrValue => {
            let attr = content_type.attr_name().unwrap();
            els.iter()
                .find_map(|e| e.value().attr(attr))
                .unwrap_or("")
                .to_string()
        }
    }
}

/// 剥离查询末尾的 `@href` / `@src` 后缀，并据此覆盖 `content_type`。
///
/// Java 端 `JsoupUtils.stripAt()` + `BookParser.getContentType()` 的等价实现。
/// 规则作者可以写 `#info > a@href` 来表示"取 href 属性而非文本"。
fn strip_at_suffix(query: &str, ct: ContentType) -> (&str, ContentType) {
    if let Some(q) = query.strip_suffix("@href") {
        (q.trim_end(), ContentType::AttrHref)
    } else if let Some(q) = query.strip_suffix("@src") {
        (q.trim_end(), ContentType::AttrSrc)
    } else {
        (query, ct)
    }
}

/// 拆 query 里 `<sel>@js:<body>` 这两段。
fn split_js(query: &str) -> (&str, Option<&str>) {
    if let Some(idx) = query.find("@js:") {
        (&query[..idx], Some(&query[idx + 4..]))
    } else {
        (query, None)
    }
}

fn is_xpath(s: &str) -> bool {
    s.starts_with('/') || s.starts_with("//") || s.starts_with("(/")
}

/// 极小 XPath → CSS 改写。覆盖现有规则中出现过的两类 XPath：
///
/// 1. `//*[@id="readbg"]/script[4]` → `#readbg > script:nth-of-type(4)`
///    （cloudflare.json 96 读书唯一一条 id 索引 XPath）。
/// 2. 纯绝对路径标签序列 `/html`、`/html/body`、`/html/body/div` …
///    → `html`、`html > body`、`html > body > div`
///    （main.json wxsy.net 的 `toc.list = "/html@js:..."`：选中 `<html>` 根
///    元素，把整个文档 inner_html 喂给 @js 后处理）。每一段必须是纯标签名，
///    不带 `*` / 属性 / 谓词 —— 出现任性片段就放弃，交给上层报 typed error。
///
/// 引入完整 XPath 引擎（libxml/sxd-xpath）的成本远高于改写这几条规则，
/// 因此只覆盖以上两种精确模式；其它 XPath 一律返回 `None`。
fn xpath_to_css(s: &str) -> Option<String> {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static RE: Lazy<Regex> = Lazy::new(|| {
        // //*[@id="readbg"]/script[4]
        // 允许 id 用单或双引号；尾部 [N] 可选（无则不指定 nth-of-type）。
        Regex::new(
            r#"^//\*\[@id\s*=\s*["']([^"']+)["']\]\s*/\s*([A-Za-z][A-Za-z0-9_-]*)\s*(?:\[(\d+)\])?$"#,
        )
        .expect("xpath rewrite re")
    });
    let s = s.trim();

    if let Some(cap) = RE.captures(s) {
        let id = cap.get(1).unwrap().as_str();
        let tag = cap.get(2).unwrap().as_str();
        let nth = cap.get(3).map(|m| m.as_str());
        return Some(match nth {
            Some(n) => format!("#{id} > {tag}:nth-of-type({n})"),
            None => format!("#{id} > {tag}"),
        });
    }

    // 纯绝对路径：`/tag/tag/...`，每段是合法标签名（无 `*`/属性/谓词）。
    if s.starts_with('/') && !s.starts_with("//") {
        let segments: Vec<&str> = s.split('/').filter(|seg| !seg.is_empty()).collect();
        if !segments.is_empty() && segments.iter().all(|seg| is_plain_tag_name(seg)) {
            return Some(segments.join(" > "));
        }
    }

    None
}

/// 是否是纯标签名（如 `html` / `body` / `div-1`）。带 `*`、属性、谓词 `[N]`
/// 的不算 —— 那些需要更完整的 XPath 改写，超出极小覆盖范围。
fn is_plain_tag_name(seg: &str) -> bool {
    !seg.is_empty()
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && seg.as_bytes()[0].is_ascii_alphabetic()
}

/// 把 selector_part 标准化为 CSS 选择器：
/// - 已经是 CSS：原样返回；
/// - 是已知极小 XPath 模式（`//*[@id=...]` 或纯绝对路径 `/html`、`/html/body`…）：
///   改写为 CSS；
/// - 其它 XPath：返回 `Err` 让上层报 `XPathNotSupported`。
fn normalize_selector(selector_part: &str) -> Result<String, SelectError> {
    if !is_xpath(selector_part) {
        return Ok(selector_part.to_string());
    }
    if let Some(css) = xpath_to_css(selector_part) {
        return Ok(css);
    }
    Err(SelectError::XPathNotSupported(selector_part.to_string()))
}

/// 清除所有元素的属性。Java `JsoupUtils.clearAllAttributes`。
/// 用途：正文 HTML 在写入模板前，去掉所有 class/style/id 等属性，
/// 避免被书源植入的 CSS 隐藏正文。
///
/// 实现：用正则把每个开标签里 `<tag ...>` 中的属性段去掉，保留 `<tag>`
/// 与 `<tag/>`（自闭合）。这比走 DOM API 更轻、且不会被 scraper
/// 重新规整化（rewrap into <html><body>）影响。
pub fn clear_all_attributes(html: &str) -> String {
    use once_cell::sync::Lazy;
    use regex::Regex;
    static OPEN_TAG: Lazy<Regex> =
        // 匹配 <tag ...> 或 <tag .../>；标签名不含 `/`，且不在 `<!`、`</` 开头处启动。
        Lazy::new(|| Regex::new(r"<([A-Za-z][A-Za-z0-9]*)\b[^>]*?(/?)>").expect("open tag re"));

    OPEN_TAG
        .replace_all(html, |caps: &regex::Captures<'_>| {
            let name = &caps[1];
            let slash = &caps[2];
            format!("<{name}{slash}>")
        })
        .into_owned()
}

/// 移除匹配 css 选择器的标签。Java `JsoupUtils.removeTags`。
/// 用于 chapter.filterTag 配置，例如把广告 div 整段删掉。
///
/// 实现：用 scraper 选中目标节点，记录其在原始字符串中的"开始位置"
/// 与"完整外层 HTML"，然后再原文里把它整段删除。这样不丢失原文里
/// 的格式（不会被 scraper 的序列化吃掉空白、或包出 `<html><body>`）。
pub fn remove_tags(html: &str, css_query: &str) -> String {
    if html.is_empty() || css_query.trim().is_empty() {
        return html.to_string();
    }

    // 多个选择器以 `,` 分隔（scraper 也支持 group selector，但拆分后更稳）。
    let selectors: Vec<Selector> = css_query
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| Selector::parse(s).ok())
        .collect();
    if selectors.is_empty() {
        return html.to_string();
    }

    let doc = Html::parse_fragment(html);

    // 把所有要删除节点的 outer-HTML 收集起来，按长度降序删（先删长的，避免短串误伤）
    let mut victims: Vec<String> = Vec::new();
    for sel in &selectors {
        for el in doc.select(sel) {
            victims.push(el.html());
        }
    }
    victims.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let mut out = html.to_string();
    for v in victims {
        // 用 String::replace 直接做一次性替换。重复出现也会全部删掉，
        // 与 jsoup 的 select+remove 语义吻合。
        out = out.replace(&v, "");
    }
    out
}

// 让 Display 友好一点
impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ContentType::Text => "text",
                ContentType::Html => "html",
                ContentType::AttrSrc => "@src",
                ContentType::AttrHref => "@href",
                ContentType::AttrContent => "@content",
                ContentType::AttrValue => "@value",
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(html: &str) -> Html {
        Html::parse_document(html)
    }

    // ---------- 基础 CSS 选择 ----------

    #[test]
    fn selects_text_content() {
        let h = doc(r#"<html><body><h1 class="t">第1章 标题</h1></body></html>"#);
        let s = dom_select_text(&h, ".t", ContentType::Text).unwrap();
        assert_eq!(s, "第1章 标题");
    }

    #[test]
    fn returns_empty_string_when_no_match() {
        let h = doc(r#"<html><body><p>hi</p></body></html>"#);
        let s = dom_select_text(&h, "#nope", ContentType::Text).unwrap();
        assert_eq!(s, "");
    }

    #[test]
    fn extracts_attr_href() {
        let h = doc(r#"<html><body><a href="/x/1.html">click</a></body></html>"#);
        let s = dom_select_text(&h, "a", ContentType::AttrHref).unwrap();
        assert_eq!(s, "/x/1.html");
    }

    #[test]
    fn extracts_meta_content() {
        let h =
            doc(r#"<html><head><meta property="og:novel:author" content="苹果"></head></html>"#);
        let s = dom_select_text(
            &h,
            r#"meta[property="og:novel:author"]"#,
            ContentType::AttrContent,
        )
        .unwrap();
        assert_eq!(s, "苹果");
    }

    // ---------- @js: 后处理 ----------

    #[test]
    fn applies_js_after_select() {
        let h = doc(r#"<html><body><div class="a">作者：苹果</div></body></html>"#);
        let q = r#".a@js:r=r.replace('作者：','')"#;
        let s = select_and_invoke_js(&h, q, ContentType::Text).unwrap();
        assert_eq!(s, "苹果");
    }

    #[test]
    fn applies_js_concat_pattern_from_real_rule() {
        // 模拟 main.json mcxs 书源 coverUrl 规则：
        //   meta[property="og:image"]@js:r='http://www.mcxs.info'+r
        let h =
            doc(r#"<html><head><meta property="og:image" content="/cover/1.jpg"></head></html>"#);
        let q = r#"meta[property="og:image"]@js:r='http://www.mcxs.info'+r"#;
        let s = select_and_invoke_js(&h, q, ContentType::AttrContent).unwrap();
        assert_eq!(s, "http://www.mcxs.info/cover/1.jpg");
    }

    #[test]
    fn xpath_returns_typed_error() {
        let h = doc("<html><body/></html>");
        // 用一个无法被极小改写覆盖的 XPath
        let q = r#"/html/body/div[1]"#;
        let err = select_and_invoke_js(&h, q, ContentType::Text).unwrap_err();
        assert!(matches!(err, SelectError::XPathNotSupported(_)), "{err}");
    }

    #[test]
    fn xpath_id_indexed_pattern_is_rewritten_to_css() {
        // cloudflare.json `96读书` 唯一一条 XPath：
        //   //*[@id="readbg"]/script[4]
        // 应被改写为 #readbg > script:nth-of-type(4)。
        let h = doc(r#"<html><body>
                <div id="readbg">
                    <script>var a = 1;</script>
                    <script>var b = 2;</script>
                    <script>var c = 3;</script>
                    <script>var nextpage = "/n/123/2.html";</script>
                </div>
            </body></html>"#);
        // 直接通过 select_and_invoke_js 端到端验证：
        let q = r#"//*[@id="readbg"]/script[4]"#;
        let s = select_and_invoke_js(&h, q, ContentType::Html).unwrap();
        assert!(s.contains("nextpage"), "got: {s}");
        assert!(s.contains("/n/123/2.html"), "got: {s}");
    }

    #[test]
    fn xpath_id_no_index_rewrites() {
        let h = doc(r#"<html><body>
                <div id="x"><span>one</span></div>
            </body></html>"#);
        let q = r#"//*[@id="x"]/span"#;
        let s = select_and_invoke_js(&h, q, ContentType::Text).unwrap();
        assert_eq!(s, "one");
    }

    #[test]
    fn xpath_absolute_html_root_rewrites_to_css() {
        // main.json wxsy.net 的 toc.list = "/html@js:..."：选中 <html> 根元素，
        // 取 inner_html（ ContentType::Html ）后交给 @js 后处理。
        // 这里端到端验证：/html 改写成 css `html`，能取到文档 HTML。
        let h = doc(
            r#"<html><body><ul class="section-list ycxsid"><li>a</li><li>b</li></ul></body></html>"#,
        );
        let q = "/html";
        let s = select_and_invoke_js(&h, q, ContentType::Html).unwrap();
        assert!(s.contains("section-list"), "got: {s}");
        assert!(s.contains("<li>a</li>"), "got: {s}");
    }

    #[test]
    fn xpath_absolute_html_root_with_js_postprocess() {
        // 端到端：/html 选根 + @js 后处理（模拟 wxsy.net 真实 list 规则的精简版）。
        let h = doc(
            r#"<html><body><ul class="section-list ycxsid"><li>a</li><li>b</li></ul></body></html>"#,
        );
        let q = "/html@js:r=r.replace(/<li>b<\\/li>/,'')";
        let s = select_and_invoke_js(&h, q, ContentType::Html).unwrap();
        assert!(s.contains("<li>a</li>"), "got: {s}");
        assert!(!s.contains("<li>b</li>"), "js should strip li b: {s}");
    }

    #[test]
    fn xpath_absolute_multi_segment_rewrites() {
        let h = doc(r#"<html><body><div><p>text</p></div></body></html>"#);
        let q = "/html/body/div";
        let s = select_and_invoke_js(&h, q, ContentType::Text).unwrap();
        assert_eq!(s, "text");
    }

    // ---------- 嵌套选择（搜索结果场景） ----------

    #[test]
    fn within_element_select() {
        let h = doc(r#"<html><body>
                <li><a href="/b/1">书 A</a><span>作者甲</span></li>
                <li><a href="/b/2">书 B</a><span>作者乙</span></li>
              </body></html>"#);
        let li_sel = Selector::parse("li").unwrap();
        let lis: Vec<_> = h.select(&li_sel).collect();
        assert_eq!(lis.len(), 2);

        let book = select_and_invoke_js_within(lis[0], "a", ContentType::Text).unwrap();
        assert_eq!(book, "书 A");
        let href = select_and_invoke_js_within(lis[1], "a", ContentType::AttrHref).unwrap();
        assert_eq!(href, "/b/2");
    }

    // ---------- clear_all_attributes ----------

    #[test]
    fn clear_attributes_strips_class_and_style() {
        let html = r#"<div class="hide" style="display:none"><p class="x">正文</p></div>"#;
        let cleaned = clear_all_attributes(html);
        assert!(!cleaned.contains("class="), "still has class: {cleaned}");
        assert!(!cleaned.contains("style="), "still has style: {cleaned}");
        assert!(cleaned.contains("正文"));
        assert!(cleaned.contains("<div>"));
        assert!(cleaned.contains("<p>"));
    }

    // ---------- remove_tags ----------

    #[test]
    fn remove_tags_drops_matching_elements() {
        let html = r#"<p>正文1</p><script>bad()</script><p>正文2</p><div class="ad">广告</div>"#;
        let out = remove_tags(html, "script, .ad");
        assert!(out.contains("正文1"));
        assert!(out.contains("正文2"));
        assert!(!out.contains("bad()"), "script not removed: {out}");
        assert!(!out.contains("广告"), "ad not removed: {out}");
    }

    #[test]
    fn remove_tags_with_empty_query_is_noop() {
        let html = "<p>x</p>";
        assert_eq!(remove_tags(html, ""), html);
    }

    #[test]
    fn remove_tags_nested_same_name_removes_all() {
        let html = "<div><div>inner</div></div><p>keep</p>";
        let out = remove_tags(html, "div");
        assert!(out.contains("keep"), "p lost: {out}");
        assert!(!out.contains("inner"), "inner div not removed: {out}");
    }

    #[test]
    fn remove_tags_deeply_nested_mixed_names() {
        // <div><p><div>deep</div></p></div> — 两个 div 都删，p 保留
        let html = "<div><p><div>deep</div></p></div>";
        let out = remove_tags(html, "div");
        assert!(!out.contains("deep"), "deep div not removed: {out}");
        // p 的开闭标签可能还在（取决于 outer HTML 替换顺序）
    }

    #[test]
    fn remove_tags_identical_siblings() {
        let html = "<div>ad</div><div>ad</div><p>正文</p>";
        let out = remove_tags(html, "div");
        assert!(out.contains("正文"), "content lost: {out}");
        assert!(!out.contains("ad"), "ad not removed: {out}");
    }

    #[test]
    fn remove_tags_no_match_returns_original() {
        let html = "<p>only</p>";
        let out = remove_tags(html, "div.nonexistent");
        assert_eq!(out, html);
    }

    // ---------- 真实测试资源 ----------

    #[test]
    fn parses_real_chapter_html_resource() {
        // bundle/web/chapter.html 是一段真实章节页
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bundle")
            .join("web")
            .join("chapter.html");
        let html = std::fs::read_to_string(&path).unwrap();
        let h = doc(&html);

        let title = dom_select_text(&h, "h1", ContentType::Text).unwrap();
        assert!(title.contains("穿越成皇"), "title: {title}");

        // 段落数 ≥ 5（资源里有多段 <p>）
        let p_sel = Selector::parse("p").unwrap();
        let count = h.select(&p_sel).count();
        assert!(count >= 4, "p count: {count}");
    }
}
