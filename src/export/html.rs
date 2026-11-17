//! HTML 导出。对应 Java `handle.HtmlTocHandler`。
//!
//! 每章一个 `NNN_.html`; 先生成 `0_目录.txt` (列出 `文件名\t\t章节名`), 再一起打包成
//! `<书名>(<作者>).zip` 放入 `out_dir`。章节直接来自内存, 不落章节缓存目录。
//!
//! HTML 导出**不**下载封面 —— 网络属调度层, 封面归 `EpubExporter`。

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::export::exporter::{ExportError, Exporter, RenderedChapter, unique_path};
use crate::models::Book;
use crate::utils::fs;

/// 把章节打包成单文件 HTML (zip)。
pub struct HtmlExporter;

impl Exporter for HtmlExporter {
    fn ext(&self) -> &'static str {
        "html"
    }

    fn merge(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        if chapters.is_empty() {
            return Err(ExportError::NoChapters);
        }

        // 目录索引: 文件名 + 章节名 (TAB 分隔)
        let mut toc_lines = vec!["文件名\t\t\t\t章节名".to_string()];
        for chapter in chapters {
            let name = format!("{:03}_.html", chapter.order);
            toc_lines.push(format!("{name}\t\t{}", chapter.title));
        }

        std::fs::create_dir_all(out_dir)?;
        let zip_name = fs::sanitize_filename(&format!("{}({}).zip", book.book_name, book.author));
        let zip_path = unique_path(out_dir, &zip_name);

        let file = File::create(&zip_path)
            .map_err(|e| ExportError::Zip(format!("create {} failed: {e}", zip_path.display())))?;
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        writer
            .start_file("0_目录.txt", options)
            .map_err(|e| ExportError::Zip(format!("start_file: {e}")))?;
        writer
            .write_all(toc_lines.join("\n").as_bytes())
            .map_err(|e| ExportError::Zip(format!("write: {e}")))?;

        for chapter in chapters {
            let name = format!("{:03}_.html", chapter.order);
            writer
                .start_file(&name, options)
                .map_err(|e| ExportError::Zip(format!("start_file: {e}")))?;
            writer
                .write_all(chapter.body.as_bytes())
                .map_err(|e| ExportError::Zip(format!("write: {e}")))?;
        }

        writer
            .finish()
            .map_err(|e| ExportError::Zip(format!("finish: {e}")))?;
        Ok(zip_path)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use std::io::Read;

    use zip::ZipArchive;

    use super::*;
    use crate::config::ExportFormat;
    use crate::export::exporter::{RenderedChapter, write_chapter_files};

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

    fn load_and_merge(book: &Book, dir: &Path, out: &Path) -> PathBuf {
        let chapters = crate::export::load_chapter_files(dir, ExportFormat::Html).unwrap();
        HtmlExporter.merge(book, &chapters, out).unwrap()
    }

    #[test]
    fn produces_zip_with_chapters_and_toc() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Html).unwrap();

        let zip_path = load_and_merge(&sample_book(), &chapters, &out);
        assert!(zip_path.exists(), "missing zip: {}", zip_path.display());
        assert!(
            zip_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap()
                .contains("起航")
        );

        let f = File::open(&zip_path).unwrap();
        let mut zr = ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..zr.len())
            .map(|i| zr.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "001_.html"));
        assert!(names.iter().any(|n| n == "002_.html"));
        assert!(names.iter().any(|n| n == "0_目录.txt"));

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
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();

        let err = HtmlExporter.merge(&sample_book(), &[], &out).unwrap_err();
        assert!(matches!(err, ExportError::NoChapters));
    }
}
