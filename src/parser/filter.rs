//! 章节正文清洗。对应 Java `core.ChapterFilter`。
//!
//! 顺序与 Java 端一致：
//! 1. **不可见字符**：移除控制字符 / 零宽 / 行段分隔等（复用 `http::clean_invisible_chars`）。
//! 2. **HTML 实体**：删除 `&xxx;` 字符引用（`&nbsp;`、`&amp;` 等都被清掉，
//!    与 Java 端 `&[^;]+;` 一致；这是为兼容 iBooks 等阅读器，故意不做转义）。
//! 3. **filterTxt 广告正则**：用规则中的正则替换为空串。
//!    - Java 用 `String.replaceAll(regex, "")`，规则里偶尔会出现 Rust `regex`
//!      不支持的语法（backreference `\1`、lookahead `(?=...)`、possessive `*+` 等）。
//!      我们已修复 `bundle/rules/main.json` 里书海阁的 `\(([^)]+)\)\1`
//!      → `\([^)]+\)`（捕获组 1 vs 2 内容本就不同，`\1` 是无效语法噪声）。
//!    - 兜底策略：**编译失败时降级为不替换 + tracing::warn**，不阻塞整章下载。
//!      这是去广告功能，不是关键路径；为剩下的边角 case 拉 `fancy-regex` 大依赖
//!      收益过低（warn 日志也方便日后 grep 出仍需手工改的规则）。
//! 4. **filterTag 节点删除**：复用 `parser::dom::remove_tags`。
//! 5. **重复标题去除**：在正文开头如果出现章节名（可能被若干 HTML 标签或空白包住），
//!    把章节名擦掉，保留前面的 tag/whitespace（与 Java `replaceFirst("^(\\s|<[^>]+>)*(title)", "$1")` 等价）。
//! 6. **`1.章节名` → `第1章 章节名`**：兼容某些阅读器目录解析。
//! 7. **空标签清理**：`<p></p>`、`<div></div>` 等。
//!
//! 这一层是**纯函数**：只依赖 rule.chapter 与输入 chapter，无 IO。

use regex::Regex;
use std::sync::LazyLock;

use crate::http::clean_invisible_chars;
use crate::models::{Chapter, RuleChapter};
use crate::parser::dom::remove_tags;

static HTML_ENTITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&[^;]+;").expect("html entity re"));
static EMPTY_TAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    // 反复运行直到稳定（嵌套空 tag）。每次匹配一个最内层的 <tag></tag>。
    Regex::new(r"<([A-Za-z][A-Za-z0-9]*)\b[^>]*>\s*</\s*([A-Za-z][A-Za-z0-9]*)\s*>")
        .expect("empty tag re")
});
static TITLE_NUMBER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+)\s*\.\s*(.+)$").expect("title number re"));

/// 清洗一个章节，返回新的 Chapter（不修改入参）。
///
/// `rule_chapter` 提供 filterTxt / filterTag 配置；其它字段（content/title/url/order）
/// 来自入参 `chapter`。
pub fn filter_chapter(chapter: &Chapter, rule_chapter: &RuleChapter) -> Chapter {
    let mut content = chapter.content.clone();
    let mut title = chapter.title.clone();

    // 1. 不可见字符
    content = clean_invisible_chars(&content);

    // 2. HTML 实体
    content = HTML_ENTITY_RE.replace_all(&content, "").into_owned();

    // 3. filterTxt 广告正则
    if !rule_chapter.filter_txt.is_empty() {
        match crate::parser::cache::cached_regex(&rule_chapter.filter_txt) {
            Ok(re) => {
                content = re.replace_all(&content, "").into_owned();
            }
            Err(e) => {
                // backreference / 负前瞻等不支持的语法 → 跳过这一步而不是崩
                tracing::warn!(
                    "filterTxt 正则不被 Rust regex 支持，已跳过广告过滤；regex={}, err={}",
                    rule_chapter.filter_txt,
                    e
                );
            }
        }
    }

    // 4. filterTag 节点删除
    if !rule_chapter.filter_tag.trim().is_empty() {
        content = remove_tags(&content, &rule_chapter.filter_tag);
    }

    // 5. 重复标题去除（保留前面的 tag/whitespace）
    content = strip_leading_title(&content, &title);

    // 6. `1.章节名` → `第1章 章节名`
    if let Some(cap) = TITLE_NUMBER_RE.captures(&title) {
        let n = cap.get(1).unwrap().as_str();
        let rest = cap.get(2).unwrap().as_str();
        title = format!("第{n}章 {rest}");
    }

    // 7. 空 tag 清理（迭代直到稳定，处理嵌套）
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
/// 等价 Java：
/// ```text
/// content.replaceFirst("^(\\s|<[^>]+>)*(quote(title)|quote(cleanBlank(title)))", "$1")
/// ```
/// 注意 `cleanBlank` 是把所有空白删掉，所以两个候选都是字面串（无元字符），
/// 我们用 `regex::escape` 处理。
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
    // replacen=1 等价 Java replaceFirst；保留第 1 组（前面的空白/标签）。
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
        // &nbsp; 与 &amp; 都被清掉
        assert_eq!(out.content, "<p>段落一</p>");
    }

    #[test]
    fn applies_filter_txt_regex() {
        // main.json 燃文小说网真实模式（精简）
        let r = rule_with(r#"\(本章完\)"#, "");
        let c = ch("第1章", "<p>正文</p><p>(本章完)</p>");
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("正文"));
        assert!(!out.content.contains("本章完"), "got {:?}", out.content);
    }

    #[test]
    fn unsupported_regex_does_not_panic() {
        // 含 Rust regex 不支持的 backreference（书海阁真实模式精简）
        let r = rule_with(r#"喜欢(.+?)\1"#, "");
        let c = ch("第1章", "<p>喜欢abcabc其他</p>");
        // 不崩；广告片段保留（降级行为）
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("喜欢"));
    }

    #[test]
    fn shuhaige_filter_txt_strips_ad_after_backreference_fix() {
        // bundle/rules/main.json 书海阁 filterTxt 的修复版本：
        //   原: 喜欢(.+?)请大家收藏：\(([^)]+)\)\1书海阁小说网更新速度全网最快。
        //   新: 喜欢.+?请大家收藏：\([^)]+\)书海阁小说网更新速度全网最快。
        // （去掉反向引用 `\1` —— Rust regex 不支持 backreference，
        //  且原写法捕获组 1 vs 2 内容本就不同，`\1` 等价于无用语法噪声。
        //  注意末尾是字面 `。` 锚定，不会跨段落吃正文 —— 见下方"不吞段落"断言。）
        let r = rule_with(
            r#"本小章还未完.+|小主.+|这章没有结束.+|喜欢.+?请大家收藏：\([^)]+\)书海阁小说网更新速度全网最快。|\(本章完\)"#,
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
        // main.json 书海阁的 filterTag: ".bottem2, hr, script, table"（精简）
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
        // 正文开头有 <h1>第1章 起航</h1>，应被擦掉，保留前面的 tag
        let r = rule_with("", "");
        let c = ch("第1章 起航", "<h1>第1章 起航</h1><p>正文</p>");
        let out = filter_chapter(&c, &r);
        // 标题文字消失，但 <h1></h1> 经空 tag 清理后也消失
        assert!(!out.content.contains("第1章 起航"));
        assert!(out.content.contains("正文"));
    }

    #[test]
    fn strips_leading_title_when_compacted() {
        // 标题里有空格，正文里却没空格的版本
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
        // <div>  </div>（含空白）也算空 tag
        assert!(!out.content.contains("<div>"), "got {:?}", out.content);
    }

    #[test]
    fn handles_nested_empty_tags() {
        let r = rule_with("", "");
        // 经 filter 后，<div><p></p></div> 也应空掉
        let c = ch("第1章", "<div><p></p></div><p>真正文</p>");
        let out = filter_chapter(&c, &r);
        assert!(out.content.contains("真正文"));
        assert!(!out.content.contains("<div>"));
    }

    #[test]
    fn end_to_end_main_json_22biqu_pattern() {
        // 模拟 main.json 笔趣阁22 风格：filterTxt = (本章完)
        let r = rule_with(r#"\(本章完\)"#, "");
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
