//! Markdown 导出: 合并 `chapters_dir` 下每章 `.md` 为单文件 (与 TXT 同形态)。
//!
//! 多出两点: YAML front matter (Hugo/Jekyll 风格) 与章节锚点 TOC (`- [标题](#chapter-N)`)。

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::export::exporter::{
    ExportError, Exporter, sort_chapter_files, strip_html_tags, unique_path,
};
use crate::models::Book;
use crate::utils::fs::sanitize_filename;

pub struct MdExporter;

impl Exporter for MdExporter {
    fn ext(&self) -> &'static str {
        "md"
    }

    fn merge(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        let files: Vec<PathBuf> = sort_chapter_files(chapters_dir)?
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| !s.starts_with("0_"))
            })
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            })
            .collect();
        if files.is_empty() {
            return Err(ExportError::EmptyChaptersDir(chapters_dir.to_path_buf()));
        }

        // 单遍预读 (title, body): 避免写 TOC + 正文时两次打开同一文件。
        let mut chapters: Vec<(String, String)> = Vec::with_capacity(files.len());
        for path in &files {
            let title = chapter_title_from_path(path);
            let body = std::fs::read_to_string(path)?;
            chapters.push((title, body));
        }

        std::fs::create_dir_all(out_dir)?;
        let out_name = sanitize_filename(&format!("{}({}).md", book.book_name, book.author));
        let out_path = unique_path(out_dir, &out_name);

        // BufWriter 流式写入, 避免把整本书拼成一个大 String (Markdown 是纯 UTF-8, 无需编码转换)。
        // 用 `write_all(format!(...).as_bytes())` 而非 `writeln!`: 后者返回 `fmt::Result`,
        // 而 `ExportError` 只 `From<io::Error>`; `write_all` 的 `io::Result` 能直接 `?`。
        let file = std::fs::File::create(&out_path)?;
        let mut w = BufWriter::new(file);

        w.write_all(b"---\n")?;
        w.write_all(format!("title: {}\n", book.book_name).as_bytes())?;
        w.write_all(format!("author: {}\n", book.author).as_bytes())?;
        if let Some(intro) = book.intro.as_deref() {
            let cleaned = strip_html_tags(intro);
            if !cleaned.is_empty() {
                w.write_all(b"description: |\n")?;
                for line in cleaned.lines() {
                    w.write_all(format!("  {line}\n").as_bytes())?;
                }
            }
        }
        w.write_all(b"---\n\n")?;

        // 顶部 H1: 与 front matter 的 title 保持一致。
        w.write_all(format!("# {}\n\n", book.book_name).as_bytes())?;

        w.write_all("## 目录\n\n".as_bytes())?;
        for (idx, (title, _)) in chapters.iter().enumerate() {
            w.write_all(format!("- [{title}](#chapter-{})\n", idx + 1).as_bytes())?;
        }
        w.write_all(b"\n")?;

        // 每章正文前置内联 HTML 锚点 (GFM / Obsidian / Hugo 均识别); `## 标题` 行由渲染自带。
        for (idx, (_title, body)) in chapters.iter().enumerate() {
            w.write_all(format!("<a id=\"chapter-{}\"></a>\n\n", idx + 1).as_bytes())?;
            w.write_all(body.trim_end().as_bytes())?;
            w.write_all(b"\n\n")?;
        }

        w.flush()?;
        Ok(out_path)
    }
}

/// 从 `001_第1章 起航.md` 抽出 `第1章 起航`。
/// `sanitize_filename` 已把文件系统非法字符替换成 `_`，回看时无需再清洗。
pub(crate) fn chapter_title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.split_once('_').map_or(s, |(_, t)| t).to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use std::path::PathBuf;

    use crate::config::ExportFormat;
    use crate::export::exporter::{
        ExportError, Exporter, RenderedChapter, exporter_for, write_chapter_files,
    };
    use crate::models::Book;

    use super::super::md::MdExporter;

    fn sample_book() -> Book {
        Book {
            url: "https://x/".into(),
            book_name: "起航".into(),
            author: "苹果".into(),
            intro: Some("<p>这是&nbsp;简介</p>".into()),
            ..Book::default()
        }
    }

    fn sample_chapters() -> Vec<RenderedChapter> {
        vec![
            RenderedChapter {
                order: 1,
                title: "第1章 楔子".into(),
                body: "## 第1章 楔子\n\n\u{3000}\u{3000}正文一\n\n\u{3000}\u{3000}正文二\n".into(),
            },
            RenderedChapter {
                order: 2,
                title: "第2章 启程".into(),
                body: "## 第2章 启程\n\n\u{3000}\u{3000}正文三\n".into(),
            },
        ]
    }

    fn write_and_merge() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Markdown).unwrap();
        let exp = MdExporter;
        let p = exp.merge(&sample_book(), &chapters, &out).unwrap();
        (dir, p)
    }

    #[test]
    fn exporter_for_markdown_returns_md_exporter() {
        let exp = exporter_for(ExportFormat::Markdown, "UTF-8");
        assert_eq!(exp.ext(), "md");
    }

    #[test]
    fn merge_writes_yaml_front_matter() {
        let (_dir, p) = write_and_merge();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(
            p.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .ends_with("(苹果).md")
        );
        let header_end = s.find("\n---\n").expect("should contain closing ---");
        let header = &s[..header_end];
        assert!(header.starts_with("---\n"));
        assert!(header.contains("title: 起航"));
        assert!(header.contains("author: 苹果"));
        // intro 已剥离 HTML 标签和实体后作为 description: |
        assert!(header.contains("description: |"));
        assert!(header.contains("  这是简介"));
    }

    #[test]
    fn merge_omits_description_when_intro_empty() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Markdown).unwrap();
        let book = Book {
            book_name: "无简介".into(),
            author: "无名".into(),
            intro: None,
            ..Book::default()
        };
        let p = MdExporter.merge(&book, &chapters, &out).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        let header_end = s.find("\n---\n").unwrap();
        let header = &s[..header_end];
        assert!(!header.contains("description:"));
    }

    #[test]
    fn merge_writes_h1_toc_and_anchor_links() {
        let (_dir, p) = write_and_merge();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("# 起航\n\n"), "缺少书名 H1");
        assert!(s.contains("## 目录\n\n"));
        assert!(s.contains("- [第1章 楔子](#chapter-1)"));
        assert!(s.contains("- [第2章 启程](#chapter-2)"));
    }

    #[test]
    fn merge_embeds_html_anchor_before_each_chapter() {
        let (_dir, p) = write_and_merge();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("<a id=\"chapter-1\"></a>"));
        assert!(s.contains("<a id=\"chapter-2\"></a>"));
    }

    #[test]
    fn chapter_title_from_path_strips_order_prefix() {
        use super::super::md::chapter_title_from_path;
        let s = chapter_title_from_path(std::path::Path::new("001_第1章 起航.md"));
        assert_eq!(s, "第1章 起航");
        // 没下划线时回退整 stem
        let s2 = chapter_title_from_path(std::path::Path::new("无名.md"));
        assert_eq!(s2, "无名");
    }

    #[test]
    fn empty_chapters_dir_returns_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let err = MdExporter
            .merge(&sample_book(), &chapters, &out)
            .unwrap_err();
        assert!(
            matches!(err, ExportError::EmptyChaptersDir(_)),
            "expected EmptyChaptersDir, got {err:?}"
        );
    }

    #[test]
    fn skips_zero_prefixed_files() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Markdown).unwrap();
        // 加一个伪封面 / 索引
        std::fs::write(chapters.join("0_目录.md"), "应该被忽略").unwrap();

        let p = MdExporter.merge(&sample_book(), &chapters, &out).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(!s.contains("应该被忽略"));
        assert!(s.contains("正文一"));
    }

    #[test]
    fn merge_dedup_output_filename_on_collision() {
        // 二次导出到同 out_dir: 因 `unique_path` 加 ` (1)` 后缀, 不应覆盖前一次。
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Markdown).unwrap();

        let book = sample_book();
        let p1 = MdExporter.merge(&book, &chapters, &out).unwrap();
        let p2 = MdExporter.merge(&book, &chapters, &out).unwrap();

        assert!(p1.exists());
        assert!(p2.exists());
        assert_ne!(p1, p2, "second merge should pick a different filename");
        assert!(p1.file_name().unwrap().to_str().unwrap().ends_with(").md"));
        assert!(
            p2.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .contains(" (1).md")
        );
    }
}
