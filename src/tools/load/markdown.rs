//! Markdown 加载：手写解析 YAML front matter，ATX 一至三级标题视作章节标题。

use std::path::Path;

use regex::Regex;

use super::{LoadOptions, stem};
use crate::tools::ToolError;
use crate::tools::encoding::decode_bytes;
use crate::tools::source_book::SourceBook;
use crate::tools::split::split_chapters;

#[derive(Debug, Default)]
struct FrontMatter {
    title: Option<String>,
    author: Option<String>,
    description: Option<String>,
}

pub(super) fn load(
    path: &Path,
    opts: &LoadOptions<'_>,
    regex: &Regex,
) -> Result<SourceBook, ToolError> {
    let bytes = std::fs::read(path)?;
    let (text, _encoding) = decode_bytes(&bytes, opts.source_encoding);
    let (meta, body) = split_front_matter(&text);
    let title = meta.title.unwrap_or_else(|| stem(path));
    let paragraphs = markdown_paragraphs(&body);
    if paragraphs.is_empty() {
        return Err(ToolError::NoContent);
    }
    let chapters = split_chapters(&paragraphs, regex, &title);
    Ok(SourceBook {
        title,
        author: meta.author.unwrap_or_default(),
        intro: meta.description,
        chapters,
        ..SourceBook::default()
    })
}

fn split_front_matter(text: &str) -> (FrontMatter, String) {
    let mut meta = FrontMatter::default();
    let Some(rest) = text.strip_prefix("---") else {
        return (meta, text.to_string());
    };
    let Some(rest) = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
    else {
        return (meta, text.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (meta, text.to_string());
    };
    let (front, after) = rest.split_at(end);
    let body = after
        .trim_start_matches('\n')
        .strip_prefix("---")
        .unwrap_or(after)
        .trim_start_matches('\n')
        .to_string();

    let mut description_lines: Vec<String> = Vec::new();
    let mut in_block_description = false;
    for line in front.lines() {
        if in_block_description {
            if line.starts_with(' ') || line.starts_with('\t') {
                description_lines.push(line.trim().to_string());
                continue;
            }
            in_block_description = false;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "title" if !value.is_empty() => meta.title = Some(unquote(value)),
            "author" if !value.is_empty() => meta.author = Some(unquote(value)),
            "description" if value == "|" => in_block_description = true,
            "description" if !value.is_empty() => meta.description = Some(unquote(value)),
            _ => {}
        }
    }
    if meta.description.is_none() && !description_lines.is_empty() {
        meta.description = Some(description_lines.join("\n"));
    }
    (meta, body)
}

fn unquote(value: &str) -> String {
    value
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn markdown_paragraphs(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut current, &mut out);
            continue;
        }
        if let Some(heading) = atx_heading(trimmed) {
            flush(&mut current, &mut out);
            out.push(heading);
            continue;
        }
        current.push(line.trim_end().to_string());
    }
    flush(&mut current, &mut out);
    out
}

fn flush(current: &mut Vec<String>, out: &mut Vec<String>) {
    if current.is_empty() {
        return;
    }
    let paragraph = current.join("\n");
    let paragraph = paragraph.trim();
    if !paragraph.is_empty() {
        out.push(paragraph.to_string());
    }
    current.clear();
}

fn atx_heading(line: &str) -> Option<String> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let rest = line[hashes..].trim_start();
    let rest = rest.trim_end_matches('#').trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::Path;

    use super::super::{LoadOptions, load_path};

    fn write_md(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn reads_front_matter_and_atx_headings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("剑来.md");
        write_md(
            &path,
            "---\ntitle: 剑来\nauthor: 烽火戏诸侯\ndescription: 一个少年\n---\n\n# 第一章\n\n甲\n\n## 第二节\n\n乙\n",
        );

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "剑来");
        assert_eq!(book.author, "烽火戏诸侯");
        assert_eq!(book.intro.as_deref(), Some("一个少年"));
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "第一章");
        assert_eq!(book.chapters[1].title, "第二节");
    }

    #[test]
    fn markdown_without_front_matter_falls_back_to_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("无元数据.md");
        write_md(&path, "# 第一章\n\n甲\n");
        let book = load_path(&path, &LoadOptions::default()).unwrap();
        assert_eq!(book.title, "无元数据");
        assert_eq!(book.chapters.len(), 1);
    }

    #[test]
    fn multiline_description_is_joined() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("书.md");
        write_md(
            &path,
            "---\ntitle: 书\ndescription: |\n  第一行\n  第二行\n---\n\n# 第一章\n\n甲\n",
        );
        let book = load_path(&path, &LoadOptions::default()).unwrap();
        assert_eq!(book.intro.as_deref(), Some("第一行\n第二行"));
    }
}
