//! 章节渲染。对应 Java `core.ChapterRenderer`。
//!
//! 等价的处理流水线：
//!
//! 1. **过滤**：`parser::filter::filter_chapter`（不可见字符 / HTML 实体 /
//!    filterTxt / filterTag / 标题去重 / 空 tag）。
//! 2. **整形**：`parser::formatter::format_chapter`（清属性 + 段落整形为 `<p>`）。
//! 3. **按目标格式渲染**：
//!    - `Txt`：从 `<p>...</p>` 抽出每段文字加全角缩进 + `\n`，标题在最上面（与 Java 一致）。
//!    - `Html` / `Epub`：套对应模板。
//!    - `Pdf`：阶段 1 锁定为不实现，调用方应在 UI 层禁用；本函数若被传入 Pdf
//!      会回落到 Html 模板（与 Java 端 PDF 模板内容相似度高）+ tracing::warn，
//!      不让用户的下载流程崩。
//!
//! 模板内嵌：避免拉 FreeMarker 等价物。仅 `${title}` / `${content}` 两个占位。

use crate::config::ExportFormat;
use crate::models::{Chapter, RuleChapter};
use crate::parser::{filter::filter_chapter, formatter::format_chapter};

/// 渲染目标。是 `ExportFormat` 的解析友好别名（避免 `export` 模块直接耦合 config）。
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RenderTarget {
    Txt,
    Html,
    Epub,
    /// 阶段 1 锁定不实现；命中时降级 Html 渲染并 warn。
    Pdf,
}

impl From<ExportFormat> for RenderTarget {
    fn from(f: ExportFormat) -> Self {
        match f {
            ExportFormat::Txt => RenderTarget::Txt,
            ExportFormat::Html => RenderTarget::Html,
            ExportFormat::Epub => RenderTarget::Epub,
            ExportFormat::Pdf => RenderTarget::Pdf,
        }
    }
}

/// 把抓取到的原始章节渲染为目标格式的字符串。
///
/// 入参 `chapter` 是 ChapterParser 拿到的 `(url, title, content=原 HTML, order)`；
/// `rule_chapter` 提供 filterTxt / filterTag / paragraphTagClosed / paragraphTag。
///
/// 返回 `(title, body)` — 调用方负责落盘（阶段 3b 导出层）。返回的 `title`
/// 是经过"`1.章节名` → `第1章 章节名`"重写后的版本。
pub fn render_chapter(
    chapter: &Chapter,
    rule_chapter: &RuleChapter,
    target: RenderTarget,
) -> (String, String) {
    let filtered = filter_chapter(chapter, rule_chapter);
    let formatted_html = format_chapter(&filtered.content, rule_chapter);

    let body = match target {
        RenderTarget::Txt => render_txt(&filtered.title, &formatted_html),
        RenderTarget::Html => render_html_template(&filtered.title, &formatted_html),
        RenderTarget::Epub => render_epub_template(&filtered.title, &formatted_html),
        RenderTarget::Pdf => {
            tracing::warn!(
                "PDF 渲染未实现，章节《{}》降级为 Html 模板输出。详见 audit §6.4。",
                filtered.title
            );
            render_html_template(&filtered.title, &formatted_html)
        }
    };

    (filtered.title, body)
}

/// TXT：从 `<p>...</p>` 中抽段落文字，全角缩进 2 字符 + 换行。
/// Java 端逻辑：`while matcher.find() { sb.append(indent).append(group(1)).append('\n'); }`
fn render_txt(title: &str, p_html: &str) -> String {
    use once_cell::sync::Lazy;
    use regex::Regex;

    static P_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<p>(.*?)</p>").expect("p tag re"));

    // 全角空格，与 Java 端 `　` 一致
    let indent = "\u{3000}\u{3000}";
    let mut sb = String::with_capacity(p_html.len());
    sb.push_str(title);
    sb.push_str("\n\n");

    let mut matched = false;
    for cap in P_RE.captures_iter(p_html) {
        matched = true;
        let inner = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        sb.push_str(indent);
        sb.push_str(inner);
        sb.push('\n');
    }
    if !matched {
        // 无 <p> 时直接把整段当一行（极端兜底）
        let s = p_html.trim();
        if !s.is_empty() {
            sb.push_str(indent);
            sb.push_str(s);
            sb.push('\n');
        }
    }
    sb
}

/// HTML 章节模板。等价 Java `templates/chapter_html.flt`：
/// 一个完整 HTML 文档，含上一页/下一页按钮（按文件名前导零数字推断）。
///
/// 模板内嵌：见 `parser::formatter` 的相同理由——只两个占位，
/// 拉 tinytemplate / handlebars 收益过低。
fn render_html_template(title: &str, content_html: &str) -> String {
    let title_esc = html_escape_attr(title);
    let template = include_str!("../../assets/chapter_html.tmpl");
    template
        .replace("${title}", &title_esc)
        .replace("${content}", content_html)
}

/// EPUB 章节模板。等价 Java `templates/chapter_epub.flt`：
/// 一个 XHTML 章节文件（注意 doctype + xhtml namespace；Apple Books 较严格）。
fn render_epub_template(title: &str, content_html: &str) -> String {
    let title_esc = html_escape_attr(title);
    let template = include_str!("../../assets/chapter_epub.tmpl");
    template
        .replace("${title}", &title_esc)
        .replace("${content}", content_html)
}

/// 简单 HTML 文本/属性转义（章节标题用）：&、<、>、"、'。
/// 不对正文 content 做转义（content 已经是合法 HTML 段落序列）。
fn html_escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule_closed_with_ad() -> RuleChapter {
        RuleChapter {
            paragraph_tag_closed: true,
            filter_txt: r"\(本章完\)".to_string(),
            ..RuleChapter::default()
        }
    }

    fn raw_chapter() -> Chapter {
        Chapter {
            url: "https://x/c1.html".into(),
            title: "第1章 起航".into(),
            content: r#"<h1>第1章 起航</h1><p>段一</p><p>段二</p><p>(本章完)</p>"#.into(),
            order: 1,
        }
    }

    // ---------- TXT ----------

    #[test]
    fn render_txt_extracts_paragraphs_with_indent() {
        let (title, body) =
            render_chapter(&raw_chapter(), &rule_closed_with_ad(), RenderTarget::Txt);
        assert_eq!(title, "第1章 起航");
        let lines: Vec<&str> = body.split('\n').collect();
        // 标题 + 空行 + 段一 + 段二 + 末尾空行
        assert_eq!(lines[0], "第1章 起航");
        assert!(lines[1].is_empty(), "expect blank line, got {:?}", lines[1]);
        assert!(
            lines[2].starts_with('\u{3000}'),
            "no full-width indent: {:?}",
            lines[2]
        );
        assert!(lines[2].contains("段一"));
        assert!(lines[3].contains("段二"));
        assert!(!body.contains("本章完"));
    }

    #[test]
    fn render_txt_handles_open_paragraph_rule() {
        let rule = RuleChapter {
            paragraph_tag_closed: false,
            paragraph_tag: "<br>+".to_string(),
            filter_txt: String::new(),
            ..RuleChapter::default()
        };
        let raw = Chapter {
            url: "https://x/c.html".into(),
            title: "第1章".into(),
            content: "段一<br><br>段二<br>段三".into(),
            order: 1,
        };
        let (_t, body) = render_chapter(&raw, &rule, RenderTarget::Txt);
        assert!(body.contains("段一"));
        assert!(body.contains("段二"));
        assert!(body.contains("段三"));
    }

    // ---------- HTML ----------

    #[test]
    fn render_html_template_wraps_correctly() {
        let (_t, body) = render_chapter(&raw_chapter(), &rule_closed_with_ad(), RenderTarget::Html);
        // 模板里有完整 HTML 文档结构
        assert!(body.contains("<html"), "missing <html: {body}");
        assert!(body.contains("<title>第1章 起航</title>"));
        assert!(body.contains("<h1>第1章 起航</h1>"));
        assert!(body.contains("<p>段一</p>"));
        assert!(body.contains("<p>段二</p>"));
        assert!(!body.contains("本章完"));
        // 翻页 JS 应在模板里
        assert!(body.contains("turnPage"), "missing turnPage hook");
    }

    #[test]
    fn render_html_template_escapes_title_specials() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: r#"第1章 <脏 & "标题">"#.into(),
            content: "<p>x</p>".into(),
            order: 1,
        };
        let (_t, body) = render_chapter(
            &raw,
            &RuleChapter {
                paragraph_tag_closed: true,
                ..RuleChapter::default()
            },
            RenderTarget::Html,
        );
        assert!(body.contains("&lt;脏 &amp; &quot;标题&quot;&gt;"));
    }

    // ---------- EPUB ----------

    #[test]
    fn render_epub_template_uses_xhtml_doctype() {
        let (_t, body) = render_chapter(&raw_chapter(), &rule_closed_with_ad(), RenderTarget::Epub);
        assert!(body.contains("<?xml"));
        assert!(body.contains("xhtml"));
        assert!(body.contains("<h2>第1章 起航</h2>"));
        assert!(body.contains("<p>段一</p>"));
    }

    // ---------- PDF 降级 ----------

    #[test]
    fn render_pdf_degrades_to_html_template() {
        let (_t, body) = render_chapter(&raw_chapter(), &rule_closed_with_ad(), RenderTarget::Pdf);
        // 与 Html 模板等同
        assert!(body.contains("<html"));
        assert!(body.contains("<h1>第1章 起航</h1>"));
    }

    // ---------- ExportFormat → RenderTarget ----------

    #[test]
    fn export_format_maps_to_render_target() {
        assert_eq!(RenderTarget::from(ExportFormat::Txt), RenderTarget::Txt);
        assert_eq!(RenderTarget::from(ExportFormat::Html), RenderTarget::Html);
        assert_eq!(RenderTarget::from(ExportFormat::Epub), RenderTarget::Epub);
        assert_eq!(RenderTarget::from(ExportFormat::Pdf), RenderTarget::Pdf);
    }

    // ---------- 标题 1.x → 第1章 x ----------

    #[test]
    fn render_rewrites_numeric_dot_title() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: "5.归航".into(),
            content: "<p>x</p>".into(),
            order: 5,
        };
        let (title, body) = render_chapter(
            &raw,
            &RuleChapter {
                paragraph_tag_closed: true,
                ..RuleChapter::default()
            },
            RenderTarget::Html,
        );
        assert_eq!(title, "第5章 归航");
        assert!(body.contains("<title>第5章 归航</title>"));
    }
}
