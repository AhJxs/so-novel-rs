//! 选择器封装 + @js: 后处理: 选元素 + 抽内容 + 可选 JS 后处理。HTML 转换在 [`super::transform`]。

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
/// 返回值约定: 选不到元素返回空字符串; 多个元素按 `ContentType` 聚合
/// (text 空格连接 / html 拼接 / attr 取首个); 含 `@js:` 交给 JS 引擎;
/// 含 `@href` / `@src` 后缀则改抽对应属性。
///
/// # Examples
///
/// ```ignore
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
            // 只取原始 attr 值; 与 jsoup `absUrl` 等价的拼绝对路径由 parser 层
            // 拿到 baseUri 后用 `url::Url::join` 完成。该臂只覆盖 attr 类型,
            // `attr_name()` 对非 attr 变体返回 "", 这里调用永远合法。
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
/// 规则作者可以写 `#info > a@href` 表示"取 href 属性而非文本"。
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

/// 极小 `XPath` → CSS 改写。只覆盖现有规则出现过的两类:
///
/// 1. `//*[@id="readbg"]/script[4]` → `#readbg > script:nth-of-type(4)`
///    (id 索引 `XPath`; id 允许单/双引号, 尾部 `[N]` 可选)。
/// 2. 纯绝对路径标签序列 `/html/body/div` → `html > body > div`。
///    每段必须是不带 `*` / 属性 / 谓词的纯标签名, 否则放弃改写。
///
/// 引入完整 `XPath` 引擎的成本远高于改写这几条规则, 故其它 `XPath` 一律返回 `None`。
fn xpath_to_css(s: &str) -> Option<String> {
    use regex::Regex;
    use std::sync::LazyLock;

    /// 编译期确定的正则：用 match + panic 避免 `clippy::expect_used`，与项目里
    /// 其它 `LazyLock` 静态正则统一风格。
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
        // //*[@id="readbg"]/script[4]; 尾部 [N] 可选 (无则不指定 nth-of-type)
        compile_static_re(
            r#"^//\*\[@id\s*=\s*["']([^"']+)["']\]\s*/\s*([A-Za-z][A-Za-z0-9_-]*)\s*(?:\[(\d+)\])?$"#,
        )
    });
    let s = s.trim();

    if let Some(cap) = RE.captures(s) {
        // regex 是 `^...$` 锚定的字面量: match 成功时 group 1/2/3 一定存在。
        // 改 regex 时必须同步更新下面的 group 访问, 否则这里立刻 panic 定位。
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

    // 纯绝对路径: 每段必须是纯标签名 (无 `*`/属性/谓词)
    if s.starts_with('/') && !s.starts_with("//") {
        let segments: Vec<&str> = s.split('/').filter(|seg| !seg.is_empty()).collect();
        if !segments.is_empty() && segments.iter().all(|seg| is_plain_tag_name(seg)) {
            return Some(segments.join(" > "));
        }
    }

    None
}

/// 是否是纯标签名 (如 `html` / `body` / `div-1`)。带 `*`、属性、谓词 `[N]` 的不算。
fn is_plain_tag_name(seg: &str) -> bool {
    !seg.is_empty()
        && seg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && seg.as_bytes()[0].is_ascii_alphabetic()
}

/// 把 `selector_part` 标准化为 CSS: 已是 CSS 原样返回; 是已知极小 `XPath` 模式
/// 则改写; 其它 `XPath` 返回 `Err` 让上层报 `XPathNotSupported`。
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

    #[test]
    fn applies_js_after_select() {
        let h = doc(r#"<html><body><div class="a">作者：苹果</div></body></html>"#);
        let q = r".a@js:r=r.replace('作者：','')";
        let s = select_and_invoke_js(&h, q, ContentType::Text).unwrap();
        assert_eq!(s, "苹果");
    }

    #[test]
    fn applies_js_concat_pattern_from_real_rule() {
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
        let h = doc(r#"<html><body>
                <div id="readbg">
                    <script>var a = 1;</script>
                    <script>var b = 2;</script>
                    <script>var c = 3;</script>
                    <script>var nextpage = "/n/123/2.html";</script>
                </div>
            </body></html>"#);
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

    #[test]
    fn parses_real_chapter_html_resource() {
        use scraper::Selector;
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("web")
            .join("chapter.html");
        let html = std::fs::read_to_string(&path).unwrap();
        let h = doc(&html);

        let title = dom_select_text(&h, "h1", ContentType::Text).unwrap();
        assert!(title.contains("穿越成皇"), "title: {title}");

        let p_sel = Selector::parse("p").unwrap();
        let count = h.select(&p_sel).count();
        assert!(count >= 4, "p count: {count}");
    }
}
