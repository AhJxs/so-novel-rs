//! PDF HTML → 结构化内容 (PR #17 拆分, 2026-07-08).
//!
//! 负责把章节 HTML 抽成 `(title, Vec<paragraph>)` 元组, 给 `document.rs` 的
//! `Paginator` 喂纯文本。包含 5 个 fn:
//! - [`extract_chapter_content`]: 主入口, 抽 h1 + p
//! - [`html_to_text`]: HTML 片段 → 纯文本
//! - [`decode_entities`]: HTML 实体解码
//! - [`extract_body`]: 剥外层 html/head, 取 body
//! - [`strip_nav_bar`]: 删翻页按钮栏
//! - [`wrap_text`]: 中文字符友好换行 (Paginator 复用)

use std::sync::LazyLock;

use regex::Regex;

use super::fonts::Measurer;

/// 从章节 body HTML 抽 (标题, 段落列表)。
///
/// # Examples
///
/// ```ignore
/// let html = r##"<h1>第1章</h1><p>正文一</p><p>正文二</p>"##;
/// let (title, paras) = extract_chapter_content(html);
/// assert_eq!(title.as_deref(), Some("第1章"));
/// assert_eq!(paras, vec!["正文一", "正文二"]);
/// ```
///
/// # Errors
///
/// 无错误返回 (Regex 编译失败会让程序启动 panic, 见 `static` 内部)
pub fn extract_chapter_content(body_html: &str) -> (Option<String>, Vec<String>) {
    static H1_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>").expect("h1 re"));
    static P_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<p[^>]*>(.*?)</p>").expect("p re"));

    let title = H1_RE
        .captures(body_html)
        .and_then(|c| c.get(1))
        .map(|m| html_to_text(m.as_str()))
        .filter(|s| !s.is_empty());

    let paras = P_RE
        .captures_iter(body_html)
        .map(|c| html_to_text(c.get(1).map(|m| m.as_str()).unwrap_or("")))
        .filter(|s| !s.is_empty())
        .collect();

    (title, paras)
}

/// HTML 片段 → 纯文本: `<br>` 转空格 → 剥所有标签 → 解码实体 → 折叠空白。
pub fn html_to_text(html: &str) -> String {
    static BR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<br\s*/?>").expect("br re"));
    static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<[^>]+>").expect("tag re"));
    static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("ws re"));

    let no_br = BR_RE.replace_all(html, " ");
    let no_tag = TAG_RE.replace_all(&no_br, "");
    let decoded = decode_entities(&no_tag);
    WS_RE.replace_all(decoded.trim(), " ").into_owned()
}

/// 解码常见 HTML 实体 + 数字实体 (`&#NN;` / `&#xHH;`)。
pub fn decode_entities(s: &str) -> String {
    static ENT_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"&#(x?)([0-9a-fA-F]+);|&([a-zA-Z]+);").expect("ent re"));
    ENT_RE
        .replace_all(s, |caps: &regex::Captures| {
            // 数字实体 &#NN; / &#xHH;
            if let (Some(hex), Some(num)) = (caps.get(1), caps.get(2)) {
                let radix = if hex.as_str().eq_ignore_ascii_case("x") {
                    16
                } else {
                    10
                };
                return u32::from_str_radix(num.as_str(), radix)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| c.to_string())
                    .unwrap_or_default();
            }
            // 命名实体
            if let Some(name) = caps.get(3) {
                return match name.as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    "nbsp" => " ",
                    _ => name.as_str(),
                }
                .to_string();
            }
            String::new()
        })
        .into_owned()
}

/// 把单章 HTML 拆出 `<body>` 内部的内容, 剥掉外层 `<html>` / `<head>` / DOCTYPE。
///
/// 找不到 `<body>` 时返回 `None` (由 caller 兜底用原文)。
pub fn extract_body(html: &str) -> Option<String> {
    static BODY_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?is)<body[^>]*>(.*)</body>").expect("body re"));
    BODY_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 删掉网页模板自带的翻页按钮栏 `<div class="bottom-bar">…上一页…下一页…</div>`。
pub fn strip_nav_bar(body_html: &str) -> String {
    static NAV_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?is)<div[^>]*class="[^"]*bottom-bar[^"]*"[^>]*>.*?</div>"#).expect("nav re")
    });
    NAV_RE.replace_all(body_html, "").into_owned()
}

/// 中文换行: 逐字累加宽度, 超 `max_w` 断行。ASCII 连续字母数字作为整体词不拆
/// (过长单词硬拆)。返回的每行都保证宽度 ≤ `max_w`。
pub fn wrap_text(s: &str, max_w: f32, size: f32, m: &Measurer) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0f32;

    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        // ASCII 词 (字母/数字连串) 整体处理, 避免半路拆 English/URL
        if ch.is_ascii_alphanumeric() {
            let mut word = String::new();
            word.push(ch);
            while let Some(&n) = chars.peek() {
                if n.is_ascii_alphanumeric() {
                    word.push(n);
                    chars.next();
                } else {
                    break;
                }
            }
            let w = m.text_w(&word, size);
            // 当前行放不下 → 先收行
            if !cur.is_empty() && cur_w + w > max_w {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0.0;
            }
            if w > max_w {
                // 单词比一行还长 → 硬拆
                if !cur.is_empty() {
                    lines.push(std::mem::take(&mut cur));
                }
                let mut hw = String::new();
                let mut hw_w = 0.0;
                for wc in word.chars() {
                    let ww = m.char_w(wc, size);
                    if !hw.is_empty() && hw_w + ww > max_w {
                        lines.push(std::mem::take(&mut hw));
                        hw_w = 0.0;
                    }
                    hw.push(wc);
                    hw_w += ww;
                }
                cur = hw;
                cur_w = hw_w;
            } else {
                cur.push_str(&word);
                cur_w += w;
            }
            continue;
        }

        let w = m.char_w(ch, size);
        if !cur.is_empty() && cur_w + w > max_w {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
        }
        cur.push(ch);
        cur_w += w;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_chapter_content_pulls_h1_and_ps() {
        let html = r##"<h1>第1章</h1><p>第一段</p><p>第二段</p>"##;
        let (title, paras) = extract_chapter_content(html);
        assert_eq!(title.as_deref(), Some("第1章"));
        assert_eq!(paras, vec!["第一段", "第二段"]);
    }

    #[test]
    fn extract_chapter_content_no_h1_returns_none_title() {
        let html = r##"<p>只有段落</p>"##;
        let (title, paras) = extract_chapter_content(html);
        assert!(title.is_none());
        assert_eq!(paras, vec!["只有段落"]);
    }

    #[test]
    fn html_to_text_strips_tags_and_decodes_entities() {
        let html = "<p>hello&nbsp;world &amp; &lt;tag&gt;</p>";
        assert_eq!(html_to_text(html), "hello world & <tag>");
    }

    #[test]
    fn decode_entities_handles_numeric_and_named() {
        assert_eq!(decode_entities("&#65;B&#x43;"), "ABC");
        assert_eq!(decode_entities("&lt;&gt;&amp;"), "<>&");
    }

    #[test]
    fn extract_body_strips_outer_html() {
        let html = r##"<!DOCTYPE html><html><head></head><body><h1>Hi</h1></body></html>"##;
        let body = extract_body(html).unwrap();
        assert!(body.contains("<h1>Hi</h1>"));
        assert!(!body.contains("<html"));
    }

    #[test]
    fn strip_nav_bar_removes_bottom_bar() {
        let html = r##"<p>keep</p><div class="bottom-bar">上一页 下一页</div><p>after</p>"##;
        let result = strip_nav_bar(html);
        assert!(!result.contains("bottom-bar"));
        assert!(!result.contains("上一页"));
        assert!(result.contains("keep"));
        assert!(result.contains("after"));
    }

    #[test]
    fn extract_body_strips_html_head_and_doctype() {
        let html = r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>x</title></head>
<body><h1>Hello</h1><p>World</p></body></html>"#;
        let inner = extract_body(html).unwrap();
        assert!(!inner.contains("<html"));
        assert!(!inner.contains("<head"));
        assert!(!inner.contains("<body"));
        assert!(inner.contains("<h1>Hello</h1>"));
        assert!(inner.contains("<p>World</p>"));
    }

    #[test]
    fn extract_body_returns_none_when_no_body() {
        assert!(extract_body("plain text only").is_none());
        assert!(extract_body("<html><head>x</head></html>").is_none());
    }

    #[test]
    fn extract_chapter_content_handles_entities_in_para() {
        let body = r#"<h1>T</h1><p>a&amp;b</p>"#;
        let (_, paras) = extract_chapter_content(body);
        assert_eq!(paras, vec!["a&b"]);
    }

    #[test]
    fn html_to_text_handles_all_entity_forms() {
        assert_eq!(html_to_text("<p>段一</p>"), "段一");
        assert_eq!(
            html_to_text("a&amp;b &lt; &gt; &quot;x&quot;"),
            "a&b < > \"x\""
        );
        assert_eq!(html_to_text("第一<br>第二"), "第一 第二");
        assert_eq!(html_to_text("  多   余  空白  "), "多 余 空白");
        // 数字实体
        assert_eq!(html_to_text("&#65;&#x4e2d;"), "A中");
    }

    #[test]
    fn wrap_text_breaks_long_cjk_line() {
        let m = Measurer::Heuristic;
        // 一行容不下时必须断成多行
        let long = "字".repeat(100);
        let lines = wrap_text(&long, 50.0, 12.0, &m);
        assert!(lines.len() > 1, "should wrap: {} lines", lines.len());
        // 每行（除可能末行）宽度 ≤ max_w
        for ln in &lines {
            let w: f32 = ln.chars().map(|c| m.char_w(c, 12.0)).sum();
            assert!(w <= 50.0 + 0.01, "line too wide: {w} ({ln})");
        }
    }

    #[test]
    fn wrap_text_keeps_ascii_word_intact() {
        let m = Measurer::Heuristic;
        // English word 不会被拆到两行
        let text = "中文EnglishWord中文";
        let lines = wrap_text(text, 30.0, 12.0, &m);
        let rejoined: String = lines.concat();
        assert!(rejoined.contains("EnglishWord"), "word split: {rejoined}");
    }
}
