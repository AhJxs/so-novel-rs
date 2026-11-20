//! HTML / ZIP 加载。ZIP 优先读导出器生成的 `0_目录.txt`，否则按文件名数字前缀排序。

use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use zip::ZipArchive;

use super::{LoadOptions, stem};
use crate::tools::ToolError;
use crate::tools::encoding::decode_bytes;
use crate::tools::source_book::{SourceBook, SourceChapter};
use crate::tools::split::split_chapters;

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?s)<[^>]+>"));
static BODY_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<body\b[^>]*>(.*?)</body>"));
static SCRIPT_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<script\b[^>]*>.*?</script>"));
static STYLE_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<style\b[^>]*>.*?</style>"));
static COMMENT_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<!--.*?-->"));
static BR_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?i)<br\s*/?>"));
static BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile_regex(
        r"(?is)</?(?:p|div|section|article|li|ul|ol|tr|td|th|table|blockquote|pre)\b[^>]*>",
    )
});
static H1_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<h1\b[^>]*>(.*?)</h1>"));
static H2_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<h2\b[^>]*>(.*?)</h2>"));
static H3_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<h3\b[^>]*>(.*?)</h3>"));
static TITLE_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<title\b[^>]*>(.*?)</title>"));

pub(super) fn load_file(
    path: &Path,
    opts: &LoadOptions<'_>,
    regex: &Regex,
) -> Result<SourceBook, ToolError> {
    let bytes = std::fs::read(path)?;
    let (html, _encoding) = decode_bytes(&bytes, opts.source_encoding);
    let title = extract_tag_text(&html, "title").unwrap_or_else(|| stem(path));
    let author = meta_content(&html, "author").unwrap_or_default();
    let intro = meta_content(&html, "description");
    let paragraphs = html_to_paragraphs(body_fragment(&html));
    if paragraphs.is_empty() {
        return Err(ToolError::NoContent);
    }
    let chapters = split_chapters(&paragraphs, regex, &title);
    Ok(SourceBook {
        title,
        author,
        intro,
        chapters,
        ..SourceBook::default()
    })
}

pub(super) fn load_zip(path: &Path) -> Result<SourceBook, ToolError> {
    let file = std::fs::File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| ToolError::ZipStructure(error.to_string()))?;

    let mut names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| ToolError::ZipStructure(error.to_string()))?;
        if entry.is_file() {
            let name = entry
                .name()
                .map_err(|error| ToolError::ZipStructure(error.to_string()))?;
            names.push(name.to_string());
        }
    }

    let mut toc: Vec<(String, String)> = Vec::new();
    if let Some(toc_name) = names
        .iter()
        .find(|name| base_name(name) == "0_目录.txt")
        .cloned()
    {
        let toc_bytes = read_entry(&mut archive, &toc_name)?;
        let toc_text = String::from_utf8_lossy(&toc_bytes);
        for line in toc_text.lines().skip(1) {
            let mut parts = line
                .split('\t')
                .map(str::trim)
                .filter(|part| !part.is_empty());
            if let (Some(file), Some(title)) = (parts.next(), parts.next()) {
                toc.push((file.to_string(), title.to_string()));
            }
        }
    }

    let mut chapter_names: Vec<String> = names
        .iter()
        .filter(|name| is_html(name) && !base_name(name).starts_with("0_"))
        .cloned()
        .collect();
    if toc.is_empty() {
        chapter_names.sort_by_key(|name| numeric_prefix(base_name(name)));
    } else {
        let mut ordered = Vec::new();
        for (file, _title) in &toc {
            if let Some(found) = chapter_names
                .iter()
                .find(|name| base_name(name) == base_name(file))
                .cloned()
            {
                ordered.push(found);
            }
        }
        for name in &chapter_names {
            if !ordered.contains(name) {
                ordered.push(name.clone());
            }
        }
        chapter_names = ordered;
    }

    let mut chapters = Vec::new();
    for name in chapter_names {
        let bytes = read_entry(&mut archive, &name)?;
        let html = String::from_utf8_lossy(&bytes);
        let toc_title = toc
            .iter()
            .find(|(file, _title)| base_name(file) == base_name(&name))
            .map(|(_file, title)| title.clone());
        let title = extract_tag_text(&html, "h1")
            .or(toc_title)
            .or_else(|| extract_tag_text(&html, "title"))
            .unwrap_or_else(|| format!("第 {} 章", chapters.len() + 1));
        // 只取 <body>，避免 <title> 等头部文本混进正文。
        let mut paragraphs = html_to_paragraphs(body_fragment(&html));
        if paragraphs
            .first()
            .is_some_and(|paragraph| paragraph.trim() == title.trim())
        {
            paragraphs.remove(0);
        }
        if paragraphs.is_empty() {
            continue;
        }
        chapters.push(SourceChapter { title, paragraphs });
    }
    if chapters.is_empty() {
        return Err(ToolError::ZipStructure(
            "ZIP 中未找到 HTML 章节".to_string(),
        ));
    }
    Ok(SourceBook {
        title: stem(path),
        chapters,
        ..SourceBook::default()
    })
}

fn read_entry(archive: &mut ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>, ToolError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| ToolError::ZipStructure(format!("{name}: {error}")))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| ToolError::ZipStructure(format!("{name}: {error}")))?;
    Ok(bytes)
}

fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn is_html(name: &str) -> bool {
    let lower = base_name(name).to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or_default();
    ext == "html" || ext == "htm" || ext == "xhtml"
}

fn numeric_prefix(name: &str) -> u32 {
    let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(u32::MAX)
}

fn meta_content(html: &str, name: &str) -> Option<String> {
    static META_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<meta\b[^>]*>"));
    for cap in META_RE.captures_iter(html) {
        let tag = &cap[0];
        if attr(tag, "name").is_some_and(|value| value.eq_ignore_ascii_case(name)) {
            return attr(tag, "content")
                .map(|value| decode_entities(&value).trim().to_string())
                .filter(|value| !value.is_empty());
        }
    }
    None
}

pub(crate) fn attr(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = tag[from..].find(name) {
        let start = from + pos;
        let before_ok = start == 0
            || tag.as_bytes()[start - 1].is_ascii_whitespace()
            || tag.as_bytes()[start - 1] == b'<';
        if before_ok {
            let after = tag[start + name.len()..].trim_start();
            if let Some(rest) = after.strip_prefix('=') {
                let rest = rest.trim_start();
                let quote = rest.chars().next()?;
                if quote == '"' || quote == '\'' {
                    let inner = &rest[1..];
                    if let Some(end) = inner.find(quote) {
                        return Some(inner[..end].to_string());
                    }
                }
            }
        }
        from = start + name.len();
    }
    None
}

/// 取 `<body>` 内容；没有 body 标签时退回整篇。
pub(crate) fn body_fragment(html: &str) -> &str {
    BODY_RE
        .captures(html)
        .map_or(html, |cap| cap.get(1).map_or("", |m| m.as_str()))
}

/// 已知 tag 只编译一次；未收录的 tag 仍按同名模板即时编译，行为与旧实现一致。
fn tag_regex(tag: &str) -> Option<&'static Regex> {
    match tag {
        "h1" => Some(&*H1_RE),
        "h2" => Some(&*H2_RE),
        "h3" => Some(&*H3_RE),
        "title" => Some(&*TITLE_RE),
        _ => None,
    }
}

pub(crate) fn extract_tag_text(html: &str, tag: &str) -> Option<String> {
    if let Some(regex) = tag_regex(tag) {
        return capture_text(regex, html);
    }
    let pattern = format!(r"(?is)<{tag}\b[^>]*>(.*?)</{tag}>");
    let regex = Regex::new(&pattern).ok()?;
    capture_text(&regex, html)
}

/// 取正则第一个捕获组的纯文本；空串当作没取到。
pub(super) fn capture_text(regex: &Regex, html: &str) -> Option<String> {
    regex
        .captures(html)
        .map(|cap| strip_html(&cap[1]))
        .filter(|value| !value.is_empty())
}

pub(crate) fn strip_html(input: &str) -> String {
    let plain = TAG_RE.replace_all(input, " ");
    decode_entities(&plain)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn html_to_paragraphs(fragment: &str) -> Vec<String> {
    let mut html = SCRIPT_RE.replace_all(fragment, "").into_owned();
    html = STYLE_RE.replace_all(&html, "").into_owned();
    html = COMMENT_RE.replace_all(&html, "").into_owned();
    for regex in [&*H1_RE, &*H2_RE, &*H3_RE] {
        html = regex.replace_all(&html, "\n\n$1\n\n").into_owned();
    }
    html = BR_RE.replace_all(&html, "\n").into_owned();
    html = BLOCK_RE.replace_all(&html, "\n\n").into_owned();
    let text = TAG_RE.replace_all(&html, "").into_owned();
    let text = decode_entities(&text);
    text.split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        let Some(semi) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" | "emsp" | "ensp" => Some(' '),
            "hellip" => Some('…'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            _ => entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| {
                    entity
                        .strip_prefix('#')
                        .and_then(|dec| dec.parse::<u32>().ok())
                })
                .and_then(char::from_u32),
        };
        if let Some(ch) = decoded {
            out.push(ch);
        } else {
            out.push_str(&tail[..=semi]);
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    out
}

fn compile_regex(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|error| unreachable!("内置正则必须可编译: {error}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::io::Write;

    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    use super::super::{LoadOptions, load_path};

    #[test]
    fn loads_single_html_with_metadata_and_headings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("测试书.html");
        std::fs::write(
            &path,
            r#"<!DOCTYPE html><html><head><title>测试书</title><meta name="author" content="作者甲"/></head><body><script>alert(1)</script><h1>第一章</h1><p>第一段&amp;续</p><h1>第二章</h1><p>第二段</p></body></html>"#,
        )
        .unwrap();

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "测试书");
        assert_eq!(book.author, "作者甲");
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "第一章");
        assert_eq!(book.chapters[0].paragraphs, vec!["第一段&续"]);
        assert!(!book.chapters[0].paragraphs[0].contains("alert"));
    }

    #[test]
    fn loads_zip_with_toc_titles() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("导出书.zip");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("001_.html", options).unwrap();
        zip.write_all(
            "<html><head><title>第一章</title></head><body><h1>第一章</h1><p>甲&amp;乙</p></body></html>"
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("002_.html", options).unwrap();
        zip.write_all(
            "<html><head><title>第二章</title></head><body><h1>第二章</h1><p>丙</p></body></html>"
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("0_目录.txt", options).unwrap();
        zip.write_all(
            "文件名\t\t\t\t章节名\n001_.html\t\t第一章\n002_.html\t\t第二章\n".as_bytes(),
        )
        .unwrap();
        zip.finish().unwrap();

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "导出书");
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "第一章");
        assert_eq!(book.chapters[1].title, "第二章");
        assert_eq!(book.chapters[0].paragraphs, vec!["甲&乙"]);
    }

    #[test]
    fn extract_tag_text_static_table_and_fallback_agree() {
        let html = r#"<html><head><title>书名</title></head><body><h1 class="t">同名</h1><h2>第二章</h2><h3>第三节</h3><h4>第四节</h4></body></html>"#;

        // 静态表路径。
        assert_eq!(super::extract_tag_text(html, "h1").as_deref(), Some("同名"));
        assert_eq!(
            super::extract_tag_text(html, "h2").as_deref(),
            Some("第二章")
        );
        assert_eq!(
            super::extract_tag_text(html, "h3").as_deref(),
            Some("第三节")
        );
        assert_eq!(
            super::extract_tag_text(html, "title").as_deref(),
            Some("书名")
        );

        // 未收录 tag 走即时编译回退，结果必须与静态表完全一致。
        assert_eq!(
            super::extract_tag_text(html, "h4").as_deref(),
            Some("第四节")
        );
        assert_eq!(
            super::extract_tag_text(r#"<h1 class="t">同名</h1>"#, "h1"),
            super::extract_tag_text(r#"<x1 class="t">同名</x1>"#, "x1")
        );

        // 回退路径匹配不到时返回 None，不 panic。
        assert_eq!(super::extract_tag_text(html, "footer"), None);
        // 非法 tag 名导致正则编译失败时同样是 None。
        assert_eq!(super::extract_tag_text(html, "a("), None);
    }

    #[test]
    fn html_paragraphs_decode_entities_and_keep_line_breaks() {
        let paragraphs =
            super::html_to_paragraphs("<div>第一行<br/>第二行</div><div>&hellip;&#x4e2d;</div>");
        assert_eq!(paragraphs, vec!["第一行\n第二行", "…中"]);
    }
}
