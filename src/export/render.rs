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
//!      会回落到 Html 模板（与 Java 端 PDF 模板内容相似度高）+ `tracing::warn`，
//!      不让用户的下载流程崩。
//!
//! 模板内嵌：避免拉 `FreeMarker` 等价物。仅 `${title}` / `${content}` 两个占位。

use crate::config::{ExportFormat, LangType};
use crate::models::{Chapter, RuleChapter};
use crate::parser::{filter::filter_chapter, formatter::format_chapter};

/// 渲染目标。是 `ExportFormat` 的解析友好别名（避免 `export` 模块直接耦合 config）。
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RenderTarget {
    Txt,
    Html,
    Epub,
    /// 章节文件沿用 Html 模板写到 `chapters_dir`，由 `PdfExporter` 读取并合成 PDF。
    Pdf,
}

impl From<ExportFormat> for RenderTarget {
    fn from(f: ExportFormat) -> Self {
        match f {
            ExportFormat::Txt => Self::Txt,
            ExportFormat::Html => Self::Html,
            ExportFormat::Epub => Self::Epub,
            ExportFormat::Pdf => Self::Pdf,
            // TODO(Task 2): RenderTarget::Markdown + render_md()。
            // 阶段 1 占位：渲染为 HTML（与 PDF 阶段 1 占位语义一致）。
            ExportFormat::Markdown => Self::Html,
        }
    }
}

/// 把抓取到的原始章节渲染为目标格式的字符串。
///
/// 入参 `chapter` 是 `ChapterParser` 拿到的 `(url, title, content=原 HTML, order)`；
/// `rule_chapter` 提供 filterTxt / filterTag / paragraphTagClosed / paragraphTag。
/// `source_lang_raw` 是 `Rule.language`（书源自带的语言标记，如 "`zh_CN`" / "`zh_TW`" /
/// "`zh_Hant"），用于判断是否需要简繁转换`；`target_lang` 是用户在 Settings 选的目标
/// 语言。source == target 或 source 解析失败 → 跳过转换。
///
/// 返回 `(title, body)` — 调用方负责落盘（阶段 3b 导出层）。返回的 `title`
/// 是经过"`1.章节名` → `第1章 章节名`"重写后的版本。
pub fn render_chapter(
    chapter: &Chapter,
    rule_chapter: &RuleChapter,
    target: RenderTarget,
    source_lang_raw: &str,
    target_lang: LangType,
) -> (String, String) {
    let filtered = filter_chapter(chapter, rule_chapter);
    let formatted_html = format_chapter(&filtered.content, rule_chapter);

    let body = match target {
        RenderTarget::Txt => render_txt(&filtered.title, &formatted_html),
        RenderTarget::Html | RenderTarget::Pdf => render_template(
            &filtered.title,
            &formatted_html,
            include_str!("../../assets/chapter_html.tmpl"),
        ),
        RenderTarget::Epub => render_template(
            &filtered.title,
            &formatted_html,
            include_str!("../../assets/chapter_epub.tmpl"),
        ),
    };

    maybe_convert_chinese(filtered.title, body, target, source_lang_raw, target_lang)
}

/// 若源语言与目标语言不同，把章节标题 + body 简繁转换。
/// TXT body 整串转；HTML/EPUB/PDF 走 `convert_html_body`（跳过 `<script>/<style>`，其它
/// 原文走 zhconv —— zhconv 不会改 ASCII 字符，所以标签结构稳定）。
fn maybe_convert_chinese(
    title: String,
    body: String,
    target: RenderTarget,
    source_lang_raw: &str,
    target_lang: LangType,
) -> (String, String) {
    use crate::utils::zhconv::{convert_html_body, convert_text};
    let Some(source) = LangType::parse(source_lang_raw) else {
        return (title, body);
    };
    if source == target_lang {
        return (title, body);
    }
    let new_title = convert_text(&title, &target_lang);
    let new_body = match target {
        RenderTarget::Txt => convert_text(&body, &target_lang),
        RenderTarget::Html | RenderTarget::Epub | RenderTarget::Pdf => {
            convert_html_body(&body, &target_lang)
        }
    };
    (new_title, new_body)
}

/// TXT：从 `<p>...</p>` 中抽段落文字，全角缩进 2 字符 + 换行。
/// Java 端逻辑：`while matcher.find() { sb.append(indent).append(group(1)).append('\n'); }`
fn render_txt(title: &str, p_html: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    /// 编译期确定的正则：用 match 走 panic 路径以避免 `clippy::expect_used`。
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

    static P_RE: LazyLock<Regex> = LazyLock::new(|| compile_static_re(r"(?s)<p>(.*?)</p>"));

    // 全角空格，与 Java 端 `　` 一致
    let indent = "\u{3000}\u{3000}";
    let mut sb = String::with_capacity(p_html.len());
    sb.push_str(title);
    sb.push_str("\n\n");

    let mut matched = false;
    for cap in P_RE.captures_iter(p_html) {
        matched = true;
        let inner = cap.get(1).map_or("", |m| m.as_str());
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

/// 用给定模板渲染章节 HTML。两个模板（HTML / EPUB）仅文件不同，逻辑一致。
// 占位符为 `$` + 标识符 形式；用 const 提出来避免 clippy::literal_string_with_formatting_args
// 误认为 `${title}` 之类是 format! 的格式化参数。
// const 必须先于函数体中所有 statement 声明, 避免 `items_after_statements`。
const TITLE_PLACEHOLDER: &str = "${title}";
const CONTENT_PLACEHOLDER: &str = "${content}";

fn render_template(title: &str, content_html: &str, template: &str) -> String {
    let title_esc = html_escape_attr(title);
    template
        .replace(TITLE_PLACEHOLDER, &title_esc)
        .replace(CONTENT_PLACEHOLDER, content_html)
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
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn rule_closed_with_ad() -> RuleChapter {
        RuleChapter {
            paragraph_tag_closed: true,
            filter_txt: r"\(本章完\)".to_string(),
            ..RuleChapter::default()
        }
    }

    /// 测试便利 wrapper：source="" 解析失败 → 跳过转换，行为与原签名等价。
    /// 已有 6 个测试用 `render(...)` 调它，避免每个测试都传 lang。
    fn render(
        chapter: &Chapter,
        rule_chapter: &RuleChapter,
        target: RenderTarget,
    ) -> (String, String) {
        render_chapter(chapter, rule_chapter, target, "", LangType::ZhCn)
    }

    fn raw_chapter() -> Chapter {
        Chapter {
            url: "https://x/c1.html".into(),
            title: "第1章 起航".into(),
            content: r"<h1>第1章 起航</h1><p>段一</p><p>段二</p><p>(本章完)</p>".into(),
            order: 1,
        }
    }

    // ---------- TXT ----------

    #[test]
    fn render_txt_extracts_paragraphs_with_indent() {
        let (title, body) = render(&raw_chapter(), &rule_closed_with_ad(), RenderTarget::Txt);
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
        let (_t, body) = render(&raw, &rule, RenderTarget::Txt);
        assert!(body.contains("段一"));
        assert!(body.contains("段二"));
        assert!(body.contains("段三"));
    }

    // ---------- HTML ----------

    #[test]
    fn render_html_template_wraps_correctly() {
        let (_t, body) = render(&raw_chapter(), &rule_closed_with_ad(), RenderTarget::Html);
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
        let (_t, body) = render(
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
        let (_t, body) = render(&raw_chapter(), &rule_closed_with_ad(), RenderTarget::Epub);
        assert!(body.contains("<?xml"));
        assert!(body.contains("xhtml"));
        assert!(body.contains("<h2>第1章 起航</h2>"));
        assert!(body.contains("<p>段一</p>"));
    }

    // ---------- PDF 降级 ----------

    #[test]
    fn render_pdf_degrades_to_html_template() {
        let (_t, body) = render(&raw_chapter(), &rule_closed_with_ad(), RenderTarget::Pdf);
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
        let (title, body) = render(
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

    // ---------- 简繁转换集成 ----------

    /// 端到端：源 `zh_CN` + 目标 `zh_TW` → TXT body 简体转繁体（含台湾用词）。
    #[test]
    fn render_converts_simplified_to_traditional_tw_for_txt() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: "软件".into(), // 测试"软体"用词转换
            content: "<p>头发的颜色</p>".into(),
            order: 1,
        };
        let (title, body) = render_chapter(
            &raw,
            &RuleChapter::default(),
            RenderTarget::Txt,
            "zh_CN",
            LangType::ZhTw,
        );
        // 简体"软件" → 台湾繁体"軟體"
        assert_eq!(title, "軟體");
        // 简体"头发" → "頭髮"；"颜色" → "顏色"
        assert!(body.contains("頭髮"), "got: {body}");
        assert!(body.contains("顏色"), "got: {body}");
    }

    /// 端到端：源 `zh_TW` + 目标 `zh_CN` → HTML body 繁体转简体（标签保护）。
    /// 注：zhconv 的 t2s 是字面繁→简（"軟體"→"软体"），不会反向做台湾用词→大陆用词
    /// 的映射（这是 `OpenCC` 算法的限制，不算 bug —— 用户拿到"软体"在大陆可读）。
    #[test]
    fn render_converts_traditional_to_simplified_for_html() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: "軟體".into(),
            content: r#"<p class="c">頭髮顏色</p><script>var x = "不转这里";</script>"#.into(),
            order: 1,
        };
        let (title, body) = render_chapter(
            &raw,
            &RuleChapter::default(),
            RenderTarget::Html,
            "zh_TW",
            LangType::ZhCn,
        );
        assert_eq!(title, "软体");
        // 标签外中文转简体（"<p class="c">..." 被模板再包一层 <p>，所以查子串）
        assert!(body.contains("头发颜色"), "text not converted: {body}");
        assert!(
            !body.contains("头髮") && !body.contains("顏色"),
            "traditional chars not converted: {body}"
        );
        // script 块原样保留（不转）
        assert!(
            body.contains(r#"var x = "不转这里";"#),
            "script mutated: {body}"
        );
    }

    /// source == target → 跳过转换（不引入 zhconv 错误风险）。
    #[test]
    fn render_skips_conversion_when_source_equals_target() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: "头发".into(),
            content: "<p>头发</p>".into(),
            order: 1,
        };
        let (title, body) = render_chapter(
            &raw,
            &RuleChapter::default(),
            RenderTarget::Txt,
            "zh_CN",
            LangType::ZhCn,
        );
        assert_eq!(title, "头发");
        assert!(body.contains("头发"), "should be unchanged: {body}");
    }

    /// source 无法解析 → 跳过转换（保守，不误转）。
    #[test]
    fn render_skips_conversion_when_source_unparseable() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: "头发".into(),
            content: "<p>头发</p>".into(),
            order: 1,
        };
        let (title, body) = render_chapter(
            &raw,
            &RuleChapter::default(),
            RenderTarget::Txt,
            "garbage_lang",
            LangType::ZhCn,
        );
        assert_eq!(title, "头发");
        assert!(body.contains("头发"), "should be unchanged: {body}");
    }
}
