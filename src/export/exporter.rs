//! 导出器统一抽象 + 共用工具。
//!
//! 流程划分（与 Java `Crawler` + `CrawlerPostHandler` 等价）：
//! 1. 渲染层（`render`）把章节正文转成"目标格式的字符串"。
//! 2. 暂存层：`write_chapter_files` 把每章的字符串写入临时章节目录，
//!    文件名形如 `001_标题.html` / `001.html` / `001_标题.txt`。
//! 3. 合并层：实现 `Exporter` trait 的具体导出器（txt/html/epub）从该目录
//!    读取所有章节，输出最终文件。
//!
//! 这一层只做**同步**文件 IO；网络下载（如 EPUB 封面）在 epub.rs 内自己处理。
//! 调度（并发抓取 + 重试）在阶段 3c 的 `crawler` 模块里。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use thiserror::Error;

use crate::config::ExportFormat;
use crate::models::Book;
use crate::util::fs::sanitize_filename;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("章节缓存目录为空: {0}")]
    EmptyChaptersDir(PathBuf),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("EPUB 生成失败: {0}")]
    Epub(String),
    #[error("ZIP 打包失败: {0}")]
    Zip(String),
    #[error("编码转换失败: {0}")]
    Encoding(String),
    #[error("PDF 生成失败: {0}")]
    Pdf(String),
}

/// 导出器统一接口。
///
/// 实现方读取 `chapters_dir` 下按文件名升序排序的章节文件，写入 `out_dir`
/// 下的最终成品（`<书名>(<作者>).<ext>` 或一个目录）。返回成品路径。
pub trait Exporter {
    /// 文件扩展名（不带点）：`txt`/`html`/`epub`。`html` 模式实际产物是 zip。
    fn ext(&self) -> &'static str;

    /// 执行合并（不带封面）。
    fn merge(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError>;

    /// 带封面的合并入口。默认实现忽略封面，与 `merge` 等价；
    /// EPUB 导出器覆写此方法以把封面字节写入电子书。
    ///
    /// `cover_bytes`：调用方（阶段 3c 调度层）负责下载封面字节，下载失败时
    /// 传 `None`，由实现者降级（不带封面继续）。
    fn merge_with_cover(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
        _cover_bytes: Option<&[u8]>,
    ) -> Result<PathBuf, ExportError> {
        self.merge(book, chapters_dir, out_dir)
    }
}

/// 按 `ExportFormat` 选择导出器。Pdf 用 `pdf_oxide` 真生成 PDF（章节文件由
/// `render_chapter(target=Pdf)` 写出成 Html，`PdfExporter` 内部再合并成 PDF）。
pub fn exporter_for(format: ExportFormat, txt_encoding: &str) -> Box<dyn Exporter + Send + Sync> {
    match format {
        ExportFormat::Txt => Box::new(super::txt::TxtExporter::new(txt_encoding)),
        ExportFormat::Html => Box::new(super::html::HtmlExporter),
        ExportFormat::Epub => Box::new(super::epub::EpubExporter),
        ExportFormat::Pdf => Box::new(super::pdf::PdfExporter),
    }
}

/// 把"已渲染章节"列表写入 `chapters_dir`，文件名前缀为前导零的 order，
/// 后缀根据格式决定。返回写入的文件总数。
///
/// 与 Java 端 `Crawler#generateChapterPath` 命名约定一致：
/// - `html` → `001_.html`（下划线后无标题，便于翻页脚本拼前缀）
/// - `txt`  → `001_<标题>.txt`
/// - `epub` → `001_<标题>.html`（XHTML 内容，最终被 epub merge 引用）
/// - `pdf`  → 同 html（阶段 1 锁定不实现）
pub fn write_chapter_files(
    chapters_dir: &Path,
    rendered: &[RenderedChapter],
    format: ExportFormat,
) -> Result<usize> {
    if !chapters_dir.exists() {
        std::fs::create_dir_all(chapters_dir)
            .with_context(|| format!("create chapters dir: {}", chapters_dir.display()))?;
    }
    let total = rendered.len();
    let digit_count = total.to_string().len().max(3); // 至少 3 位

    for ch in rendered {
        let order = pad_zero(ch.order, digit_count);
        let safe_title = sanitize_filename(&ch.title);
        let filename = match format {
            ExportFormat::Html => format!("{order}_.html"),
            ExportFormat::Txt => format!("{order}_{safe_title}.txt"),
            ExportFormat::Epub => format!("{order}_{safe_title}.html"),
            ExportFormat::Pdf => format!("{order}_.html"),
        };
        let path = unique_path(chapters_dir, &filename);
        std::fs::write(&path, &ch.body)
            .with_context(|| format!("write chapter file {}", path.display()))?;
    }
    Ok(total)
}

/// 按"前缀数字"升序枚举目录下的章节文件。
/// 跳过 `0_`（书目索引、封面这类）开头的辅助文件以避免被当成正文章节。
pub fn sort_chapter_files(dir: &Path) -> Result<Vec<PathBuf>, ExportError> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();

    files.sort_by_key(|p| {
        p.file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.split('_').next())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });
    Ok(files)
}

/// 工厂：用书名 + 作者构造目录名。与 Java 端
/// `"%s (%s) %s".formatted(bookName, author, extName.toUpperCase())` 一致。
pub fn build_book_dir_name(book: &Book, format: ExportFormat) -> String {
    let raw = format!(
        "{} ({}) {}",
        book.book_name,
        book.author,
        format.as_lower().to_uppercase()
    );
    sanitize_filename(&raw)
}

/// 单个已渲染章节。
///
/// `body` 已是目标格式的字符串（来自 `export::render::render_chapter`），
/// `title` 是 ChapterFilter 重写过的最终标题。
#[derive(Debug, Clone)]
pub struct RenderedChapter {
    pub order: u32,
    pub title: String,
    pub body: String,
}

/// 如果 `dir/filename` 已存在，追加 ` (1)` / ` (2)` 后缀直到不冲突。
/// 正常路径（无冲突）零开销：只做一次 `exists()` 检查。
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let path = dir.join(filename);
    if !path.exists() {
        return path;
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("chapter");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("html");
    for i in 1u32.. {
        let candidate = dir.join(format!("{stem} ({i}).{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

pub(crate) fn pad_zero(n: u32, width: usize) -> String {
    let s = n.to_string();
    if s.len() >= width {
        s
    } else {
        let mut out = String::with_capacity(width);
        for _ in 0..(width - s.len()) {
            out.push('0');
        }
        out.push_str(&s);
        out
    }
}

/// 极简 HTML 标签清理（仅用于 intro 文本）：删除 `<...>`、HTML 实体、多余空白。
/// 不引入 ammonia / scraper（intro 一般几百字，正则足够）。
pub(crate) fn strip_html_tags(s: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    static TAG_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<[^>]+>").expect("strip tag re"));
    static ENTITY_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"&[^;]+;").expect("strip entity re"));
    static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("ws re"));

    let no_tag = TAG_RE.replace_all(s, "").into_owned();
    let no_ent = ENTITY_RE.replace_all(&no_tag, "").into_owned();
    WS_RE.replace_all(&no_ent, " ").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_zero_basic() {
        assert_eq!(pad_zero(1, 3), "001");
        assert_eq!(pad_zero(99, 3), "099");
        assert_eq!(pad_zero(1234, 3), "1234"); // 超出位宽不截断
    }

    #[test]
    fn build_book_dir_name_includes_format() {
        let book = Book {
            book_name: "起航".into(),
            author: "苹果".into(),
            ..Book::default()
        };
        let s = build_book_dir_name(&book, ExportFormat::Epub);
        assert!(s.contains("起航"));
        assert!(s.contains("苹果"));
        assert!(s.contains("EPUB"));
    }

    #[test]
    fn build_book_dir_name_strips_path_separators() {
        let book = Book {
            book_name: "a/b\\c".into(),
            author: "x".into(),
            ..Book::default()
        };
        let s = build_book_dir_name(&book, ExportFormat::Txt);
        assert!(!s.contains('/'));
        assert!(!s.contains('\\'));
    }

    #[test]
    fn write_chapter_files_creates_padded_filenames_for_html() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = vec![
            RenderedChapter {
                order: 1,
                title: "起".into(),
                body: "<html>1</html>".into(),
            },
            RenderedChapter {
                order: 2,
                title: "承".into(),
                body: "<html>2</html>".into(),
            },
        ];
        let count = write_chapter_files(dir.path(), &rendered, ExportFormat::Html).unwrap();
        assert_eq!(count, 2);
        // 至少 3 位前导零（即使章节数 < 1000）
        assert!(dir.path().join("001_.html").exists());
        assert!(dir.path().join("002_.html").exists());
    }

    #[test]
    fn write_chapter_files_creates_titled_filenames_for_txt() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = vec![RenderedChapter {
            order: 5,
            title: "起航".into(),
            body: "正文".into(),
        }];
        write_chapter_files(dir.path(), &rendered, ExportFormat::Txt).unwrap();
        assert!(dir.path().join("005_起航.txt").exists());
    }

    #[test]
    fn sort_chapter_files_by_numeric_prefix() {
        let dir = tempfile::tempdir().unwrap();
        for n in [10u32, 1, 2, 11] {
            let order_str = pad_zero(n, 3);
            std::fs::write(dir.path().join(format!("{order_str}_.html")), "x").unwrap();
        }
        let files = sort_chapter_files(dir.path()).unwrap();
        let names: Vec<_> = files
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(String::from))
            .collect();
        assert_eq!(
            names,
            vec!["001_.html", "002_.html", "010_.html", "011_.html"]
        );
    }

    #[test]
    fn sort_chapter_files_handles_zero_prefix() {
        // 0_ 前缀（封面、目录索引）排在最前面：sort 用 numeric prefix → 0 < 任何章节序号
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("001_.html"), "x").unwrap();
        std::fs::write(dir.path().join("0_目录.txt"), "x").unwrap();
        let files = sort_chapter_files(dir.path()).unwrap();
        let first = files[0].file_name().unwrap().to_string_lossy().into_owned();
        assert!(first.starts_with("0_"));
    }

    #[test]
    fn write_chapter_files_deduplicates_same_title() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = vec![
            RenderedChapter {
                order: 1,
                title: "第1章 楔子".into(),
                body: "<html>first</html>".into(),
            },
            RenderedChapter {
                order: 1,
                title: "第1章 楔子".into(),
                body: "<html>second</html>".into(),
            },
        ];
        let count = write_chapter_files(dir.path(), &rendered, ExportFormat::Epub).unwrap();
        assert_eq!(count, 2);
        // 原文件 + 去重文件都应存在
        let original = dir.path().join("001_第1章 楔子.html");
        let deduped = dir.path().join("001_第1章 楔子 (1).html");
        assert!(original.exists(), "original missing");
        assert!(deduped.exists(), "deduped missing");
        // 内容不同
        assert_eq!(
            std::fs::read_to_string(&original).unwrap(),
            "<html>first</html>"
        );
        assert_eq!(
            std::fs::read_to_string(&deduped).unwrap(),
            "<html>second</html>"
        );
    }

    #[test]
    fn unique_path_returns_input_when_no_collision() {
        let dir = tempfile::tempdir().unwrap();
        let p = unique_path(dir.path(), "book.epub");
        assert_eq!(p, dir.path().join("book.epub"));
    }

    #[test]
    fn unique_path_adds_suffix_on_collision() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("book.epub"), b"old").unwrap();
        let p = unique_path(dir.path(), "book.epub");
        assert_eq!(p, dir.path().join("book (1).epub"));
    }

    #[test]
    fn unique_path_increments_suffix_chain() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("book.epub"), b"old").unwrap();
        std::fs::write(dir.path().join("book (1).epub"), b"old1").unwrap();
        let p = unique_path(dir.path(), "book.epub");
        assert_eq!(p, dir.path().join("book (2).epub"));
    }

    #[test]
    fn unique_path_preserves_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("data.txt"), b"x").unwrap();
        let p = unique_path(dir.path(), "data.txt");
        assert_eq!(p, dir.path().join("data (1).txt"));
    }

    #[test]
    fn unique_path_handles_filename_without_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), b"x").unwrap();
        let p = unique_path(dir.path(), "README");
        // stem=README, ext=None → fallback "html" used
        assert_eq!(p, dir.path().join("README (1).html"));
    }
}
