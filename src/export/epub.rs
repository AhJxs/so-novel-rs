//! EPUB 导出。对应 Java `handle.EpubMergeHandler`。
//!
//! 输出 `<outDir>/<bookName>(<author>).epub`: metadata 写 title/author/description/language
//! (与 Java 一致); 章节正文由调用方渲染好后按内存传入, TOC 标题取 `RenderedChapter.title`。
//!
//! 封面字节由调度层下载后经 `merge_with_cover_bytes` 传入 (export 层不依赖 reqwest);
//! 未传或写入失败时 soft-skip 封面、继续产出。

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ReferenceType, ZipLibrary};

use super::exporter::pad_zero;
use crate::export::exporter::{ExportError, Exporter, RenderedChapter, unique_path};
use crate::models::BookDetail;
use crate::utils::fs;

/// 把渲染好的章节打包成 EPUB。
pub struct EpubExporter;

impl Exporter for EpubExporter {
    fn ext(&self) -> &'static str {
        "epub"
    }

    fn merge_with_progress(
        &self,
        book: &BookDetail,
        chapters: &[RenderedChapter],
        out_dir: &Path,
        cover_bytes: Option<&[u8]>,
        on_chapter: &mut dyn FnMut(usize),
    ) -> Result<PathBuf, ExportError> {
        merge_epub(book, chapters, out_dir, cover_bytes, on_chapter)
    }
}

/// 实际合并实现：支持外部传入封面字节。
///
/// `cover_bytes` 为 None 时不写封面；Some 时按 magic number 判 mime (PNG/JPEG/GIF/WEBP/BMP,
/// 未知降级 `image/jpeg`), 与 Java 端把 cover bytes 写到 `cover.jpg` 一致。
///
/// # Errors
///
/// 章节为空、epub 打包写入失败时返回 [`ExportError`]。
pub fn merge_with_cover_bytes(
    book: &BookDetail,
    chapters: &[RenderedChapter],
    out_dir: &Path,
    cover_bytes: Option<&[u8]>,
) -> Result<PathBuf, ExportError> {
    merge_epub(book, chapters, out_dir, cover_bytes, &mut |_| {})
}

/// 合并并逐章回调进度（epub-builder 在 `add_content` 时就把每章压进 zip 缓冲，
/// 因此逐章回调基本等价于真实的写盘进度）。
fn merge_epub(
    book: &BookDetail,
    chapters: &[RenderedChapter],
    out_dir: &Path,
    cover_bytes: Option<&[u8]>,
    on_chapter: &mut dyn FnMut(usize),
) -> Result<PathBuf, ExportError> {
    if chapters.is_empty() {
        return Err(ExportError::NoChapters);
    }

    let zip = ZipLibrary::new().map_err(|e| ExportError::Epub(format!("ZipLibrary: {e}")))?;
    let mut builder =
        EpubBuilder::new(zip).map_err(|e| ExportError::Epub(format!("new builder: {e}")))?;

    // EPUB lang：优先用 book.language（如 "zh-CN"），回落到 "zh"。
    let epub_lang = if book.language.is_empty() {
        "zh".to_string()
    } else {
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

    let total_chapters = chapters.len();
    let max_order = chapters
        .iter()
        .map(|c| c.order)
        .max()
        .unwrap_or(total_chapters as u32);
    let digit = max_order.to_string().len().max(3);
    let started = Instant::now();
    for (idx, chapter) in chapters.iter().enumerate() {
        let id = pad_zero(chapter.order, digit);
        let entry_name = format!("{id}.xhtml");
        builder
            .add_content(
                EpubContent::new(&entry_name, chapter.body.as_bytes())
                    .title(&chapter.title)
                    .reftype(ReferenceType::Text),
            )
            .map_err(|e| ExportError::Epub(format!("add chapter {entry_name}: {e}")))?;
        on_chapter(idx + 1);
    }
    tracing::info!(
        chapters = total_chapters,
        elapsed_ms = started.elapsed().as_millis() as u64,
        "EPUB 章节合并完成"
    );

    std::fs::create_dir_all(out_dir)?;
    let out_name = fs::sanitize_filename(&format!("{}({}).epub", book.book_name, book.author));
    let out_path = unique_path(out_dir, &out_name);
    let file = File::create(&out_path)?;
    let mut writer = BufWriter::new(file);
    builder
        .generate(&mut writer)
        .map_err(|e| ExportError::Epub(format!("generate: {e}")))?;
    writer.flush()?;

    Ok(out_path)
}

fn detect_image_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
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
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use zip::ZipArchive;

    use super::*;
    use crate::config::ExportFormat;
    use crate::export::exporter::{RenderedChapter, write_chapter_files};

    fn sample_book() -> BookDetail {
        BookDetail {
            url: "https://x/".into(),
            book_name: "起航".into(),
            author: "苹果".into(),
            intro: Some("简介内容".into()),
            ..BookDetail::default()
        }
    }

    fn sample_chapters() -> Vec<RenderedChapter> {
        let xhtml1 = r#"<?xml version="1.0" encoding="UTF-8" ?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>第1章 楔子</title></head>
<body><h2>第1章 楔子</h2><p>正文一</p></body></html>"#;
        let xhtml2 = r#"<?xml version="1.0" encoding="UTF-8" ?>
<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>第2章 启程</title></head>
<body><h2>第2章 启程</h2><p>正文二</p></body></html>"#;
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

    fn load_and_merge(book: &BookDetail, dir: &Path, out: &Path) -> PathBuf {
        let chapters = crate::export::load_chapter_files(dir, ExportFormat::Epub).unwrap();
        EpubExporter.merge(book, &chapters, out).unwrap()
    }

    fn load_and_merge_cover(book: &BookDetail, dir: &Path, out: &Path, cover: &[u8]) -> PathBuf {
        let chapters = crate::export::load_chapter_files(dir, ExportFormat::Epub).unwrap();
        merge_with_cover_bytes(book, &chapters, out, Some(cover)).unwrap()
    }

    #[test]
    fn merge_reports_every_chapter_to_progress_callback() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let chapters = sample_chapters();
        let mut seen = Vec::new();

        EpubExporter
            .merge_with_progress(&sample_book(), &chapters, &out, None, &mut |done| {
                seen.push(done);
            })
            .unwrap();

        assert_eq!(seen, vec![1, 2]);
    }

    #[test]
    fn produces_epub_file_without_cover() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Epub).unwrap();

        let path = load_and_merge(&sample_book(), &chapters, &out);
        assert!(path.exists(), "missing epub: {}", path.display());
        assert!(
            std::path::Path::new(path.file_name().and_then(|s| s.to_str()).unwrap())
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
        );

        let f = File::open(&path).unwrap();
        let mut zr = ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..zr.len())
            .map(|i| zr.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "mimetype"), "names={names:?}");
        assert!(names.iter().any(|n| n == "META-INF/container.xml"));
        // OPF / NCX 路径在 epub-builder 默认布局下放在 OEBPS 子目录
        assert!(
            names.iter().any(|n| std::path::Path::new(n)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("opf"))),
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

        // 用 PNG signature + 任意字节构造输入 (epub-builder 只校验文件头)。
        let mut png = vec![0x89u8, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend_from_slice(&[0u8; 32]);

        let path = load_and_merge_cover(&sample_book(), &chapters, &out, &png);
        let f = File::open(&path).unwrap();
        let mut zr = ZipArchive::new(f).unwrap();
        let names: Vec<String> = (0..zr.len())
            .map(|i| zr.by_index(i).unwrap().name().unwrap().to_string())
            .collect();
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
        let out = dir.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let err = EpubExporter.merge(&sample_book(), &[], &out).unwrap_err();
        assert!(matches!(err, ExportError::NoChapters));
    }

    #[test]
    fn detect_image_mime_for_png_and_jpeg() {
        let png = [0x89u8, b'P', b'N', b'G', 0, 0, 0, 0];
        assert_eq!(detect_image_mime(&png), "image/png");
        let jpeg = [0xffu8, 0xd8, 0xff, 0xe0];
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
        assert_eq!(detect_image_mime(&[0xff, 0xd8]), "image/jpeg");
        // RIFF 但不够 12 字节 → 降级
        assert_eq!(detect_image_mime(b"RIFF"), "image/jpeg");
    }
}
