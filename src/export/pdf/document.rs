//! PDF 文档构建主流程 (PR #17 拆分, 2026-07-08).
//!
//! `PdfExporter` + `Paginator` (流式分页) + `Run` (单行文本) + 排版常量。

use std::fs;
use std::path::{Path, PathBuf};

use pdf_oxide::writer::{DocumentBuilder, DocumentMetadata, EmbeddedFont, PageSize};

use crate::export::exporter::{
    ExportError, Exporter, sort_chapter_files, strip_html_tags, unique_path,
};
use crate::models::Book;
use crate::utils::fs::sanitize_filename;

use super::chapters::{extract_body, extract_chapter_content, html_to_text, strip_nav_bar, wrap_text};
use super::fonts::{CJK_FONT, Measurer, find_cjk_font};

/// PDF 导出器 (实现 [`Exporter`] trait)。
pub struct PdfExporter;

impl Exporter for PdfExporter {
    fn ext(&self) -> &'static str {
        "pdf"
    }

    #[tracing::instrument(
        name = "pdf_merge",
        skip_all,
        fields(book = %book.book_name, chapters_dir = %chapters_dir.display())
    )]
    fn merge(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        let files = sort_chapter_files(chapters_dir)?
            .into_iter()
            // 跳过 0_ 开头的辅助文件 (封面 / 目录索引), 与 html/epub 一致
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

        // 解析每章: 抽 <h1> 标题 + <p> 段落, 剥成纯文本。
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

        // CJK 字体: 找到 → 量宽用 EmbeddedFont, 注册到 builder; 找不到 → 启发式量宽,
        // 不注册字体 (中文 tofu 但排版照常)。
        let font_bytes = find_cjk_font();
        let measurer = match &font_bytes {
            Some(b) => Measurer::Embedded {
                font: Box::new(
                    EmbeddedFont::from_data(Some(CJK_FONT.into()), b.clone())
                        .map_err(|e| ExportError::Pdf(format!("font parse: {e}")))?,
                ),
                width_cache: std::cell::RefCell::new(std::collections::HashMap::new()),
            },
            None => {
                tracing::warn!(
                    "未找到系统 CJK 字体（msyh.ttc / NotoSansCJK / \
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
            // 量宽用的 EmbeddedFont 已被 measurer 持有 (非 Clone), 这里再解析一份注册
            // 到 builder。解析 msyh.ttc ~20k 字形表, 开销可忽略。
            let font = EmbeddedFont::from_data(Some(CJK_FONT.into()), b.clone())
                .map_err(|e| ExportError::Pdf(format!("font parse: {e}")))?;
            builder = builder.register_embedded_font(CJK_FONT, font);
        }

        // 排版 + 流式分页: 每填满一页就 flush 到 builder, 内存只留当前页的 runs。
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

/// 单行文本: 内容 + 绝对坐标 (x=左边距或居中, y=基线) + 字体名 + 字号。
struct Run {
    text: String,
    x: f32,
    y: f32,
    font: &'static str,
    size: f32,
}

/// 排版常量 (pt, A4 = 595×842)。
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

/// 流式分页器: 边排边把满页 flush 到 DocumentBuilder, 内存只占一页。
struct Paginator<'b> {
    builder: &'b mut DocumentBuilder,
    measurer: &'b Measurer,
    font: &'static str,
    y: f32, // 下一行的"顶"坐标 (PDF y 向上), 触底 flush
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

    /// 加一行: 先判是否触底 (需新页), 再落 run、下移游标。
    /// `y` 是行顶, 基线 = `y - size` (ascender 约在行顶, 留出顶部 margin)。
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

    /// 强制分页 (封面后、每章首)。
    fn page_break(&mut self) {
        if !self.runs.is_empty() {
            self.flush();
        }
    }

    /// 居中加一行 (标题用)。
    fn line_centered(&mut self, text: &str, size: f32, lh: f32) {
        let w = self.measurer.text_w(text, size);
        let x = ((PAGE_W - w) / 2.0).max(MARGIN);
        self.line(text, x, size, lh);
    }

    /// 一个段落: 首行缩进 2em, 逐字换行到 CONTENT_W。段后留白。
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

    /// 封面页: 书名 / 作者 / 简介, 之后强制分页。
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

    /// 一章: 强制新页 → 标题居中 → 各段落。标题为空时跳过标题行。
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

    /// 把当前页 runs 写入 builder, 重置游标。
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

    /// 收尾: flush 最后一页 (哪怕没满)。
    fn finish(&mut self) {
        if !self.runs.is_empty() {
            self.flush();
        }
    }
}

/// `DocumentBuilder::build` 返回 `pdf_oxide::error::Error`, 统一转 `ExportError::Pdf`。
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

    /// 模拟 `render_chapter(target=Pdf)` 写出的章节 HTML (Html 模板产物)
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
        // 加一个 0_ 前缀的"目录"文件 — 不该被当成章节
        std::fs::write(chapters.join("0_目录.txt"), "ignore me").unwrap();

        let path = PdfExporter.merge(&sample_book(), &chapters, &out).unwrap();
        assert!(path.exists());
        let len = std::fs::metadata(&path).unwrap().len();
        assert!(len > 100, "pdf too small: {len} bytes");
    }
}
