//! 阅读内容加载：TXT / EPUB → 章节化纯文本。首版只放行这两种格式。

use std::path::Path;

use crate::library::BookFormat;
use crate::tools::ToolError;
use crate::tools::load::{LoadOptions, load_path};

/// 阅读器内的整本书。
#[derive(Debug, Clone)]
pub struct ReaderBook {
    pub title: String,
    pub author: String,
    pub chapters: Vec<ReaderChapter>,
    /// TXT 实际使用的编码名；EPUB 为 `None`。
    pub encoding: Option<String>,
}

/// 单章正文。
#[derive(Debug, Clone)]
pub struct ReaderChapter {
    pub title: String,
    pub paragraphs: Vec<String>,
}

/// 打开书籍时的错误。
#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    /// 格式不在首版可读白名单内。
    #[error("暂不支持在应用内阅读该格式")]
    UnsupportedFormat(BookFormat),
    /// 文件已被移动或删除。
    #[error("文件已被移动或删除")]
    MissingFile,
    /// 读盘失败。
    #[error("读取文件失败: {0}")]
    Io(#[from] std::io::Error),
    /// 解码失败。
    #[error("无法解码 {} 文件: {}", .0.label(), .1)]
    Decode(BookFormat, String),
    /// EPUB 结构 / DRM 问题。
    #[error("EPUB 文件无法打开: {0}")]
    Epub(String),
    /// 文件没有正文。
    #[error("文件中没有可阅读的正文")]
    NoContent,
    /// 用户指定的编码标签无法识别。
    #[error("未知编码: {0}")]
    InvalidEncoding(String),
}

impl ReaderError {
    /// 对应的 i18n key（`Reader.error.*`）。
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::UnsupportedFormat(_) => "Reader.error.unsupported",
            Self::MissingFile => "Reader.error.missing",
            Self::Io(_) => "Reader.error.io",
            Self::Decode(_, _) => "Reader.error.decode",
            Self::Epub(_) => "Reader.error.epub",
            Self::NoContent => "Reader.error.no_content",
            Self::InvalidEncoding(_) => "Reader.error.encoding",
        }
    }

    /// 技术细节（文件名 / 结构错误），UI 用弱化小字展示。
    #[must_use]
    pub fn detail(&self) -> String {
        self.to_string()
    }
}

/// 打开一本书并章节化。
///
/// # Errors
///
/// 格式不可读、文件缺失、编码标签非法、解码 / EPUB 结构失败、无正文时返回 [`ReaderError`]。
#[tracing::instrument(name = "reader::load_book", skip_all, fields(path = %path.display()))]
pub fn load_book(
    path: &Path,
    format: BookFormat,
    encoding: Option<&str>,
) -> Result<ReaderBook, ReaderError> {
    if !format.is_readable() {
        return Err(ReaderError::UnsupportedFormat(format));
    }
    if !path.is_file() {
        return Err(ReaderError::MissingFile);
    }
    let explicit = crate::tools::encoding::parse_encoding(encoding).map_err(|_| {
        ReaderError::InvalidEncoding(encoding.unwrap_or_default().to_string())
    })?;
    let detected = if format == BookFormat::Txt {
        crate::tools::encoding::detect_file_encoding(path)
            .ok()
            .map(|value| value.name().to_string())
    } else {
        None
    };
    let source = load_path(
        path,
        &LoadOptions {
            source_encoding: explicit,
            chapter_regex: None,
        },
    )
    .map_err(|error| map_error(format, error))?;

    let chapters: Vec<ReaderChapter> = source
        .chapters
        .into_iter()
        .map(|chapter| ReaderChapter {
            title: chapter.title,
            paragraphs: chapter.paragraphs,
        })
        .collect();
    if chapters
        .iter()
        .all(|chapter| chapter.paragraphs.iter().all(|line| line.trim().is_empty()))
    {
        return Err(ReaderError::NoContent);
    }
    Ok(ReaderBook {
        title: source.title,
        author: source.author,
        chapters,
        encoding: explicit.map(|value| value.name().to_string()).or(detected),
    })
}

fn map_error(format: BookFormat, error: ToolError) -> ReaderError {
    match error {
        ToolError::Io(error) => ReaderError::Io(error),
        ToolError::Decode(message) => ReaderError::Decode(format, message),
        ToolError::EpubStructure(message) => ReaderError::Epub(message),
        ToolError::NoContent => ReaderError::NoContent,
        ToolError::UnsupportedFormat(_) => ReaderError::UnsupportedFormat(format),
        other => ReaderError::Decode(format, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    // 每行之间都要有空行：标题行必须独占一个段落才会被识别成章节，
    // 否则「第一章 开始\n正文一」会被当成一段普通正文。
    const TXT: &str = "第一章 开始\n\n正文一\n\n第二章 继续\n\n正文二\n";

    #[test]
    fn loads_utf8_txt_with_chapters() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("书.txt");
        std::fs::write(&path, TXT).unwrap();

        let book = load_book(&path, BookFormat::Txt, None).unwrap();
        assert_eq!(book.title, "书");
        assert_eq!(book.chapters.len(), 2);
        assert!(book.chapters[0].title.contains("第一章"));
        assert_eq!(book.chapters[0].paragraphs, vec!["正文一"]);
        assert_eq!(book.encoding.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn decodes_gbk_txt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gbk.txt");
        let (bytes, _, _) = encoding_rs::GBK.encode(TXT);
        std::fs::write(&path, &bytes).unwrap();

        let book = load_book(&path, BookFormat::Txt, None).unwrap();
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].paragraphs, vec!["正文一"]);
        assert!(book.encoding.is_some());
    }

    #[test]
    fn explicit_encoding_overrides_detection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gbk.txt");
        let (bytes, _, _) = encoding_rs::GBK.encode(TXT);
        std::fs::write(&path, &bytes).unwrap();

        let book = load_book(&path, BookFormat::Txt, Some("gbk")).unwrap();
        assert_eq!(book.chapters[0].paragraphs, vec!["正文一"]);
        assert_eq!(book.encoding.as_deref(), Some("GBK"));
    }

    #[test]
    fn rejects_unknown_encoding_label() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, TXT).unwrap();

        assert!(matches!(
            load_book(&path, BookFormat::Txt, Some("klingon")),
            Err(ReaderError::InvalidEncoding(_))
        ));
    }

    #[test]
    fn rejects_unsupported_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("a.pdf");
        std::fs::write(&pdf, b"%PDF-1.7").unwrap();
        assert!(matches!(
            load_book(&pdf, BookFormat::Pdf, None),
            Err(ReaderError::UnsupportedFormat(BookFormat::Pdf))
        ));
        assert!(matches!(
            load_book(&dir.path().join("nope.txt"), BookFormat::Txt, None),
            Err(ReaderError::MissingFile)
        ));
    }

    #[test]
    fn empty_txt_is_no_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, "").unwrap();
        assert!(matches!(
            load_book(&path, BookFormat::Txt, None),
            Err(ReaderError::NoContent)
        ));
    }
}
