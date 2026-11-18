//! 元数据提取：EPUB 读 OPF，TXT 探测编码；其他格式只从文件名推断标题。
//!
//! 这里只做「没有副作用」的提取：封面字节原样返回，写盘由后台工作线程负责，
//! 这样 `drain` 回到 UI 线程时不需要碰磁盘。

use std::path::Path;

use super::book::BookFormat;

/// 提取结果；封面是原始字节。
#[derive(Debug, Clone, Default)]
pub struct ExtractedMeta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub cover: Option<Vec<u8>>,
    pub encoding: Option<String>,
}

/// 按格式提取元数据。任何失败都降级成「文件名做标题」，不返回错误 ——
/// 一本元数据坏掉的书仍然应该出现在书架上。
#[must_use]
pub fn extract(path: &Path, format: BookFormat) -> ExtractedMeta {
    match format {
        BookFormat::Epub => extract_epub(path),
        BookFormat::Txt => ExtractedMeta {
            title: file_stem(path),
            encoding: crate::tools::encoding::detect_file_encoding(path)
                .ok()
                .map(|encoding| encoding.name().to_string()),
            ..ExtractedMeta::default()
        },
        _ => ExtractedMeta {
            title: file_stem(path),
            ..ExtractedMeta::default()
        },
    }
}

fn extract_epub(path: &Path) -> ExtractedMeta {
    match crate::tools::load::epub::read_metadata(path) {
        Ok(meta) => ExtractedMeta {
            title: meta.title,
            author: meta.author,
            intro: meta.intro,
            cover: meta.cover,
            encoding: None,
        },
        Err(error) => {
            tracing::warn!("读取 EPUB 元数据失败 {}: {error}", path.display());
            ExtractedMeta {
                title: file_stem(path),
                ..ExtractedMeta::default()
            }
        }
    }
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::Path;

    use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];

    fn write_epub(path: &Path) {
        let zip = ZipLibrary::new().unwrap();
        let mut builder = EpubBuilder::new(zip).unwrap();
        builder.metadata("title", "元数据书名").unwrap();
        builder.metadata("author", "作者乙").unwrap();
        builder
            .add_cover_image("cover.png", PNG, "image/png")
            .unwrap();
        builder
            .add_content(
                EpubContent::new(
                    "c1.xhtml",
                    "<html><body><p>正文</p></body></html>".as_bytes(),
                )
                .title("第一章"),
            )
            .unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        builder.generate(&mut file).unwrap();
    }

    #[test]
    fn extracts_epub_metadata_and_cover() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("book.epub");
        write_epub(&path);

        let meta = extract(&path, BookFormat::Epub);
        assert_eq!(meta.title.as_deref(), Some("元数据书名"));
        assert_eq!(meta.author.as_deref(), Some("作者乙"));
        assert!(meta.cover.is_some_and(|bytes| !bytes.is_empty()));
        assert!(meta.encoding.is_none());
    }

    #[test]
    fn broken_epub_falls_back_to_file_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("坏书.epub");
        std::fs::write(&path, b"not a zip").unwrap();

        let meta = extract(&path, BookFormat::Epub);
        assert_eq!(meta.title.as_deref(), Some("坏书"));
        assert!(meta.author.is_none());
        assert!(meta.cover.is_none());
    }

    #[test]
    fn txt_reports_detected_encoding() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("小说.txt");
        std::fs::write(&path, "第一章 开始\n正文内容").unwrap();

        let meta = extract(&path, BookFormat::Txt);
        assert_eq!(meta.title.as_deref(), Some("小说"));
        assert_eq!(meta.encoding.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn other_formats_only_use_file_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("文档.pdf");
        std::fs::write(&path, b"%PDF-1.7").unwrap();

        let meta = extract(&path, BookFormat::Pdf);
        assert_eq!(meta.title.as_deref(), Some("文档"));
        assert!(meta.cover.is_none());
        assert!(meta.encoding.is_none());
    }
}
