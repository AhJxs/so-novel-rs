//! HTML 导出。对应 Java `handle.HtmlTocHandler`。
//!
//! 行为：
//! - `chapters_dir` 下每章一个 `NNN_.html` 文件（已由 `write_chapter_files` 写好）；
//! - 生成 `0_目录.txt`，列出 `文件名\t\t章节名`；
//! - 把整个 chapters_dir 打包成 zip 放到 out_dir 下，文件名 `<书名>(<作者>).zip`。
//!
//! 与 Java 端差异：
//! - Java 还会下载封面（HTTP）；为了让阶段 3b 不引入网络依赖（封面下载属
//!   阶段 3c 调度层处理或 EPUB 专属），HTML 导出**不**下载封面。
//!   `EpubExporter` 单独处理封面。
//! - Java 用 hutool `ZipUtil.zip` 一行打包；Rust 用 `zip` crate 4.x 走标准 deflate。

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use regex::Regex;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;

use crate::export::exporter::{sort_chapter_files, ExportError, Exporter};
use crate::models::Book;
use crate::util::fs::sanitize_filename;

pub struct HtmlExporter;

impl Exporter for HtmlExporter {
    fn ext(&self) -> &'static str {
        "html"
    }

    fn merge(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        let files = sort_chapter_files(chapters_dir)?;
        if files.iter().filter(|p| is_chapter_html(p)).count() == 0 {
            return Err(ExportError::EmptyChaptersDir(chapters_dir.to_path_buf()));
        }

        // 1. 写目录索引：文件名 \t\t\t\t 章节名
        let toc_path = chapters_dir.join("0_目录.txt");
        let mut toc_lines = vec!["文件名\t\t\t\t章节名".to_string()];
        for f in &files {
            if !is_chapter_html(f) {
                continue;
            }
            let name = f.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            let html = std::fs::read_to_string(f)?;
            let title = extract_title(&html).unwrap_or_else(|| name.to_string());
            // index_.html → 输出文件名是 `<index>_.html`
            toc_lines.push(format!("{name}\t\t{title}"));
        }
        std::fs::write(&toc_path, toc_lines.join("\n"))?;

        // 2. 打包 zip
        std::fs::create_dir_all(out_dir)?;
        let zip_name = sanitize_filename(&format!("{}({}).zip", book.book_name, book.author));
        let zip_path = out_dir.join(zip_name);
        zip_directory(chapters_dir, &zip_path)?;

        Ok(zip_path)
    }
}

fn is_chapter_html(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("html"))
        .unwrap_or(false)
}

/// 从单章 HTML 里提取 `<title>...</title>` 文本。
fn extract_title(html: &str) -> Option<String> {
    static TITLE_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title re"));
    TITLE_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
}

/// 把 `src` 目录下所有文件平铺打包成 `dst` 这个 zip 文件。
fn zip_directory(src: &Path, dst: &Path) -> Result<(), ExportError> {
    let file = File::create(dst)
        .map_err(|e| ExportError::Zip(format!("create {} failed: {e}", dst.display())))?;
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| ExportError::Zip(format!("bad file name: {}", path.display())))?;

        writer
            .start_file(file_name, options)
            .map_err(|e| ExportError::Zip(format!("start_file: {e}")))?;
        let mut buf = Vec::new();
        File::open(&path)?.read_to_end(&mut buf)?;
        writer
            .write_all(&buf)
            .map_err(|e| ExportError::Zip(format!("write: {e}")))?;
    }
    writer
        .finish()
        .map_err(|e| ExportError::Zip(format!("finish: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExportFormat;
    use crate::export::exporter::{write_chapter_files, RenderedChapter};
    use std::io::Read;

    fn sample_book() -> Book {
        Book {
            url: "https://x/".into(),
            book_name: "起航".into(),
            author: "苹果".into(),
            ..Book::default()
        }
    }

    fn sample_chapters() -> Vec<RenderedChapter> {
        // body 里要有 <title> 才能被 extract_title 抽到
        vec![
            RenderedChapter {
                order: 1,
                title: "第1章 楔子".into(),
                body: "<html><head><title>第1章 楔子</title></head><body>1</body></html>".into(),
            },
            RenderedChapter {
                order: 2,
                title: "第2章 启程".into(),
                body: "<html><head><title>第2章 启程</title></head><body>2</body></html>".into(),
            },
        ]
    }

    #[test]
    fn produces_zip_with_chapters_and_toc() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Html).unwrap();

        let exp = HtmlExporter;
        let zip_path = exp.merge(&sample_book(), &chapters, &out).unwrap();
        assert!(zip_path.exists(), "missing zip: {}", zip_path.display());
        assert!(zip_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap()
            .contains("起航"));

        // 打开 zip 验证内容
        let f = File::open(&zip_path).unwrap();
        let mut zr = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..zr.len())
            .map(|i| zr.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "001_.html"));
        assert!(names.iter().any(|n| n == "002_.html"));
        assert!(names.iter().any(|n| n == "0_目录.txt"));

        // 目录文件含两章标题
        let mut toc_buf = String::new();
        zr.by_name("0_目录.txt")
            .unwrap()
            .read_to_string(&mut toc_buf)
            .unwrap();
        assert!(toc_buf.contains("第1章 楔子"));
        assert!(toc_buf.contains("第2章 启程"));
        assert!(toc_buf.contains("001_.html"));
    }

    #[test]
    fn empty_chapters_dir_returns_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();

        let exp = HtmlExporter;
        let err = exp.merge(&sample_book(), &chapters, &out).unwrap_err();
        assert!(matches!(err, ExportError::EmptyChaptersDir(_)));
    }

    #[test]
    fn extract_title_handles_multiline_and_attrs() {
        let html = "<html><head><title id=\"x\">\n  hello\n</title></head></html>";
        assert_eq!(extract_title(html).as_deref(), Some("hello"));
        assert_eq!(extract_title("<html><body>nope</body></html>"), None);
    }
}
