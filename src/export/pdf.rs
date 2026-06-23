//! PDF 导出。对应 Java `handle.PdfMergeHandler`。
//!
//! 行为：
//! - 读 `chapters_dir` 下按文件名升序的章节 HTML 文件（每章一个，由 `write_chapter_files`
//!   在 Pdf 模式下写出 `{order}_.html`，body 用 Html 模板）；
//! - 用 `pdf_oxide` 的 `DocumentBuilder` 直接构建 PDF（不再走 `from_html_css` 的
//!   HTML→DOM→Taffy 管道——该管道对中文小说排版问题多：字号/行距/缩进/分页不受控）。
//!
//! 实现要点：
//! - **结构化内容**：从每章 HTML 抽 `<h1>`(标题) + `<p>`(段落)，剥标签/解码实体得到纯文本，
//!   喂给 DocumentBuilder。不再拼大 HTML 串。
//! - **CJK 字体**：`find_cjk_font` 找系统字体（msyh.ttc / NotoSansCJK / PingFang 等），
//!   `EmbeddedFont::from_data` 解析后 `register_embedded_font("CJK", ...)` 注册。找不到字体
//!   时降级到 Base-14 Helvetica（中文显示为 tofu）并告警，但排版/元数据仍正常。
//! - **元数据**：`DocumentMetadata` 写入 title/author/subject/keywords（`from_html_css`
//!   路径做不到，只能靠封面页 hack）。
//! - **中文换行**：库自带 `wrap_text` 用 `split_whitespace`，对无空格的中文整段不换行会
//!   冲出页面。这里用 `EmbeddedFont::char_width` 逐字量宽，CJK 字符按字断行，ASCII 连续
//!   字母数字作为整体不拆词。
//! - **分页**：维护 `y` 游标，触底自动开新页；封面页 + 每章首页强制分页。
//!
//! 局限：
//! - 无 CJK 字体时中文是 tofu（建议安装 Noto Sans CJK）。
//! - CJK 粗体需第二个字体文件，当前只用常规体；章节标题靠加大字号 + 居中区分。

use std::fs;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use pdf_oxide::writer::{DocumentBuilder, DocumentMetadata, EmbeddedFont, PageSize};
use regex::Regex;

use crate::export::exporter::{
    ExportError, Exporter, sort_chapter_files, strip_html_tags, unique_path,
};
use crate::models::Book;
use crate::util::fs::sanitize_filename;

/// 正文与标题用的 CJK 字体注册名（找不到系统字体时退化为 Base-14 Helvetica）。
const CJK_FONT: &str = "CJK";

pub struct PdfExporter;

impl Exporter for PdfExporter {
    fn ext(&self) -> &'static str {
        "pdf"
    }

    fn merge(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        let files = sort_chapter_files(chapters_dir)?
            .into_iter()
            // 跳过 0_ 开头的辅助文件（封面 / 目录索引），与 html/epub 一致
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| !s.starts_with("0_"))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();
        if files.is_empty() {
            return Err(ExportError::EmptyChaptersDir(chapters_dir.to_path_buf()));
        }

        // 解析每章：抽 <h1> 标题 + <p> 段落，剥成纯文本。
        let mut chapters: Vec<(Option<String>, Vec<String>)> = Vec::with_capacity(files.len());
        for path in &files {
            let raw = fs::read_to_string(path)?;
            let body = extract_body(&raw).unwrap_or_else(|| raw.trim().to_string());
            let body = strip_nav_bar(&body);
            chapters.push(extract_chapter_content(&body));
        }

        std::fs::create_dir_all(out_dir)?;
        let out_name = sanitize_filename(&format!("{}({}).pdf", book.book_name, book.author));
        let out_path = unique_path(out_dir, &out_name);

        // CJK 字体：找到 → 量宽用 EmbeddedFont，注册到 builder；找不到 → 启发式量宽，
        // 不注册字体（中文 tofu 但排版照常）。
        let font_bytes = find_cjk_font();
        let measurer = match &font_bytes {
            Some(b) => Measurer::Embedded(Box::new(
                EmbeddedFont::from_data(Some(CJK_FONT.into()), b.clone())
                    .map_err(|e| ExportError::Pdf(format!("font parse: {e}")))?,
            )),
            None => {
                tracing::warn!(
                    "未找到系统 CJK 字体（C:\\Windows\\Fonts\\msyh.ttc / NotoSansCJK / \
                     PingFang 等），PDF 中中文将显示为方块。建议安装 Noto Sans CJK 后重试。"
                );
                Measurer::Heuristic
            }
        };

        let subject = book
            .intro
            .as_deref()
            .map(|s| strip_html_tags(s).chars().take(200).collect::<String>())
            .unwrap_or_default();
        let metadata = DocumentMetadata::new()
            .title(book.book_name.clone())
            .author(book.author.clone())
            .subject(subject)
            .keywords("so-novel-rs");

        let mut builder = DocumentBuilder::new().metadata(metadata);
        if let Some(b) = &font_bytes {
            // 量宽用的 EmbeddedFont 已被 measurer 持有（非 Clone），这里再解析一份注册
            // 到 builder。解析 msyh.ttc ~20k 字形表，开销可忽略。
            let font = EmbeddedFont::from_data(Some(CJK_FONT.into()), b.clone())
                .map_err(|e| ExportError::Pdf(format!("font parse: {e}")))?;
            builder = builder.register_embedded_font(CJK_FONT, font);
        }

        // 排版 + 流式分页：每填满一页就 flush 到 builder，内存只留当前页的 runs。
        let font_name = if font_bytes.is_some() {
            CJK_FONT
        } else {
            "Helvetica"
        };
        let total_chapters = chapters.len();
        let started = std::time::Instant::now();
        {
            let mut pg = Paginator::new(&mut builder, &measurer, font_name);
            pg.render_cover(book);
            for (idx, (title, paras)) in chapters.iter().enumerate() {
                pg.render_chapter(title.as_deref(), paras);
                tracing::debug!(
                    chapter = idx + 1,
                    total = total_chapters,
                    paragraphs = paras.len(),
                    "PDF 章节排版"
                );
            }
            pg.finish();
        }
        tracing::info!(
            chapters = total_chapters,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "PDF 排版完成"
        );

        let bytes = builder
            .build()
            .map_err(|e| ExportError::Pdf(format!("build: {e}")))?;
        let file = std::fs::File::create(&out_path)?;
        let mut writer = std::io::BufWriter::new(file);
        std::io::Write::write_all(&mut writer, &bytes)?;
        std::io::Write::flush(&mut writer)?;

        Ok(out_path)
    }
}

// ---------------------------------------------------------------------------
// 排版
// ---------------------------------------------------------------------------

/// 单行文本：内容 + 绝对坐标(x=左边距或居中, y=基线) + 字体名 + 字号。
struct Run {
    text: String,
    x: f32,
    y: f32,
    font: &'static str,
    size: f32,
}

/// 字符宽度量宽器。
/// - `Embedded`：用真实 CJK 字体字形度量（精确，CJK 路径）。
/// - `Heuristic`：CJK=1em、ASCII=0.55em、空格=0.3em 的近似（无字体降级路径）。
enum Measurer {
    Embedded(Box<EmbeddedFont>),
    Heuristic,
}

impl Measurer {
    fn char_w(&self, ch: char, size: f32) -> f32 {
        match self {
            Measurer::Embedded(f) => f.char_width(ch as u32) as f32 * size / 1000.0,
            Measurer::Heuristic => {
                if ch == ' ' {
                    0.3 * size
                } else if ch.is_ascii() {
                    0.55 * size
                } else {
                    size // CJK 全角
                }
            }
        }
    }

    fn text_w(&self, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.char_w(c, size)).sum()
    }
}

/// 排版常量（pt，A4 = 595×842）。
const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 56.0; // ≈2cm
const CONTENT_W: f32 = PAGE_W - 2.0 * MARGIN; // 可排宽度

const BODY_SIZE: f32 = 12.0;
const TITLE_SIZE: f32 = 20.0;
const TITLE_LH: f32 = 20.0 * 1.4;
const COVER_TITLE_SIZE: f32 = 30.0;
const COVER_AUTHOR_SIZE: f32 = 14.0;
const COVER_INTRO_SIZE: f32 = 11.0;

/// 流式分页器：边排边把满页 flush 到 DocumentBuilder，内存只占一页。
struct Paginator<'b> {
    builder: &'b mut DocumentBuilder,
    measurer: &'b Measurer,
    font: &'static str,
    y: f32, // 下一行的"顶"坐标（PDF y 向上），触底 flush
    runs: Vec<Run>,
}

impl<'b> Paginator<'b> {
    fn new(builder: &'b mut DocumentBuilder, measurer: &'b Measurer, font: &'static str) -> Self {
        Self {
            builder,
            measurer,
            font,
            y: PAGE_H - MARGIN,
            runs: Vec::new(),
        }
    }

    /// 加一行：先判是否触底（需新页），再落 run、下移游标。
    /// `y` 是行顶，基线 = `y - size`（ascender 约在行顶，留出顶部 margin）。
    fn line(&mut self, text: &str, x: f32, size: f32, lh: f32) {
        if !self.runs.is_empty() && self.y - size < MARGIN {
            self.flush();
        }
        let baseline = self.y - size;
        self.runs.push(Run {
            text: text.to_string(),
            x,
            y: baseline,
            font: self.font,
            size,
        });
        self.y -= lh;
    }

    /// 垂直留白。
    fn space(&mut self, gap: f32) {
        self.y -= gap;
    }

    /// 强制分页（封面后、每章首）。
    fn page_break(&mut self) {
        if !self.runs.is_empty() {
            self.flush();
        }
    }

    /// 居中加一行（标题用）。
    fn line_centered(&mut self, text: &str, size: f32, lh: f32) {
        let w = self.measurer.text_w(text, size);
        let x = ((PAGE_W - w) / 2.0).max(MARGIN);
        self.line(text, x, size, lh);
    }

    /// 一个段落：首行缩进 2em，逐字换行到 CONTENT_W。段后留白。
    fn paragraph(&mut self, text: &str, size: f32) {
        let lh = size * 1.8;
        let indent = 2.0 * size;
        let wrapped = wrap_text(text, CONTENT_W, size, self.measurer);
        for (i, ln) in wrapped.iter().enumerate() {
            let x = if i == 0 { MARGIN + indent } else { MARGIN };
            self.line(ln, x, size, lh);
        }
        self.space(size * 0.5);
    }

    /// 封面页：书名 / 作者 / 简介，之后强制分页。
    fn render_cover(&mut self, book: &Book) {
        self.line_centered(&book.book_name, COVER_TITLE_SIZE, COVER_TITLE_SIZE * 1.3);
        self.space(COVER_TITLE_SIZE * 0.8);
        self.line_centered(&book.author, COVER_AUTHOR_SIZE, COVER_AUTHOR_SIZE * 1.5);
        self.space(COVER_TITLE_SIZE * 2.0);
        if let Some(intro) = book.intro.as_deref() {
            let intro_text = html_to_text(intro);
            if !intro_text.is_empty() {
                self.paragraph(&intro_text, COVER_INTRO_SIZE);
            }
        }
        self.page_break();
    }

    /// 一章：强制新页 → 标题居中 → 各段落。标题为空时跳过标题行。
    fn render_chapter(&mut self, title: Option<&str>, paras: &[String]) {
        self.page_break();
        if let Some(t) = title {
            if !t.is_empty() {
                self.line_centered(t, TITLE_SIZE, TITLE_LH);
                self.space(TITLE_SIZE * 0.8);
            }
        }
        for p in paras {
            if !p.is_empty() {
                self.paragraph(p, BODY_SIZE);
            }
        }
    }

    /// 把当前页 runs 写入 builder，重置游标。
    fn flush(&mut self) {
        if self.runs.is_empty() {
            self.y = PAGE_H - MARGIN;
            return;
        }
        let mut p = self.builder.page(PageSize::A4);
        for r in &self.runs {
            p = p.font(r.font, r.size).at(r.x, r.y).text(&r.text);
        }
        p.done();
        self.runs.clear();
        self.y = PAGE_H - MARGIN;
    }

    /// 收尾：flush 最后一页（哪怕没满）。
    fn finish(&mut self) {
        if !self.runs.is_empty() {
            self.flush();
        }
    }
}

/// 中文换行：逐字累加宽度，超 `max_w` 断行。ASCII 连续字母数字作为整体词不拆
/// （过长单词硬拆）。返回的每行都保证宽度 ≤ `max_w`。
fn wrap_text(s: &str, max_w: f32, size: f32, m: &Measurer) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0.0f32;

    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        // ASCII 词（字母/数字连串）整体处理，避免半路拆 English/URL
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

// ---------------------------------------------------------------------------
// HTML → 结构化内容
// ---------------------------------------------------------------------------

/// 从章节 body HTML 抽 (标题, 段落列表)。
/// - 标题：第一个 `<h1>` 内文（剥标签 + 解码实体）；无则 None。
/// - 段落：所有 `<p>` 内文，剥标签 + 解码实体，丢空。
fn extract_chapter_content(body_html: &str) -> (Option<String>, Vec<String>) {
    static H1_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>").expect("h1 re"));
    static P_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?is)<p[^>]*>(.*?)</p>").expect("p re"));

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

/// HTML 片段 → 纯文本：`<br>` 转空格 → 剥所有标签 → 解码实体 → 折叠空白。
fn html_to_text(html: &str) -> String {
    static BR_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<br\s*/?>").expect("br re"));
    static TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<[^>]+>").expect("tag re"));
    static WS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").expect("ws re"));

    let no_br = BR_RE.replace_all(html, " ");
    let no_tag = TAG_RE.replace_all(&no_br, "");
    let decoded = decode_entities(&no_tag);
    WS_RE.replace_all(decoded.trim(), " ").into_owned()
}

/// 解码常见 HTML 实体 + 数字实体（&#NN; / &#xHH;）。
fn decode_entities(s: &str) -> String {
    static ENT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"&#(x?)([0-9a-fA-F]+);|&([a-zA-Z]+);").expect("ent re"));
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

/// 把单章 HTML 拆出 `<body>` 内部的内容，剥掉外层 `<html>` / `<head>` / DOCTYPE。
///
/// 找不到 `<body>` 时返回 None（由 caller 兜底用原文）。
fn extract_body(html: &str) -> Option<String> {
    static BODY_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<body[^>]*>(.*)</body>").expect("body re"));
    BODY_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 删掉网页模板自带的翻页按钮栏 `<div class="bottom-bar">…上一页…下一页…</div>`。
fn strip_nav_bar(body_html: &str) -> String {
    static NAV_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?is)<div[^>]*class="[^"]*bottom-bar[^"]*"[^>]*>.*?</div>"#).expect("nav re")
    });
    NAV_RE.replace_all(body_html, "").into_owned()
}

/// 跑遍常见系统路径找一份 CJK 字体（TTC/TTF/OTF）。
fn find_cjk_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        // Windows
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\msyhbd.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        r"C:\Windows\Fonts\simfang.ttf",
        // macOS
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/Library/Fonts/Songti.ttc",
        // Linux — 主流发行版
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        // 用户目录（手动放的）
        "fonts/NotoSansCJK-Regular.ttc",
        "assets/NotoSansCJK-Regular.ttc",
    ];
    for path in CANDIDATES {
        let p = Path::new(path);
        if !p.exists() {
            continue;
        }
        match fs::read(p) {
            Ok(bytes) if !bytes.is_empty() => {
                tracing::debug!("使用 CJK 字体: {}", p.display());
                return Some(bytes);
            }
            Ok(_) => continue,
            Err(e) => {
                tracing::debug!("读 CJK 字体 {} 失败：{}", p.display(), e);
                continue;
            }
        }
    }
    None
}

// DocumentBuilder::save / build 返回 pdf_oxide::error::Error，统一转 ExportError::Pdf。
impl From<pdf_oxide::error::Error> for ExportError {
    fn from(e: pdf_oxide::error::Error) -> Self {
        ExportError::Pdf(format!("{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExportFormat;
    use crate::export::exporter::{RenderedChapter, write_chapter_files};

    fn sample_book() -> Book {
        Book {
            url: "https://x/".into(),
            book_name: "起航".into(),
            author: "苹果".into(),
            intro: Some("<p>这是&nbsp;简介</p>".into()),
            ..Book::default()
        }
    }

    /// 模拟 `render_chapter(target=Pdf)` 写出的章节 HTML（Html 模板产物）
    fn sample_html_chapter(title: &str, body: &str) -> String {
        format!(
            r##"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>{title}</title></head>
<body><h1>{title}</h1><p>{body}</p></body></html>"##
        )
    }

    fn write_chapters(dir: &Path) {
        let chapters = vec![
            RenderedChapter {
                order: 1,
                title: "第1章 楔子".into(),
                body: sample_html_chapter("第1章 楔子", "正文一"),
            },
            RenderedChapter {
                order: 2,
                title: "第2章 启程".into(),
                body: sample_html_chapter("第2章 启程", "正文二"),
            },
        ];
        write_chapter_files(dir, &chapters, ExportFormat::Pdf).unwrap();
    }

    #[test]
    fn produces_pdf_file() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapters(&chapters);

        let path = PdfExporter.merge(&sample_book(), &chapters, &out).unwrap();
        assert!(path.exists(), "missing pdf: {}", path.display());
        assert!(
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap()
                .ends_with(".pdf")
        );
        // PDF magic: 文件头 `%PDF-`
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            bytes.starts_with(b"%PDF-"),
            "not a PDF (magic mismatch): {:?}",
            &bytes[..8.min(bytes.len())]
        );
        // 文件名含书名 + 作者
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.contains("起航"), "name missing book: {name}");
        assert!(name.contains("苹果"), "name missing author: {name}");
    }

    #[test]
    fn empty_chapters_dir_returns_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();

        let err = PdfExporter
            .merge(&sample_book(), &chapters, &out)
            .unwrap_err();
        assert!(matches!(err, ExportError::EmptyChaptersDir(_)));
    }

    #[test]
    fn skips_zero_prefixed_aux_files() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapters(&chapters);
        // 加一个 0_ 前缀的"目录"文件 —— 不该被当成章节
        std::fs::write(chapters.join("0_目录.txt"), "ignore me").unwrap();

        let path = PdfExporter.merge(&sample_book(), &chapters, &out).unwrap();
        assert!(path.exists());
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len > 100, "pdf too small: {len} bytes");
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
    fn strip_nav_bar_removes_paging_buttons() {
        let html = r#"<h1>第1章</h1><p>正文</p>\
<div class="bottom-bar d-flex justify-content-between"><button id="btn-pre">上一页</button><button id="btn-next">下一页</button></div>"#;
        let out = strip_nav_bar(html);
        assert!(out.contains("正文"), "body dropped: {out}");
        assert!(!out.contains("上一页"), "prev button not stripped: {out}");
        assert!(!out.contains("下一页"), "next button not stripped: {out}");
        assert!(!out.contains("bottom-bar"), "bar div not stripped: {out}");
    }

    #[test]
    fn strip_nav_bar_preserves_content_without_bar() {
        let html = "<h1>第1章</h1><p>正文</p>";
        let out = strip_nav_bar(html);
        assert_eq!(out, html);
    }

    #[test]
    fn strip_html_tags_cleans_intro() {
        let s = "<p>这是&nbsp;简介</p>";
        assert_eq!(strip_html_tags(s), "这是简介");
        let long: String = "中".repeat(500);
        let cleaned = strip_html_tags(&format!("<p>{long}</p>"));
        assert_eq!(cleaned.chars().count(), 500);
        // 截短到 200 字由调用方负责
        let truncated: String = cleaned.chars().take(200).collect();
        assert_eq!(truncated.chars().count(), 200);
    }

    #[test]
    fn html_to_text_strips_tags_and_decodes_entities() {
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
    fn extract_chapter_content_pulls_title_and_paras() {
        let body = r#"<h1>第1章 楔子</h1><p>正文一</p><p>正文二</p>"#;
        let (title, paras) = extract_chapter_content(body);
        assert_eq!(title.as_deref(), Some("第1章 楔子"));
        assert_eq!(paras, vec!["正文一", "正文二"]);
    }

    #[test]
    fn extract_chapter_content_handles_entities_in_para() {
        let body = r#"<h1>T</h1><p>a&amp;b</p>"#;
        let (_, paras) = extract_chapter_content(body);
        assert_eq!(paras, vec!["a&b"]);
    }

    #[test]
    fn extract_chapter_content_no_h1_returns_none_title() {
        let body = "<p>只有段落</p>";
        let (title, paras) = extract_chapter_content(body);
        assert!(title.is_none());
        assert_eq!(paras, vec!["只有段落"]);
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

    #[test]
    fn long_book_produces_multi_page_pdf() {
        // 足够长的正文 → 必然超过一页，验证分页 + 内容真的写进去了。
        let para: String = "这是一段足够长的中文正文用来填满整页以便触发分页。".repeat(40);
        let body = format!(
            r##"<!DOCTYPE html><html><head></head><body><h1>长章</h1>{}</body></html>"##,
            (0..30)
                .map(|i| format!("<p>第{i}段：{para}</p>"))
                .collect::<Vec<_>>()
                .join("")
        );
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(
            &chapters,
            &[RenderedChapter {
                order: 1,
                title: "长章".into(),
                body,
            }],
            ExportFormat::Pdf,
        )
        .unwrap();

        let path = PdfExporter.merge(&sample_book(), &chapters, &out).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"), "bad magic");
        assert!(
            bytes.windows(5).any(|w| w == b"%%EOF"),
            "missing %%EOF trailer"
        );

        // 读回页数：长内容必须 > 1 页。
        let mut pdf = pdf_oxide::api::Pdf::open(&path).expect("open produced pdf");
        let pages = pdf.page_count().expect("page count");
        assert!(pages > 1, "expected multi-page pdf, got {pages} page(s)");
    }
}
