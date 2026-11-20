//! 纯文本加载：BOM/显式编码/chardetng 解码，按空行分段（全篇无空行时按行分段）。

use std::path::Path;

use regex::Regex;

use super::{LoadOptions, stem};
use crate::tools::ToolError;
use crate::tools::encoding::decode_bytes;
use crate::tools::source_book::SourceBook;
use crate::tools::split::split_chapters;

pub(super) fn load(
    path: &Path,
    opts: &LoadOptions<'_>,
    regex: &Regex,
) -> Result<SourceBook, ToolError> {
    let bytes = std::fs::read(path)?;
    let (text, encoding) = decode_bytes(&bytes, opts.source_encoding);
    let paragraphs = paragraphs_from_text(&text, regex);
    if paragraphs.is_empty() {
        return Err(ToolError::NoContent);
    }
    let title = stem(path);
    let chapters = split_chapters(&paragraphs, regex, &title);
    Ok(SourceBook {
        title,
        chapters,
        encoding: Some(encoding.name().to_string()),
        ..SourceBook::default()
    })
}

fn paragraphs_from_text(text: &str, regex: &Regex) -> Vec<String> {
    let has_blank_line = text.lines().any(|line| line.trim().is_empty());
    let mut paragraphs = Vec::new();
    if has_blank_line {
        let mut current: Vec<&str> = Vec::new();
        let flush = |current: &mut Vec<&str>, paragraphs: &mut Vec<String>| {
            if !current.is_empty() {
                paragraphs.push(current.join("\n").trim().to_string());
                current.clear();
            }
        };
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                flush(&mut current, &mut paragraphs);
            } else if regex.is_match(trimmed) {
                // 标题行必须独占一段，否则 split_chapters 的单行判据会漏掉它。
                flush(&mut current, &mut paragraphs);
                paragraphs.push(trimmed.to_string());
            } else {
                current.push(line.trim_end());
            }
        }
        flush(&mut current, &mut paragraphs);
    } else {
        for line in text.lines() {
            let line = line.trim();
            if !line.is_empty() {
                paragraphs.push(line.to_string());
            }
        }
    }
    paragraphs.retain(|paragraph| !paragraph.is_empty());
    paragraphs
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::super::{LoadOptions, load_path};

    fn write_gbk(path: &std::path::Path, text: &str) {
        let (bytes, _, _) = encoding_rs::GBK.encode(text);
        std::fs::write(path, &bytes).unwrap();
    }

    #[test]
    fn loads_gbk_txt_and_splits_chapters() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("测试书.txt");
        write_gbk(
            &path,
            "简介\n\n第一章 起航\n\n第一段\n\n第2章 风\n\n第二段\n",
        );

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "测试书");
        assert_eq!(book.chapters.len(), 3);
        assert_eq!(book.chapters[0].title, "前言");
        assert_eq!(book.chapters[0].paragraphs, vec!["简介"]);
        assert_eq!(book.chapters[1].title, "第一章 起航");
        assert_eq!(book.chapters[1].paragraphs, vec!["第一段"]);
        assert_eq!(book.chapters[2].title, "第2章 风");
    }

    #[test]
    fn txt_without_blank_lines_uses_one_paragraph_per_line() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("无空行.txt");
        std::fs::write(&path, "第一段\n第二段\n").unwrap();

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.chapters[0].title, "无空行");
        assert_eq!(book.chapters[0].paragraphs, vec!["第一段", "第二段"]);
    }

    #[test]
    fn heading_is_not_glued_to_following_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("novel.txt");
        // 标题后没有空行：中文小说最常见的排版。
        std::fs::write(&path, "第一章 起\n正文甲\n\n正文乙\n第二章 到\n正文丙\n").unwrap();

        let book = load_path(
            &path,
            &LoadOptions {
                source_encoding: Some(encoding_rs::UTF_8),
                ..LoadOptions::default()
            },
        )
        .unwrap();

        let titles: Vec<_> = book.chapters.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, vec!["第一章 起", "第二章 到"]);
        assert_eq!(book.chapters[0].paragraphs, vec!["正文甲", "正文乙"]);
        assert_eq!(book.chapters[1].paragraphs, vec!["正文丙"]);
    }

    #[test]
    fn records_actual_decoded_encoding() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bom.txt");
        // UTF-8 BOM 优先于显式 GBK：记录的必须是实际解码用的编码。
        std::fs::write(
            &path,
            b"\xEF\xBB\xBF\xE7\xAC\xAC\xE4\xB8\x80\xE7\xAB\xA0\n\xE6\xAD\xA3\xE6\x96\x87",
        )
        .unwrap();

        let book = load_path(
            &path,
            &LoadOptions {
                source_encoding: Some(encoding_rs::GBK),
                ..LoadOptions::default()
            },
        )
        .unwrap();

        assert_eq!(book.encoding.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn empty_txt_returns_no_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("空.txt");
        std::fs::write(&path, "  \n\n  ").unwrap();
        let result = load_path(&path, &LoadOptions::default());
        assert!(matches!(result, Err(crate::tools::ToolError::NoContent)));
    }
}
