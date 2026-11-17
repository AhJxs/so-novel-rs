//! 把归一化章节渲染成各导出器需要的内存正文。
//!
//! TXT / Markdown 保留纯文本；HTML / EPUB / PDF 共用完整 HTML 文档，
//! 满足 HTML 目录 `<title>`、EPUB 正文与 PDF 的 `<h1>` / `<p>` 提取约定。

use crate::config::ExportFormat;
use crate::tools::source_book::SourceChapter;

/// 中文排版首行缩进（两个全角空格）。
const INDENT: &str = "\u{3000}\u{3000}";

/// 按目标格式渲染一章。
#[must_use]
pub fn render_chapter(chapter: &SourceChapter, target: ExportFormat) -> String {
    match target {
        ExportFormat::Txt => render_txt(chapter),
        ExportFormat::Markdown => render_markdown(chapter),
        ExportFormat::Html | ExportFormat::Epub | ExportFormat::Pdf => render_html(chapter),
    }
}

fn render_txt(chapter: &SourceChapter) -> String {
    let mut out = String::new();
    out.push_str(&chapter.title);
    out.push_str("\n\n");
    for paragraph in &chapter.paragraphs {
        out.push_str(INDENT);
        out.push_str(paragraph);
        out.push('\n');
    }
    out
}

fn render_markdown(chapter: &SourceChapter) -> String {
    let mut out = format!("## {}\n\n", chapter.title);
    for (index, paragraph) in chapter.paragraphs.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(paragraph.trim());
        out.push('\n');
    }
    out
}

fn render_html(chapter: &SourceChapter) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"zh\">\n<head>\n<meta charset=\"utf-8\"/>\n");
    out.push_str("<title>");
    out.push_str(&escape_html(&chapter.title));
    out.push_str("</title>\n</head>\n<body>\n<h1>");
    out.push_str(&escape_html(&chapter.title));
    out.push_str("</h1>\n");
    for paragraph in &chapter.paragraphs {
        out.push_str("<p>");
        out.push_str(&escape_html(paragraph).replace('\n', "<br/>"));
        out.push_str("</p>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// HTML 文本转义。
#[must_use]
pub(crate) fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use crate::config::ExportFormat;
    use crate::tools::source_book::SourceChapter;

    use super::render_chapter;

    fn chapter() -> SourceChapter {
        SourceChapter {
            title: "第一章 <起航>".to_string(),
            paragraphs: vec!["甲 & 乙".to_string(), "丙\n丁".to_string()],
        }
    }

    #[test]
    fn txt_puts_title_then_indented_paragraphs() {
        let out = render_chapter(&chapter(), ExportFormat::Txt);
        assert!(out.starts_with("第一章 <起航>\n\n"));
        assert!(out.contains("\u{3000}\u{3000}甲 & 乙\n"));
        assert!(out.contains("\u{3000}\u{3000}丙\n丁\n"));
    }

    #[test]
    fn markdown_uses_heading_and_blank_lines() {
        let out = render_chapter(&chapter(), ExportFormat::Markdown);
        assert!(out.starts_with("## 第一章 <起航>\n\n"));
        assert!(out.contains("甲 & 乙\n\n丙\n丁\n"));
    }

    #[test]
    fn html_escapes_title_and_wraps_paragraphs() {
        let out = render_chapter(&chapter(), ExportFormat::Html);
        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains("<title>第一章 &lt;起航&gt;</title>"));
        assert!(out.contains("<h1>第一章 &lt;起航&gt;</h1>"));
        assert!(out.contains("<p>甲 &amp; 乙</p>"));
        assert!(out.contains("<p>丙<br/>丁</p>"));
    }
}
