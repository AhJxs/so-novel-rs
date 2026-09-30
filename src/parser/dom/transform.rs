//! HTML 转换工具: 清属性 / 删标签。选择器 + @js 后处理在 [`super::selector`]。

use regex::Regex;
use scraper::{Html, Selector};
use std::sync::LazyLock;

/// 清除所有元素的属性。Java `JsoupUtils.clearAllAttributes`。
/// 用途: 正文 HTML 在写入模板前, 去掉所有 class/style/id 等属性,
/// 避免被书源植入的 CSS 隐藏正文。
///
/// 实现: 正则去掉开标签里的属性段, 保留 `<tag>` 与 `<tag/>`。
/// 不走 DOM API 是因为 scraper 会重新包出 `<html><body>`。
///
/// # Examples
///
/// ```ignore
/// use so_novel_rs::parser::dom::clear_all_attributes;
/// let cleaned = clear_all_attributes(r#"<div class="hide"><p>正文</p></div>"#);
/// ```
///
/// # Panics
///
/// 若 `OPEN_TAG` 静态正则字面量改坏 (group 数量不再为 2) 会在 closure 内
/// panic; 这意味着 regex 修改者在第一处替换处就能定位。
pub fn clear_all_attributes(html: &str) -> String {
    /// 编译期确定的正则：用 match + panic 避免 `clippy::expect_used`。
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

    static OPEN_TAG: LazyLock<Regex> = LazyLock::new(|| {
        // 匹配 <tag ...> 或 <tag .../>; 标签名不含 `/`, 且不在 `<!`、`</` 开头处启动。
        compile_static_re(r"<([A-Za-z][A-Za-z0-9]*)\b[^>]*?(/?)>")
    });

    OPEN_TAG
        .replace_all(html, |caps: &regex::Captures<'_>| {
            // OPEN_TAG 固定两个 group；改 regex 时必须同步保留 group 数量，
            // 否则 get() 返回 None 会在这里立刻 panic 定位。
            #[allow(clippy::panic, reason = "regex match success guarantees group exists; panic = programmer error on regex change")]
            let name = caps
                .get(1)
                .map_or_else(|| panic!("OPEN_TAG group 1 (tag name) missing — 修改 regex 时必须保留"), |m| m.as_str());
            #[allow(clippy::panic, reason = "regex match success guarantees group exists; panic = programmer error on regex change")]
            let slash = caps.get(2).map_or_else(
                || panic!("OPEN_TAG group 2 (self-close slash) missing — 修改 regex 时必须保留"),
                |m| m.as_str(),
            );
            format!("<{name}{slash}>")
        })
        .into_owned()
}

/// 移除匹配 css 选择器的标签。Java `JsoupUtils.removeTags`。
/// 用于 chapter.filterTag 配置, 例如把广告 div 整段删掉。
///
/// 实现: 用 scraper 选中节点后, 记录其 outer HTML, 再在**原始字符串**里整段删掉。
/// 不走序列化输出的原因: 那会吃掉空白并包出 `<html><body>`。
///
/// # Examples
///
/// ```ignore
/// use so_novel_rs::parser::dom::remove_tags;
/// let out = remove_tags("<p>x</p><script>bad()</script>", "script");
/// ```
pub fn remove_tags(html: &str, css_query: &str) -> String {
    if html.is_empty() || css_query.trim().is_empty() {
        return html.to_string();
    }

    // 多个选择器以 `,` 分隔 (scraper 也支持 group selector, 但拆分后更稳)。
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

    // 按长度降序删 (先删长的, 避免短串误伤)
    let mut victims: Vec<String> = Vec::new();
    for sel in &selectors {
        for el in doc.select(sel) {
            victims.push(el.html());
        }
    }
    victims.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let mut out = html.to_string();
    for v in victims {
        out = out.replace(&v, "");
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

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
        let html = "<div><p><div>deep</div></p></div>";
        let out = remove_tags(html, "div");
        assert!(!out.contains("deep"), "deep div not removed: {out}");
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
}
