//! TXT 导出。对应 Java `handle.TxtMergeHandler`。
//!
//! 行为：
//! - 输出文件 `<outDir>/<bookName>(<author>).txt`；
//! - 首页插入书籍信息（书名 / 作者 / 简介），然后按章节文件名升序合并；
//! - 编码可选 UTF-8（默认）/ GBK / Big5 等，由 config.txt-encoding 决定。
//!   底层用 `encoding_rs` 转码；目标编码不可识别时降级 UTF-8 + warn。
//!
//! 与 Java 端的差异：
//! - Java 用 hutool `FileAppender` 一次次 append；Rust 这里用 `BufWriter` 按片段
//!   编码写入，避免超长小说整本内容同时驻留内存。
//! - Java 用 `HtmlUtil.cleanHtmlTag(intro)` 去 HTML 标签；
//!   Rust 用一个简单正则去标签 + 去掉 HTML 实体（`&xxx;`），与 `ChapterFilter` 用法一致。

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use encoding_rs::{Encoding, UTF_8};

use crate::export::exporter::{
    ExportError, Exporter, sort_chapter_files, strip_html_tags, unique_path,
};
use crate::models::Book;
use crate::utils::fs::sanitize_filename;

/// 流式转码写入器：将 UTF-8 `&str` 片段即时转码为目标编码写入底层 writer，
/// 避免整本小说先拼成一个大 `String` 再统一编码。
struct TxtEncodedWriter<W: Write> {
    inner: W,
    encoding: &'static Encoding,
    encoding_label: String,
    had_errors: bool,
}

impl<W: Write> TxtEncodedWriter<W> {
    fn new(inner: W, encoding: &'static Encoding, label: &str) -> Self {
        Self {
            inner,
            encoding,
            encoding_label: label.to_string(),
            had_errors: false,
        }
    }

    fn write_str(&mut self, s: &str) -> Result<(), ExportError> {
        let (encoded, _, errors) = self.encoding.encode(s);
        self.had_errors |= errors;
        self.inner.write_all(&encoded)?;
        Ok(())
    }

    fn finish(mut self) -> Result<(), ExportError> {
        if self.had_errors {
            tracing::warn!(
                "TXT 编码 {} 时存在不可表示字符，已替换为占位符。",
                self.encoding_label
            );
        }
        self.inner.flush()?;
        Ok(())
    }
}

pub struct TxtExporter {
    encoding: &'static Encoding,
    encoding_label: String,
}

impl TxtExporter {
    pub fn new(label: &str) -> Self {
        let label = label.trim();
        let label = if label.is_empty() { "UTF-8" } else { label };
        let enc = Encoding::for_label(label.as_bytes()).unwrap_or_else(|| {
            tracing::warn!("未知 txt-encoding `{label}`，降级为 UTF-8");
            UTF_8
        });
        Self {
            encoding: enc,
            encoding_label: enc.name().to_string(),
        }
    }
}

impl Exporter for TxtExporter {
    fn ext(&self) -> &'static str {
        "txt"
    }

    fn merge(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        let files = sort_chapter_files(chapters_dir)?;
        if files.is_empty() {
            return Err(ExportError::EmptyChaptersDir(chapters_dir.to_path_buf()));
        }

        // 转码 + 写入：按首页/章节片段流式编码，避免整本小说拼成一个 String。
        std::fs::create_dir_all(out_dir)?;
        let filename = sanitize_filename(&format!("{}({}).txt", book.book_name, book.author));
        let out_path = unique_path(out_dir, &filename);
        let file = std::fs::File::create(&out_path)?;
        let mut writer =
            TxtEncodedWriter::new(BufWriter::new(file), self.encoding, &self.encoding_label);

        // 首页：书籍信息（与 Java 端首段格式一致）
        writer.write_str(&format!("书名：{}\n", book.book_name))?;
        writer.write_str(&format!("作者：{}\n", book.author))?;
        let intro = book.intro.as_deref().unwrap_or("");
        let intro_clean = strip_html_tags(intro);
        writer.write_str(&format!(
            "简介：{}\n\n",
            if intro_clean.is_empty() {
                "暂无"
            } else {
                intro_clean.as_str()
            }
        ))?;

        // 章节合并（每个文件已是 render::render_txt 的输出：标题 + 缩进段落 + \n）
        for path in &files {
            // 跳过 0_ 开头的辅助文件（封面图、目录索引）
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name.starts_with("0_") || name == "0_目录.txt" {
                    continue;
                }
            }
            let content = std::fs::read_to_string(path)?;
            writer.write_str(&content)?;
            // 章节之间留一空行
            if !content.ends_with("\n\n") {
                writer.write_str("\n")?;
            }
        }
        writer.finish()?;
        Ok(out_path)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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

    fn sample_chapters() -> Vec<RenderedChapter> {
        vec![
            RenderedChapter {
                order: 1,
                title: "第1章 楔子".into(),
                body: "第1章 楔子\n\n\u{3000}\u{3000}正文一\n\u{3000}\u{3000}正文二\n".into(),
            },
            RenderedChapter {
                order: 2,
                title: "第2章 启程".into(),
                body: "第2章 启程\n\n\u{3000}\u{3000}正文三\n".into(),
            },
        ]
    }

    fn write_and_export(encoding: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();

        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Txt).unwrap();
        let exp = TxtExporter::new(encoding);
        let p = exp.merge(&sample_book(), &chapters, &out).unwrap();
        assert!(p.exists(), "expected output file: {}", p.display());
        // 移交所有权前先把生成路径读一遍，避免后面忘记
        let _ = p;
        dir
    }

    #[test]
    fn writes_utf8_txt_with_book_info_and_chapters() {
        let dir = write_and_export("UTF-8");
        let out = dir.path().join("out").join("起航(苹果).txt");
        let bytes = std::fs::read(&out).unwrap();
        let s = std::str::from_utf8(&bytes).unwrap();
        assert!(s.contains("书名：起航"));
        assert!(s.contains("作者：苹果"));
        // intro 的 HTML 标签和实体被清理（`&nbsp;` 整段删除，与 ChapterFilter 一致）
        assert!(s.contains("简介：这是简介"), "got intro line in:\n{s}");
        assert!(s.contains("第1章 楔子"));
        assert!(s.contains("正文一"));
        assert!(s.contains("第2章 启程"));
        assert!(s.contains("正文三"));
    }

    #[test]
    fn empty_intro_falls_back_to_暂无() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Txt).unwrap();

        let book = Book {
            url: "https://x/".into(),
            book_name: "无简介书".into(),
            author: "无名氏".into(),
            intro: None,
            ..Book::default()
        };
        let exp = TxtExporter::new("UTF-8");
        let p = exp.merge(&book, &chapters, &out).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("简介：暂无"));
    }

    #[test]
    fn writes_gbk_when_configured() {
        let dir = write_and_export("GBK");
        let out = dir.path().join("out").join("起航(苹果).txt");
        let bytes = std::fs::read(&out).unwrap();
        // GBK 编码的"中文"两字应在文件里以 0xD6 0xD0 0xCE 0xC4 出现
        // 但我们没写"中文"；先确认 UTF-8 解码失败（GBK 字节不是合法 UTF-8）
        let utf8_decoded = std::str::from_utf8(&bytes);
        assert!(
            utf8_decoded.is_err(),
            "GBK output unexpectedly valid UTF-8 (or content empty)"
        );
        // 用 encoding_rs 还原
        let (cow, _, _) = encoding_rs::GBK.decode(&bytes);
        assert!(cow.contains("书名：起航"));
        assert!(cow.contains("正文一"));
    }

    #[test]
    fn skips_zero_prefixed_files() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Txt).unwrap();
        // 加一个伪封面 / 索引
        std::fs::write(chapters.join("0_目录.txt"), "应该被忽略").unwrap();

        let exp = TxtExporter::new("UTF-8");
        let p = exp.merge(&sample_book(), &chapters, &out).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(!s.contains("应该被忽略"));
        assert!(s.contains("正文一"));
    }

    #[test]
    fn empty_chapters_dir_returns_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();

        let exp = TxtExporter::new("UTF-8");
        let err = exp.merge(&sample_book(), &chapters, &out).unwrap_err();
        assert!(
            matches!(err, ExportError::EmptyChaptersDir(_)),
            "expected EmptyChaptersDir, got {err:?}"
        );
    }

    #[test]
    fn unknown_encoding_falls_back_to_utf8() {
        let exp = TxtExporter::new("not-a-real-encoding");
        // 不抛错；TxtExporter::new 内部会 warn 并用 UTF-8。
        assert_eq!(exp.encoding, encoding_rs::UTF_8);
    }

    #[test]
    fn merge_dedup_output_filename_on_collision() {
        // 同一本书二次导出到同 out_dir：第一次得到 `<book>(<author>).txt`，
        // 第二次因 `unique_path` 加 ` (1)` 后缀，不应覆盖前一次。
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Txt).unwrap();

        let exp = TxtExporter::new("UTF-8");
        let book = sample_book();
        let p1 = exp.merge(&book, &chapters, &out).unwrap();
        let p2 = exp.merge(&book, &chapters, &out).unwrap();

        assert!(p1.exists());
        assert!(p2.exists());
        assert_ne!(p1, p2, "second merge should pick a different filename");
        assert!(p1.file_name().unwrap().to_str().unwrap().ends_with(").txt"));
        assert!(
            p2.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .contains(" (1).txt")
        );
    }
}
