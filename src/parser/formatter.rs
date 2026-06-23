//! 章节正文段落整形。对应 Java `core.ChapterFormatter`。
//!
//! 调用顺序（与 Java 端一致）：先 `clear_all_attributes`，再做段落整形。
//!
//! 两种整形模式（由规则的 `paragraphTagClosed` 决定）：
//!
//! 1. **闭合标签** (`true`)：源站每段已经被某个 tag 包住（常见是 `<p>`，但也有
//!    `<span>` / `<div>` 等）。把所有非 `<p>` 的成对闭合 tag 改写为 `<p>`。
//!    Java 用 `<(?!p\\b)([^>]+)>(.*?)</\\1>` 含负前瞻 + backreference，
//!    Rust regex 都不支持。**改写策略**：因为前一步已经清空属性，输入形如
//!    `<tag>x</tag>` 非常稳定，用
//!    `<([A-Za-z][A-Za-z0-9]*)>([\s\S]*?)</[A-Za-z][A-Za-z0-9]*>` + 闭包里跳过
//!    `p` 即可（不强制开闭对应；与 Java 实际行为等价：匹配最近的 `</tag2>`
//!    而非 `</tag1>` 时，最终都被规整为 `<p>`，差异只在嵌套结构上，而源站
//!    正文几乎没有正文级嵌套）。
//!
//! 2. **非闭合标签** (`false`)：源站每段以 `<br>+` 等分隔符隔开。按规则中的
//!    `paragraphTag`（已经是个正则，例如 `<br>+`）切分，逐段包 `<p>`。

use once_cell::sync::Lazy;
use regex::Regex;

use crate::models::RuleChapter;
use crate::parser::dom::clear_all_attributes;

static NON_P_PAIR_RE: Lazy<Regex> = Lazy::new(|| {
    // 匹配一对开闭标签（不要求名字一致；见模块文档解释）。
    Regex::new(r"<([A-Za-z][A-Za-z0-9]*)>([\s\S]*?)</[A-Za-z][A-Za-z0-9]*>").expect("non-p pair re")
});

/// 把规则化清洗后的章节内容整形为"一段一行 `<p>` 的 HTML"。
///
/// 入参 `content` 通常来自 `filter_chapter(...).content`。
pub fn format_chapter(content: &str, rule_chapter: &RuleChapter) -> String {
    // 与 Java 一致：先把属性全清掉，避免被 class/style 影响匹配。
    let cleared = clear_all_attributes(content);

    if rule_chapter.paragraph_tag_closed {
        format_closed(&cleared)
    } else {
        format_open(&cleared, &rule_chapter.paragraph_tag)
    }
}

/// 处理闭合标签模式：把成对开闭标签全部改写为 `<p>...</p>`（含 `<p>` 自身）。
fn format_closed(html: &str) -> String {
    NON_P_PAIR_RE
        .replace_all(html, |caps: &regex::Captures<'_>| {
            let inner = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            format!("<p>{inner}</p>")
        })
        .into_owned()
}

/// 处理非闭合模式：按 `paragraph_tag` 正则切分，逐段包 `<p>`。
///
/// 与 Java `String.split(paragraphTag)` 行为对齐。空段落跳过。
fn format_open(html: &str, paragraph_tag: &str) -> String {
    if paragraph_tag.is_empty() {
        // 没有切分符，整段当一段
        return wrap_p_if_nonblank(html.trim());
    }
    let Ok(re) = crate::parser::cache::cached_regex(paragraph_tag) else {
        // 切分正则不合法时降级：原样返回，至少不丢失正文。
        tracing::warn!("paragraphTag 不被 Rust regex 支持，已降级为不切分；regex={paragraph_tag}");
        return wrap_p_if_nonblank(html.trim());
    };
    let mut out = String::with_capacity(html.len());
    for piece in re.split(html) {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        out.push_str("<p>");
        out.push_str(piece);
        out.push_str("</p>");
    }
    out
}

fn wrap_p_if_nonblank(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!("<p>{s}</p>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed_rule() -> RuleChapter {
        RuleChapter {
            paragraph_tag_closed: true,
            paragraph_tag: String::new(),
            ..RuleChapter::default()
        }
    }

    fn open_rule(tag: &str) -> RuleChapter {
        RuleChapter {
            paragraph_tag_closed: false,
            paragraph_tag: tag.to_string(),
            ..RuleChapter::default()
        }
    }

    // ---------- 闭合模式 ----------

    #[test]
    fn closed_keeps_existing_p_tags() {
        let r = closed_rule();
        let out = format_chapter("<p>段一</p><p>段二</p>", &r);
        assert_eq!(out, "<p>段一</p><p>段二</p>");
    }

    #[test]
    fn closed_rewrites_span_to_p() {
        // 源站可能用 <span>段</span> 当段落
        let r = closed_rule();
        let out = format_chapter("<span>段一</span><span>段二</span>", &r);
        assert_eq!(out, "<p>段一</p><p>段二</p>");
    }

    #[test]
    fn closed_strips_attributes_before_rewriting() {
        // 输入带属性（class/style，被反爬隐藏）；期望先清属性再整形为 <p>
        let r = closed_rule();
        let out = format_chapter(r#"<p class="hide" style="display:none">真正文</p>"#, &r);
        assert_eq!(out, "<p>真正文</p>");
    }

    #[test]
    fn closed_rewrites_div_to_p() {
        let r = closed_rule();
        let out = format_chapter("<div>段一</div><div>段二</div>", &r);
        assert_eq!(out, "<p>段一</p><p>段二</p>");
    }

    // ---------- 非闭合模式 ----------

    #[test]
    fn open_splits_by_br_plus() {
        // main.json 中最常见：paragraphTag = "<br>+"
        let r = open_rule("<br>+");
        let out = format_chapter("段一<br><br>段二<br>段三", &r);
        assert_eq!(out, "<p>段一</p><p>段二</p><p>段三</p>");
    }

    #[test]
    fn open_skips_empty_pieces() {
        let r = open_rule("<br>+");
        let out = format_chapter("<br><br>段一<br><br><br>段二<br><br>", &r);
        assert_eq!(out, "<p>段一</p><p>段二</p>");
    }

    #[test]
    fn open_with_invalid_regex_degrades_safely() {
        // 不合法切分符 → 不切分 + warn；不丢正文
        let r = open_rule("[invalid");
        let out = format_chapter("段一<br>段二", &r);
        // 全部当一段包起来
        assert!(out.contains("段一"));
        assert!(out.contains("段二"));
    }

    #[test]
    fn open_with_empty_paragraph_tag_wraps_whole_content() {
        let r = open_rule("");
        let out = format_chapter("整段正文", &r);
        assert_eq!(out, "<p>整段正文</p>");
    }

    // ---------- 端到端：Filter → Formatter 联动 ----------

    #[test]
    fn end_to_end_filter_then_format_22biqu_style() {
        use crate::models::Chapter;
        use crate::parser::filter::filter_chapter;

        // 笔趣阁22 风格：paragraphTagClosed = true，filterTxt = (本章完)
        let rule_chapter = RuleChapter {
            paragraph_tag_closed: true,
            filter_txt: r"\(本章完\)".to_string(),
            ..RuleChapter::default()
        };
        let raw = Chapter {
            url: "https://x/".into(),
            title: "第1章 起航".into(),
            content:
                r#"<h1 class="title">第1章 起航</h1><p>正文一</p><p>正文二</p><p>(本章完)</p>"#
                    .into(),
            order: 1,
        };
        let filtered = filter_chapter(&raw, &rule_chapter);
        let final_html = format_chapter(&filtered.content, &rule_chapter);

        assert!(final_html.contains("<p>正文一</p>"));
        assert!(final_html.contains("<p>正文二</p>"));
        assert!(!final_html.contains("第1章 起航"));
        assert!(!final_html.contains("本章完"));
        assert!(!final_html.contains("<h1>"));
    }

    #[test]
    fn end_to_end_filter_then_format_xbiqu_style() {
        use crate::models::Chapter;
        use crate::parser::filter::filter_chapter;

        // 香书小说 风格：paragraphTagClosed = false, paragraphTag = <br>+
        let rule_chapter = RuleChapter {
            paragraph_tag_closed: false,
            paragraph_tag: "<br>+".to_string(),
            filter_txt: r"\(本章完\)".to_string(),
            ..RuleChapter::default()
        };
        let raw = Chapter {
            url: "https://x/".into(),
            title: "第1章".into(),
            content: "第1章<br><br>正文一<br>正文二<br><br>(本章完)".into(),
            order: 1,
        };
        let filtered = filter_chapter(&raw, &rule_chapter);
        let final_html = format_chapter(&filtered.content, &rule_chapter);

        assert!(final_html.contains("<p>正文一</p>"));
        assert!(final_html.contains("<p>正文二</p>"));
        assert!(!final_html.contains("本章完"));
    }
}
