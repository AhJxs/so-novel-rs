//! 章节正文清洗。对应 Java `core.ChapterFilter`。纯函数, 无 IO。
//!
//! 顺序与 Java 端一致：1. 不可见字符（复用 `http::clean_invisible_chars`）; 2. HTML 实体:
//!    删 `&xxx;` 引用, 故意不转义以兼容 iBooks 等阅读器; 3. filterTxt 广告正则替换为空串
//!    —— 规则里偶有 Rust `regex` 不支持的语法, **编译失败时降级为不替换 + warn**, 不阻塞下载;
//! 4. filterTag 节点删除（复用 `parser::dom::remove_tags`）; 5. 正文开头的章节名擦掉,
//!    保留前面的 tag/whitespace; 6. `1.章节名` → `第1章 章节名`; 7. 清理空标签。

use regex::Regex;
use std::sync::LazyLock;

use crate::http::clean_invisible_chars;
use crate::models::{Chapter, RuleChapter};
use crate::parser::dom::remove_tags;

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

static HTML_ENTITY_RE: LazyLock<Regex> = LazyLock::new(|| compile_static_re(r"&[^;]+;"));
static EMPTY_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    // 反复运行直到稳定（嵌套空 tag）。每次匹配一个最内层的 <tag></tag>。
    compile_static_re(r"<([A-Za-z][A-Za-z0-9]*)\b[^>]*>\s*</\s*([A-Za-z][A-Za-z0-9]*)\s*>")
});
static TITLE_NUMBER_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_static_re(r"^(\d+)\s*\.\s*(.+)$"));

/// 清洗一个章节，返回新的 Chapter（不修改入参）。
///
/// `rule_chapter` 提供 filterTxt / filterTag 配置；其它字段来自入参 `chapter`。
///
/// # Panics
///
/// 若 `TITLE_NUMBER_RE` 静态正则字面量改坏 (捕获组数量不再为 2) 会在标题重写时
/// panic; regex 改动者立即定位。
pub fn filter_chapter(chapter: &Chapter, rule_chapter: &RuleChapter) -> Chapter {
    let mut content = chapter.content.clone();
    let mut title = chapter.title.clone();

    content = clean_invisible_chars(&content);

    content = HTML_ENTITY_RE.replace_all(&content, "").into_owned();

    if !rule_chapter.filter_txt.is_empty() {
        match crate::parser::cache::cached_regex(&rule_chapter.filter_txt) {
            Ok(re) => {
                content = re.replace_all(&content, "").into_owned();
            }
            Err(e) => {
                // 不支持的语法 → 跳过这一步而不是崩
                tracing::warn!(
                    "filterTxt 正则不被 Rust regex 支持，已跳过广告过滤；regex={}, err={}",
                    rule_chapter.filter_txt,
                    e
                );
            }
        }
    }

    // filterTag: 配置为 css 选择器
    if !rule_chapter.filter_tag.trim().is_empty() {
        content = remove_tags(&content, &rule_chapter.filter_tag);
    }

    content = strip_leading_title(&content, &title);

    if let Some(cap) = TITLE_NUMBER_RE.captures(&title) {
        // TITLE_NUMBER_RE 固定两个捕获组; match 成功时 group 1/2 一定存在。
        // 改 regex 时必须同步更新下面的 group 访问, 否则这里立刻 panic 定位。
        #[allow(
            clippy::panic,
            reason = "regex match success guarantees group exists; panic = programmer error on regex change"
        )]
        let n = cap.get(1).map_or_else(
            || panic!("TITLE_NUMBER_RE group 1 (number) missing — 修改上方 regex 时必须保留"),
            |m| m.as_str(),
        );
        #[allow(
            clippy::panic,
            reason = "regex match success guarantees group exists; panic = programmer error on regex change"
        )]
        let rest = cap.get(2).map_or_else(
            || panic!("TITLE_NUMBER_RE group 2 (rest) missing — 修改上方 regex 时必须保留"),
            |m| m.as_str(),
        );
        title = format!("第{n}章 {rest}");
    }

    content = strip_empty_tags(&content);

    Chapter {
        url: chapter.url.clone(),
        title,
        content,
        order: chapter.order,
    }
}

/// 删除正文开头处出现的章节标题。
///
/// `cleanBlank` 会把标题里所有空白删掉, 所以候选都是字面串, 用 `regex::escape` 处理。
fn strip_leading_title(content: &str, title: &str) -> String {
    if title.is_empty() {
        return content.to_string();
    }
    let title_compact: String = title.chars().filter(|c| !c.is_whitespace()).collect();

    let pat = if title == title_compact {
        format!("^((?:\\s|<[^>]+>)*)(?:{})", regex::escape(title))
    } else {
        format!(
            "^((?:\\s|<[^>]+>)*)(?:{}|{})",
            regex::escape(title),
            regex::escape(&title_compact)
        )
    };
    let Ok(re) = Regex::new(&pat) else {
        return content.to_string();
    };
    // replacen=1 等价 Java replaceFirst; 保留第 1 组（前面的空白/标签）
    re.replacen(content, 1, "$1").into_owned()
}

/// 反复清除空 tag（含嵌套）。等价 Java hutool `HtmlUtil.cleanEmptyTag` 的语义。
fn strip_empty_tags(html: &str) -> String {
    let mut prev = html.to_string();
    // 上限保险：现实中嵌套 ≤ 几层。
    for _ in 0..16 {
        let next = EMPTY_TAG_RE.replace_all(&prev, "").into_owned();
        if next == prev {
            return next;
        }
        prev = next;
    }
    prev
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn rule_with(filter_txt: &str, filter_tag: &str) -> RuleChapter {
        RuleChapter {
            filter_txt: filter_txt.to_string(),
            filter_tag: filter_tag.to_string(),
            ..RuleChapter::default()
        }
    }

    fn ch(title: &str, content: &str) -> Chapter {
        Chapter {
            url: "https://x/".into(),
            title: title.into(),
            content: content.into(),
            order: 1,
        }
    }

    #[test]
    fn removes_invisible_chars() {
        let r = rule_with("", "");
        let c = ch("第1章", "中\u{200B}文\u{FEFF}内容");
        let out = filter_chapter(&c, &r);
        assert_eq!(out.content, "中文内容");
    }

    #[test]
    fn removes_html_entities() {
        let r = rule_with("", "");
        let c = ch("第1章", "<p>段&nbsp;落&amp;一</p>");
        let out = filter_chapter(&c, &r);
        assert_eq!(out.content, "<p>段落一</p>");
    }

    #[test]
    fn applies_filter_txt_regex() {
        let r = rule_with(r"\(本章完\)", "");
        let c = ch("第1章", "<p>正文</p><p>(本章完)</p>");
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("正文"));
        assert!(!out.content.contains("本章完"), "got {:?}", out.content);
    }

    #[test]
    fn unsupported_regex_does_not_panic() {
        let r = rule_with(r"喜欢(.+?)\1", "");
        let c = ch("第1章", "<p>喜欢abcabc其他</p>");
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("喜欢"));
    }

    #[test]
    fn shuhaige_filter_txt_strips_ad_after_backreference_fix() {
        // 书海阁 filterTxt 去掉反向引用 `\1` 后的版本; 末尾字面 `。` 锚定,
        // 不会跨段落吃正文（见下方"不吞段落"断言）。
        let r = rule_with(
            r"本小章还未完.+|小主.+|这章没有结束.+|喜欢.+?请大家收藏：\([^)]+\)书海阁小说网更新速度全网最快。|\(本章完\)",
            "",
        );
        let c = ch(
            "第1章",
            "<p>正文一</p><p>喜欢本站请大家收藏：(本站123)书海阁小说网更新速度全网最快。</p><p>正文二</p>",
        );
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("正文一"));
        assert!(out.content.contains("正文二"), "got {:?}", out.content);
        assert!(!out.content.contains("书海阁"), "got {:?}", out.content);
        assert!(!out.content.contains("请大家收藏"), "got {:?}", out.content);
    }

    #[test]
    fn applies_filter_tag_via_dom() {
        let r = rule_with("", "script, .ad");
        let c = ch(
            "第1章",
            r#"<p>正文1</p><script>bad()</script><div class="ad">广告</div><p>正文2</p>"#,
        );
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("正文1"));
        assert!(out.content.contains("正文2"));
        assert!(!out.content.contains("bad()"));
        assert!(!out.content.contains("广告"));
    }

    #[test]
    fn strips_leading_title_with_html_wrapper() {
        let r = rule_with("", "");
        let c = ch("第1章 起航", "<h1>第1章 起航</h1><p>正文</p>");
        let out = filter_chapter(&c, &r);
        assert!(!out.content.contains("第1章 起航"));
        assert!(out.content.contains("正文"));
    }

    #[test]
    fn strips_leading_title_when_compacted() {
        let r = rule_with("", "");
        let c = ch("第 1 章 起航", "<p>第1章起航 接下来的正文</p>");
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("接下来的正文"));
        assert!(!out.content.contains("第1章起航"), "got {:?}", out.content);
    }

    #[test]
    fn does_not_strip_title_when_not_at_start() {
        let r = rule_with("", "");
        // 标题在中间，不应被擦
        let c = ch("第1章", "<p>引子</p><p>第1章 在中间</p>");
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("第1章 在中间"));
    }

    #[test]
    fn rewrites_numeric_dot_title() {
        let r = rule_with("", "");
        let c = ch("1.起航", "<p>正文</p>");
        let out = filter_chapter(&c, &r);
        assert_eq!(out.title, "第1章 起航");
    }

    #[test]
    fn rewrites_numeric_dot_title_with_spaces() {
        let r = rule_with("", "");
        let c = ch("12 . 终章", "<p>x</p>");
        let out = filter_chapter(&c, &r);
        assert_eq!(out.title, "第12章 终章");
    }

    #[test]
    fn strips_empty_tags_after_filter() {
        let r = rule_with("", "");
        let c = ch("第1章", "<p></p><p>正文</p><div>  </div>");
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("正文"));
        assert!(!out.content.contains("<p></p>"));
        assert!(!out.content.contains("<div>"), "got {:?}", out.content);
    }

    #[test]
    fn handles_nested_empty_tags() {
        let r = rule_with("", "");
        let c = ch("第1章", "<div><p></p></div><p>真正文</p>");
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("真正文"));
        assert!(!out.content.contains("<div>"));
    }

    #[test]
    fn end_to_end_main_json_22biqu_pattern() {
        let r = rule_with(r"\(本章完\)", "");
        let c = ch(
            "第1章 起航",
            "<h1>第1章 起航</h1><p>正文一</p><p>正文二</p><p>(本章完)</p>",
        );
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("正文一"));
        assert!(out.content.contains("正文二"));
        assert!(!out.content.contains("第1章 起航"));
        assert!(!out.content.contains("本章完"));
    }
}
