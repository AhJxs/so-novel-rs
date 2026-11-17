//! EPUB 加载：container.xml → OPF metadata / manifest / spine，按 spine 顺序读正文。

use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use zip::ZipArchive;

use super::html::{attr, body_fragment, extract_tag_text, html_to_paragraphs, strip_html};
use super::stem;
use crate::tools::ToolError;
use crate::tools::source_book::{SourceBook, SourceChapter};

static ITEM_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<item\b[^>]*>"));
static ITEMREF_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<itemref\b[^>]*>"));
static ROOTFILE_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<rootfile\b[^>]*>"));
static META_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<meta\b[^>]*>"));

#[derive(Debug)]
struct ManifestItem {
    id: String,
    href: String,
    media_type: String,
    properties: String,
}

pub(super) fn load(path: &Path) -> Result<SourceBook, ToolError> {
    let file = std::fs::File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| ToolError::EpubStructure(error.to_string()))?;

    if archive.by_name("META-INF/encryption.xml").is_ok() {
        return Err(ToolError::EpubStructure(
            "不支持 DRM 加密的 EPUB".to_string(),
        ));
    }

    let container =
        String::from_utf8_lossy(&read_entry(&mut archive, "META-INF/container.xml")?).into_owned();
    let opf_path = ROOTFILE_RE
        .captures(&container)
        .and_then(|cap| attr(&cap[0], "full-path"))
        .ok_or_else(|| ToolError::EpubStructure("container.xml 缺少 rootfile".to_string()))?;
    let opf = String::from_utf8_lossy(&read_entry(&mut archive, &opf_path)?).into_owned();
    let base = opf_path
        .rsplit_once('/')
        .map(|(dir, _file)| dir.to_string())
        .unwrap_or_default();

    let items = manifest_items(&opf);
    let spine = spine_ids(&opf);
    let cover_id = cover_manifest_id(&opf, &items);

    let mut chapters = Vec::new();
    for idref in spine {
        let Some(item) = items.iter().find(|item| item.id == idref) else {
            continue;
        };
        if !item.media_type.contains("html") {
            continue;
        }
        let name = resolve_path(&base, &item.href);
        let html = String::from_utf8_lossy(&read_entry(&mut archive, &name)?).into_owned();
        let title = extract_tag_text(&html, "h1")
            .or_else(|| extract_tag_text(&html, "h2"))
            .or_else(|| extract_tag_text(&html, "title"))
            .unwrap_or_else(|| format!("第 {} 章", chapters.len() + 1));
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
        return Err(ToolError::EpubStructure(
            "EPUB 中没有可读取的正文".to_string(),
        ));
    }

    let cover = cover_id
        .and_then(|id| items.iter().find(|item| item.id == id))
        .and_then(|item| {
            let name = resolve_path(&base, &item.href);
            let bytes = read_entry(&mut archive, &name).ok()?;
            (!bytes.is_empty()).then_some(bytes)
        });

    Ok(SourceBook {
        title: dc_text(&opf, "title").unwrap_or_else(|| stem(path)),
        author: dc_text(&opf, "creator").unwrap_or_default(),
        intro: dc_text(&opf, "description"),
        language: dc_text(&opf, "language").unwrap_or_else(|| "zh".to_string()),
        cover,
        chapters,
    })
}

fn manifest_items(opf: &str) -> Vec<ManifestItem> {
    ITEM_RE
        .captures_iter(opf)
        .filter_map(|cap| {
            Some(ManifestItem {
                id: attr(&cap[0], "id")?,
                href: attr(&cap[0], "href")?,
                media_type: attr(&cap[0], "media-type").unwrap_or_default(),
                properties: attr(&cap[0], "properties").unwrap_or_default(),
            })
        })
        .collect()
}

fn spine_ids(opf: &str) -> Vec<String> {
    ITEMREF_RE
        .captures_iter(opf)
        .filter_map(|cap| attr(&cap[0], "idref"))
        .collect()
}

fn cover_manifest_id(opf: &str, items: &[ManifestItem]) -> Option<String> {
    if let Some(item) = items.iter().find(|item| {
        item.properties
            .split_whitespace()
            .any(|value| value == "cover-image")
    }) {
        return Some(item.id.clone());
    }
    META_RE.captures_iter(opf).find_map(|cap| {
        let tag = &cap[0];
        (attr(tag, "name").is_some_and(|value| value.eq_ignore_ascii_case("cover")))
            .then(|| attr(tag, "content"))
            .flatten()
    })
}

fn dc_text(opf: &str, tag: &str) -> Option<String> {
    let pattern = format!(r"(?is)<dc:{tag}\b[^>]*>(.*?)</dc:{tag}>");
    let regex = Regex::new(&pattern).ok()?;
    regex
        .captures(opf)
        .map(|cap| strip_html(&cap[1]))
        .filter(|value| !value.is_empty())
}

fn read_entry(archive: &mut ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>, ToolError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| ToolError::EpubStructure(format!("{name}: {error}")))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| ToolError::EpubStructure(format!("{name}: {error}")))?;
    Ok(bytes)
}

fn resolve_path(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let href = percent_decode(href);
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2]))
        {
            out.push(high * 16 + low);
            index += 3;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn compile_regex(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|error| unreachable!("内置正则必须可编译: {error}"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::Path;

    use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

    use super::super::{LoadOptions, load_path};

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];

    fn write_epub(path: &Path) {
        let zip = ZipLibrary::new().unwrap();
        let mut builder = EpubBuilder::new(zip).unwrap();
        builder.metadata("title", "测试书").unwrap();
        builder.metadata("author", "作者甲").unwrap();
        builder.metadata("lang", "zh-CN").unwrap();
        builder.metadata("description", "一本书").unwrap();
        builder
            .add_cover_image("cover.png", PNG, "image/png")
            .unwrap();
        builder
            .add_content(
                EpubContent::new(
                    "c1.xhtml",
                    "<html><head><title>第一章</title></head><body><h1>第一章</h1><p>甲</p></body></html>"
                        .as_bytes(),
                )
                .title("第一章"),
            )
            .unwrap();
        builder
            .add_content(
                EpubContent::new(
                    "c2.xhtml",
                    "<html><head><title>第二章</title></head><body><h1>第二章</h1><p>乙</p></body></html>"
                        .as_bytes(),
                )
                .title("第二章"),
            )
            .unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        builder.generate(&mut file).unwrap();
    }

    #[test]
    fn loads_metadata_spine_and_cover() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("书.epub");
        write_epub(&path);

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "测试书");
        assert_eq!(book.author, "作者甲");
        assert_eq!(book.language, "zh-CN");
        assert_eq!(book.intro.as_deref(), Some("一本书"));
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "第一章");
        assert_eq!(book.chapters[0].paragraphs, vec!["甲"]);
        assert_eq!(book.chapters[1].paragraphs, vec!["乙"]);
        assert!(book.cover.as_ref().is_some_and(|bytes| !bytes.is_empty()));
    }

    #[test]
    fn missing_container_returns_epub_structure_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("坏.epub");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("mimetype", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.finish().unwrap();

        let result = load_path(&path, &LoadOptions::default());
        assert!(matches!(
            result,
            Err(crate::tools::ToolError::EpubStructure(_))
        ));
    }
}
