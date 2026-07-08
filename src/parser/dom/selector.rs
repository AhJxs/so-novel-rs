//! 选择器封装 + @js: 后处理
//!
//! 来自原 `parser/dom.rs`, 关注"选元素 + 抽内容 + 可选 JS 后处理"。
//! HTML 转换 (`clear_all_attributes` / `remove_tags`) 在 [`super::transform`]。

use std::fmt;

use scraper::{ElementRef, Html};
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
/// 返回值约定:
/// - 选不到任何元素时返回空字符串 (与 Java 端一致);
/// - 多个元素: 按 `ContentType` 聚合 (text 用空格连接、html 拼接、attr 取首个);
/// - 含 `@js:` 时把抽取结果交给 JS 引擎处理后返回;
/// - 含 `@href` / `@src` 后缀时自动切换到对应属性抽取模式。
///
/// # Examples
///
/// ```ignore
/// use crate::parser::dom::select_and_invoke_js;
/// use crate::models::ContentType;
/// let html = scraper::Html::parse_document(r#"<div class="a">作者: 苹果</div>"#);
/// let s = select_and_invoke_js(&html, r#".a@js:r=r.replace('作者: ','')"#, ContentType::Text).unwrap();
/// assert_eq!(s, "苹果");
/// ```
///
/// # Errors
///
/// - `SelectError::BadSelector` — 无效 CSS
/// - `SelectError::XPathNotSupported` — 极小改写未覆盖的 `XPath`
/// - `SelectError::JsFailed` — `@js:` 后处理执行失败
pub fn select_and_invoke_js(
    document: &Html,
    query: &str,
    content_type: ContentType,
) -> Result<String, SelectError> {
    select_and_invoke_js_impl(query, content_type, |sel, ct| {
        dom_select_text(document, sel, ct)
    })
}

/// 同上, 但作用于已选中的 `ElementRef` (嵌套查询场景, 例如搜索结果列表里
/// 对每条 result 元素再选 bookName/author 等子字段)。
pub fn select_and_invoke_js_within(
    el: ElementRef<'_>,
    query: &str,
    content_type: ContentType,
) -> Result<String, SelectError> {
    select_and_invoke_js_impl(query, content_type, |sel, ct| {
        element_select_text(el, sel, ct)
    })
}

/// 共享逻辑: 剥离后缀 → 拆 JS → 选择器归一化 → 抽取 → 可选 JS 后处理。
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

/// 仅做选择 + 内容抽取, 不做 JS 后处理。
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
            // 与 jsoup `Elements.text()` 行为一致: 拼接每个元素的全文本, 空格分隔
            let parts: Vec<String> = els
                .iter()
                .map(|e| e.text().collect::<Vec<_>>().join("").trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            parts.join(" ")
        }
        ContentType::Html => els
            .iter()
            .map(scraper::ElementRef::inner_html)
            .collect::<String>(),
        ContentType::AttrSrc | ContentType::AttrHref => {
            // 与 jsoup `absUrl(attrName)` 等价的实现需要文档 baseUri;
            // 阶段 2a 这里只取原始 attr 值, 把"absUrl"工作交给 parser 层
            // 自己做 (parser 拿到 baseUri 后再用 url::Url::join 解析)。
            // 该 match 臂只覆盖 attr 类型 — `attr_name()` 对非 attr 变体返回 "",
            // 在这里调用永远是合法的 src/href。
            let attr = content_type.attr_name();
            els.iter()
                .find_map(|e| e.value().attr(attr))
                .unwrap_or("")
                .to_string()
        }
        ContentType::AttrContent | ContentType::AttrValue => {
            let attr = content_type.attr_name();
            els.iter()
                .find_map(|e| e.value().attr(attr))
                .unwrap_or("")
                .to_string()
        }
    }
}

/// 剥离查询末尾的 `@href` / `@src` 后缀, 并据此覆盖 `content_type`。
///
/// Java 端 `JsoupUtils.stripAt()` + `BookParser.getContentType()` 的等价实现。
/// 规则作者可以写 `#info > a@href` 来表示"取 href 属性而非文本"。
fn strip_at_suffix(query: &str, ct: ContentType) -> (&str, ContentType) {
    query.strip_suffix("@href").map_or_else(
        || {
            query
                .strip_suffix("@src")
                .map_or((query, ct), |q| (q.trim_end(), ContentType::AttrSrc))
        },
        |q| (q.trim_end(), ContentType::AttrHref),
    )
}

/// 拆 query 里 `<sel>@js:<body>` 这两段。
pub fn split_js(query: &str) -> (&str, Option<&str>) {
    query.find("@js:").map_or((query, None), |idx| {
        (&query[..idx], Some(&query[idx + 4..]))
    })
}

fn is_xpath(s: &str) -> bool {
    s.starts_with('/') || s.starts_with("//") || s.starts_with("(/")
}

/// 极小 `XPath` → CSS 改写。覆盖现有规则中出现过的两类 `XPath`:
///
/// 1. `//*[@id="readbg"]/script[4]` → `#readbg > script:nth-of-type(4)`
///    (cloudflare.json 96 读书唯一一条 id 索引 `XPath`)。
/// 2. 纯绝对路径标签序列 `/html`、`/html/body`、`/html/body/div` …
///    → `html`、`html > body`、`html > body > div`
///    (main.json wxsy.net 的 `toc.list = "/html@js:..."`: 选中 `<html>` 根
///    元素, 把整个文档 `inner_html` 喂给 @js 后处理)。每一段必须是纯标签名,
///    不带 `*` / 属性 / 谓词 —— 出现任性片段就放弃, 交给上层报 typed error。
///
/// 引入完整 `XPath` 引擎 (libxml/sxd-xpath) 的成本远高于改写这几条规则,
/// 因此只覆盖以上两种精确模式; 其它 `XPath` 一律返回 `None`。
fn xpath_to_css(s: &str) -> Option<String> {
    use regex::Regex;
    use std::sync::LazyLock;

    /// 编译期确定的正则：用 match + panic 避免 `clippy::expect_used`，与项目里
    /// 其它 `LazyLock` 静态正则统一风格。
    /// panic IS the design：源码字面量写错就是程序员错误。
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

    static RE: LazyLock<Regex> = LazyLock::new(|| {
        // //*[@id="readbg"]/script[4]
        // 允许 id 用单或双引号; 尾部 [N] 可选 (无则不指定 nth-of-type)。
        compile_static_re(
            r#"^//\*\[@id\s*=\s*["']([^"']+)["']\]\s*/\s*([A-Za-z][A-Za-z0-9_-]*)\s*(?:\[(\d+)\])?$"#,
        )
    });
    let s = s.trim();

    if let Some(cap) = RE.captures(s) {
        // 该 regex 是 `^...$` 锚定的字面量: match 成功时 group 1/2/3 一定存在。
        // 静态 regex 模式不变时永远命中；这里把不可能的 miss 显式 panic，让未来
        // 改动 regex 时定位明确。
        // panic IS the design：regex 改变未同步更新下面的 group 访问时立即炸出来。
        #[allow(
            clippy::panic,
            reason = "regex match success guarantees group exists; panic = programmer error on regex change"
        )]
        let id = cap.get(1).map_or_else(
            || panic!("XPATH_RE group 1 (id) missing — 修改上方 regex 时必须保留"),
            |m| m.as_str(),
        );
        #[allow(
            clippy::panic,
            reason = "regex match success guarantees group exists; panic = programmer error on regex change"
        )]
        let tag = cap.get(2).map_or_else(
            || panic!("XPATH_RE group 2 (tag) missing — 修改上方 regex 时必须保留"),
            |m| m.as_str(),
        );
        let nth = cap.get(3).map(|m| m.as_str());
        return Some(nth.map_or_else(
            || format!("#{id} > {tag}"),
            |n| format!("#{id} > {tag}:nth-of-type({n})"),
        ));
    }

    // 纯绝对路径: `/tag/tag/...`, 每段是合法标签名 (无 `*`/属性/谓词)。
    if s.starts_with('/') && !s.starts_with("//") {
        let segments: Vec<&str> = s.split('/').filter(|seg| !seg.is_empty()).collect();
        if !segments.is_empty() && segments.iter().all(|seg| is_plain_tag_name(seg)) {
            return Some(segments.join(" > "));
        }
    }

    None
}

/// 是否是纯标签名 (如 `html` / `body` / `div-1`)。带 `*`、属性、谓词 `[N]`
/// 的不算 —— 那些需要更完整的 `XPath` 改写, 超出极小覆盖范围。
fn is_plain_tag_name(seg: &str) -> bool {
    !seg.is_empty()
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && seg.as_bytes()[0].is_ascii_alphabetic()
}

/// 把 `selector_part` 标准化为 CSS 选择器:
/// - 已经是 CSS: 原样返回;
/// - 是已知极小 `XPath` 模式 (`//*[@id=...]` 或纯绝对路径 `/html`、`/html/body`…):
///   改写为 CSS;
/// - 其它 `XPath`: 返回 `Err` 让上层报 `XPathNotSupported`。
fn normalize_selector(selector_part: &str) -> Result<String, SelectError> {
    if !is_xpath(selector_part) {
        return Ok(selector_part.to_string());
    }
    if let Some(css) = xpath_to_css(selector_part) {
        return Ok(css);
    }
    Err(SelectError::XPathNotSupported(selector_part.to_string()))
}

// 让 Display 友好一点
impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Text => "text",
                Self::Html => "html",
                Self::AttrSrc => "@src",
                Self::AttrHref => "@href",
                Self::AttrContent => "@content",
                Self::AttrValue => "@value",
            }
        )
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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
        let h = doc(r"<html><body><p>hi</p></body></html>");
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
        let q = r".a@js:r=r.replace('作者：','')";
        let s = select_and_invoke_js(&h, q, ContentType::Text).unwrap();
        assert_eq!(s, "苹果");
    }

    #[test]
    fn applies_js_concat_pattern_from_real_rule() {
        // 模拟 main.json mcxs 书源 coverUrl 规则:
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
        let q = r"/html/body/div[1]";
        let err = select_and_invoke_js(&h, q, ContentType::Text).unwrap_err();
        assert!(matches!(err, SelectError::XPathNotSupported(_)), "{err}");
    }

    #[test]
    fn xpath_id_indexed_pattern_is_rewritten_to_css() {
        // cloudflare.json `96读书` 唯一一条 XPath:
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
        // 直接通过 select_and_invoke_js 端到端验证:
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
        // main.json wxsy.net 的 toc.list = "/html@js:...": 选中 <html> 根元素,
        // 取 inner_html ( ContentType::Html ) 后交给 @js 后处理。
        // 这里端到端验证: /html 改写成 css `html`, 能取到文档 HTML。
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
        // 端到端: /html 选根 + @js 后处理 (模拟 wxsy.net 真实 list 规则的精简版)。
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
        let h = doc(r"<html><body><div><p>text</p></div></body></html>");
        let q = "/html/body/div";
        let s = select_and_invoke_js(&h, q, ContentType::Text).unwrap();
        assert_eq!(s, "text");
    }

    // ---------- 嵌套选择 (搜索结果场景) ----------

    #[test]
    fn within_element_select() {
        use scraper::Selector;
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

    // ---------- 真实测试资源 ----------

    #[test]
    fn parses_real_chapter_html_resource() {
        use scraper::Selector;
        // bundle/web/chapter.html 是一段真实章节页
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bundle")
            .join("web")
            .join("chapter.html");
        let html = std::fs::read_to_string(&path).unwrap();
        let h = doc(&html);

        let title = dom_select_text(&h, "h1", ContentType::Text).unwrap();
        assert!(title.contains("穿越成皇"), "title: {title}");

        // 段落数 ≥ 4 (资源里有多段 <p>)
        let p_sel = Selector::parse("p").unwrap();
        let count = h.select(&p_sel).count();
        assert!(count >= 4, "p count: {count}");
    }
}
