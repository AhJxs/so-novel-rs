//! EPUB 导出。对应 Java `handle.EpubMergeHandler`。
//!
//! 行为：
//! - 输出文件 `<outDir>/<bookName>(<author>).epub`；
//! - metadata：title/author/description/language/publisher（与 Java 一致）；
//! - 章节：从 `chapters_dir` 按文件名升序读，文件名形如 `001_<标题>.html`，
//!   截"第一个 `_` 后"为 TOC 标题；
//! - 封面：阶段 3b 暂不下载（避免 export 层引入 reqwest 直接调用）。
//!   `book.cover_url` 字段在阶段 3c 调度层下载后通过 `merge_with_cover_bytes`
//!   传入；纯离线测试通过 `merge_with_cover_bytes(&[])` 跳过封面即可。
//!
//! 与 Java 端差异：
//! - Java 用 `epub4j-core`；Rust 用 `epub-builder`，两者 API 不同但 EPUB 输出
//!   结构一致（`mimetype` + `META-INF/container.xml` + OPF + NCX + 章节 XHTML）。
//! - Java 端封面下载失败会 `Console.error` 但仍继续；我们这里把"封面字节"
//!   作为输入参数，由调度层负责 soft-skip 决策。

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ReferenceType, ZipLibrary};

use super::exporter::pad_zero;
use crate::export::exporter::{ExportError, Exporter, sort_chapter_files, unique_path};
use crate::models::Book;
use crate::util::fs::sanitize_filename;

pub struct EpubExporter;

impl Exporter for EpubExporter {
    fn ext(&self) -> &'static str {
        "epub"
    }

    fn merge(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        merge_with_cover_bytes(book, chapters_dir, out_dir, None)
    }

    fn merge_with_cover(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
        cover_bytes: Option<&[u8]>,
    ) -> Result<PathBuf, ExportError> {
        merge_with_cover_bytes(book, chapters_dir, out_dir, cover_bytes)
    }
}

/// 实际合并实现：支持外部传入封面字节（mime/png/jpeg 推断）。
///
/// `cover_bytes` 为 None 时不写封面；为 Some 时按文件头判断 mime（仅 PNG / JPEG，
/// 其它降级为 `image/jpeg`）。这与 Java 端把 cover bytes 写到 `cover.jpg` 一致。
pub fn merge_with_cover_bytes(
    book: &Book,
    chapters_dir: &Path,
    out_dir: &Path,
    cover_bytes: Option<&[u8]>,
) -> Result<PathBuf, ExportError> {
    let files: Vec<PathBuf> = sort_chapter_files(chapters_dir)?
        .into_iter()
        .filter(|p| is_chapter_xhtml(p))
        .collect();
    if files.is_empty() {
        return Err(ExportError::EmptyChaptersDir(chapters_dir.to_path_buf()));
    }

    let zip = ZipLibrary::new().map_err(|e| ExportError::Epub(format!("ZipLibrary: {e}")))?;
    let mut builder =
        EpubBuilder::new(zip).map_err(|e| ExportError::Epub(format!("new builder: {e}")))?;

    // EPUB lang：优先用 book.language（如 "zh-CN"），回落到 "zh"。
    let epub_lang = if book.language.is_empty() {
        "zh".to_string()
    } else {
        // EPUB 规范接受 "zh-CN" 等 BCP 47 标签，直接使用。
        book.language.clone()
    };
    builder
        .epub_version(EpubVersion::V30)
        .metadata("title", &book.book_name)
        .map_err(|e| ExportError::Epub(format!("metadata title: {e}")))?
        .metadata("author", &book.author)
        .map_err(|e| ExportError::Epub(format!("metadata author: {e}")))?
        .metadata("lang", &epub_lang)
        .map_err(|e| ExportError::Epub(format!("metadata lang: {e}")))?
        .metadata("description", book.intro.as_deref().unwrap_or(""))
        .map_err(|e| ExportError::Epub(format!("metadata description: {e}")))?
        .metadata("generator", "so-novel-rs")
        .map_err(|e| ExportError::Epub(format!("metadata generator: {e}")))?;

    // 封面
    if let Some(bytes) = cover_bytes.filter(|b| !b.is_empty()) {
        let mime = detect_image_mime(bytes);
        let ext = match mime {
            "image/png" => "png",
            _ => "jpg",
        };
        let cover_name = format!("cover.{ext}");
        if let Err(e) = builder.add_cover_image(&cover_name, bytes, mime) {
            tracing::warn!("EPUB 封面写入失败，跳过封面: {e}");
        } else {
            // 添加封面 XHTML 页（Apple Books 兼容）
            let cover_xhtml = build_cover_xhtml(&cover_name, &book.book_name, &epub_lang);
            if let Err(e) = builder.add_content(
                EpubContent::new("cover.xhtml", cover_xhtml.as_bytes())
                    .title("封面")
                    .reftype(ReferenceType::Cover),
            ) {
                tracing::warn!("EPUB 封面页写入失败: {e}");
            }
        }
    }

    // 章节
    let total_chapters = files.len();
    let digit = total_chapters.to_string().len().max(3);
    let started = std::time::Instant::now();
    for (idx, path) in files.iter().enumerate() {
        let mut buf = Vec::new();
        File::open(path)?.read_to_end(&mut buf)?;
        let title =
            chapter_title_from_filename(path).unwrap_or_else(|| format!("第 {} 章", idx + 1));
        let id = pad_zero((idx + 1) as u32, digit);
        let entry_name = format!("{id}.xhtml");
        builder
            .add_content(
                EpubContent::new(&entry_name, &buf[..])
                    .title(title)
                    .reftype(ReferenceType::Text),
            )
            .map_err(|e| ExportError::Epub(format!("add chapter {entry_name}: {e}")))?;
    }
    tracing::info!(
        chapters = total_chapters,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "EPUB 章节合并完成"
    );

    // 落盘
    std::fs::create_dir_all(out_dir)?;
    let out_name = sanitize_filename(&format!("{}({}).epub", book.book_name, book.author));
    let out_path = unique_path(out_dir, &out_name);
    let file = File::create(&out_path)?;
    let mut writer = BufWriter::new(file);
    builder
        .generate(&mut writer)
        .map_err(|e| ExportError::Epub(format!("generate: {e}")))?;
    writer.flush()?;

    Ok(out_path)
}

fn is_chapter_xhtml(p: &Path) -> bool {
    let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let is_html = ext.eq_ignore_ascii_case("html") || ext.eq_ignore_ascii_case("xhtml");
    if !is_html {
        return false;
    }
    // 跳过 0_ 开头的辅助文件（封面、目录索引）
    let name = p.file_name().and_then(|s| s.to_str()).unwrap_or_default();
    !name.starts_with("0_")
}

/// `001_第1章 楔子.html` → `第1章 楔子`。无 `_` 时回退到 stem。
fn chapter_title_from_filename(p: &Path) -> Option<String> {
    let stem = p.file_stem().and_then(|s| s.to_str())?;
    if let Some(idx) = stem.find('_') {
        let title = &stem[idx + 1..];
        if !title.is_empty() {
            return Some(title.to_string());
        }
    }
    Some(stem.to_string())
}

fn detect_image_mime(bytes: &[u8]) -> &'static str {
    // 简单 magic number 检测，覆盖常见图片格式。
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        // 兜底：最常见的是 JPEG，但无法确定时降级为通用二进制
        "image/jpeg"
    }
}

fn build_cover_xhtml(cover_filename: &str, book_name: &str, lang: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" ?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="{lang}">
<head>
  <title>{book_name}</title>
  <style type="text/css">body{{margin:0;padding:0;text-align:center;}} img{{max-width:100%;height:auto;}}</style>
</head>
<body>
  <div><img src="{cover_filename}" alt="cover"/></div>
</body>
</html>
"#,
        lang = html_escape(lang),
        book_name = html_escape(book_name),
        cover_filename = cover_filename
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
            intro: Some("简介内容".into()),
            ..Book::default()
        }
    }

    fn sample_chapters() -> Vec<RenderedChapter> {
        let xhtml1 = r##"<?xml version="1.0" encoding="UTF-8" ?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>第1章 楔子</title></head>
<body><h2>第1章 楔子</h2><p>正文一</p></body></html>"##;
        let xhtml2 = r##"<?xml version="1.0" encoding="UTF-8" ?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>第2章 启程</title></head>
<body><h2>第2章 启程</h2><p>正文二</p></body></html>"##;
        vec![
            RenderedChapter {
                order: 1,
                title: "第1章 楔子".into(),
                body: xhtml1.into(),
            },
            RenderedChapter {
                order: 2,
                title: "第2章 启程".into(),
                body: xhtml2.into(),
            },
        ]
    }

    #[test]
    fn produces_epub_file_without_cover() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Epub).unwrap();

        let path = EpubExporter.merge(&sample_book(), &chapters, &out).unwrap();
        assert!(path.exists(), "missing epub: {}", path.display());
        assert!(
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap()
                .ends_with(".epub")
        );

        // EPUB 是 ZIP；用 zip crate 打开校验关键文件
        let f = File::open(&path).unwrap();
        let mut zr = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..zr.len())
            .map(|i| zr.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "mimetype"), "names={names:?}");
        assert!(names.iter().any(|n| n == "META-INF/container.xml"));
        // OPF / NCX 路径在 epub-builder 默认布局下放在 OEBPS 子目录
        assert!(
            names.iter().any(|n| n.ends_with(".opf")),
            "no opf in {names:?}"
        );
    }

    #[test]
    fn produces_epub_with_cover_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Epub).unwrap();

        // 最小 PNG 头 + IEND（构造一个合法的 1x1 PNG 字节序列）
        // 直接用 PNG signature + 一段任意字节即可让 epub-builder 接受写入。
        let mut png = vec![0x89u8, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        png.extend_from_slice(&[0u8; 32]);

        let path = merge_with_cover_bytes(&sample_book(), &chapters, &out, Some(&png)).unwrap();
        let f = File::open(&path).unwrap();
        let mut zr = zip::ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..zr.len())
            .map(|i| zr.by_index(i).unwrap().name().to_string())
            .collect();
        // 封面相关文件应存在
        assert!(
            names.iter().any(|n| n.ends_with("cover.png")),
            "no cover.png in {names:?}"
        );
        assert!(
            names.iter().any(|n| n.ends_with("cover.xhtml")),
            "no cover.xhtml in {names:?}"
        );
    }

    #[test]
    fn empty_chapters_dir_returns_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let err = EpubExporter
            .merge(&sample_book(), &chapters, &out)
            .unwrap_err();
        assert!(matches!(err, ExportError::EmptyChaptersDir(_)));
    }

    #[test]
    fn detect_image_mime_for_png_and_jpeg() {
        let png = [0x89u8, b'P', b'N', b'G', 0, 0, 0, 0];
        assert_eq!(detect_image_mime(&png), "image/png");
        let jpeg = [0xFFu8, 0xD8, 0xFF, 0xE0];
        assert_eq!(detect_image_mime(&jpeg), "image/jpeg");
        let unknown = [0u8, 0, 0, 0];
        assert_eq!(detect_image_mime(&unknown), "image/jpeg");
    }

    #[test]
    fn detect_image_mime_for_gif_webp_bmp() {
        let gif = [b'G', b'I', b'F', b'8', b'9', b'a', 0, 0];
        assert_eq!(detect_image_mime(&gif), "image/gif");
        let gif87 = [b'G', b'I', b'F', b'8', b'7', b'a', 0, 0];
        assert_eq!(detect_image_mime(&gif87), "image/gif");

        let mut webp = [0u8; 12];
        webp[..4].copy_from_slice(b"RIFF");
        webp[8..12].copy_from_slice(b"WEBP");
        assert_eq!(detect_image_mime(&webp), "image/webp");

        let bmp = [b'B', b'M', 0, 0, 0, 0, 0, 0];
        assert_eq!(detect_image_mime(&bmp), "image/bmp");
    }

    #[test]
    fn detect_image_mime_short_input_does_not_panic() {
        assert_eq!(detect_image_mime(&[]), "image/jpeg");
        assert_eq!(detect_image_mime(&[0x89]), "image/jpeg");
        assert_eq!(detect_image_mime(&[0xFF, 0xD8]), "image/jpeg");
        // RIFF 但不够 12 字节 → 降级
        assert_eq!(detect_image_mime(b"RIFF"), "image/jpeg");
    }

    #[test]
    fn chapter_title_from_filename_extracts_after_first_underscore() {
        let p = std::path::PathBuf::from("001_第1章 楔子.html");
        assert_eq!(
            chapter_title_from_filename(&p).as_deref(),
            Some("第1章 楔子")
        );
        let p2 = std::path::PathBuf::from("002_.html");
        // 下划线后为空 → 回退到 stem
        assert_eq!(chapter_title_from_filename(&p2).as_deref(), Some("002_"));
    }
}
