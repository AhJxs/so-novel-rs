//! HTML 转换工具
//!
//! 来自原 `parser/dom.rs`, 关注"清属性 / 删标签"两种 HTML 转换。
//! 选择器 + @js 后处理在 [`super::selector`]。

use regex::Regex;
use scraper::{Html, Selector};
use std::sync::LazyLock;

/// 清除所有元素的属性。Java `JsoupUtils.clearAllAttributes`。
/// 用途: 正文 HTML 在写入模板前, 去掉所有 class/style/id 等属性,
/// 避免被书源植入的 CSS 隐藏正文。
///
/// 实现: 用正则把每个开标签里 `<tag ...>` 中的属性段去掉, 保留 `<tag>`
/// 与 `<tag/>` (自闭合)。这比走 DOM API 更轻、且不会被 scraper
/// 重新规整化 (rewrap into <html><body>) 影响。
///
/// # Examples
///
/// ```
/// use so_novel_rs::parser::dom::clear_all_attributes;
/// let html = r#"<div class="hide" style="display:none"><p>正文</p></div>"#;
/// let cleaned = clear_all_attributes(html);
/// assert!(!cleaned.contains("class="));
/// assert!(cleaned.contains("正文"));
/// ```
///
/// # Panics
///
/// 若 `OPEN_TAG` 静态正则字面量改坏 (group 数量不再为 2) 会在 closure 内
/// panic; 这意味着 regex 修改者在第一处替换处就能定位。
pub fn clear_all_attributes(html: &str) -> String {
    /// 编译期确定的正则：用 match + panic 避免 `clippy::expect_used`。
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

    static OPEN_TAG: LazyLock<Regex> = LazyLock::new(|| {
        // 匹配 <tag ...> 或 <tag .../>; 标签名不含 `/`, 且不在 `<!`、`</` 开头处启动。
        compile_static_re(r"<([A-Za-z][A-Za-z0-9]*)\b[^>]*?(/?)>")
    });

    OPEN_TAG
        .replace_all(html, |caps: &regex::Captures<'_>| {
            // OPEN_TAG regex `<([A-Za-z][A-Za-z0-9]*)\b[^>]*?(/?)>` 固定两个 group。
            // 改 regex 时必须保持 2 个 group；这里用 `match` 把不可能的 miss
            // 显式 panic, 让未来改动能精确定位。
            // panic IS the design：regex 改变未同步更新 group 数量时立即炸出来。
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
/// 实现: 用 scraper 选中目标节点, 记录其在原始字符串中的"开始位置"
/// 与"完整外层 HTML", 然后再原文里把它整段删除。这样不丢失原文里
/// 的格式 (不会被 scraper 的序列化吃掉空白、或包出 `<html><body>`)。
///
/// # Examples
///
/// ```
/// use so_novel_rs::parser::dom::remove_tags;
/// let html = "<p>x</p><script>bad()</script><p>y</p>";
/// let out = remove_tags(html, "script");
/// assert!(out.contains("x"));
/// assert!(!out.contains("bad()"));
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

    // 把所有要删除节点的 outer-HTML 收集起来, 按长度降序删 (先删长的, 避免短串误伤)
    let mut victims: Vec<String> = Vec::new();
    for sel in &selectors {
        for el in doc.select(sel) {
            victims.push(el.html());
        }
    }
    victims.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let mut out = html.to_string();
    for v in victims {
        // 用 String::replace 直接做一次性替换。重复出现也会全部删掉,
        // 与 jsoup 的 select+remove 语义吻合。
        out = out.replace(&v, "");
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

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
        // <div><p><div>deep</div></p></div> — 两个 div 都删, p 保留
        let html = "<div><p><div>deep</div></p></div>";
        let out = remove_tags(html, "div");
        assert!(!out.contains("deep"), "deep div not removed: {out}");
        // p 的开闭标签可能还在 (取决于 outer HTML 替换顺序)
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
