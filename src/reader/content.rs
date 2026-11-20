//! 阅读内容加载：TXT / EPUB → 章节化纯文本。首版只放行这两种格式。

use std::path::Path;

use rust_i18n::t;
use serde::{Deserialize, Serialize};

use crate::library::BookFormat;
use crate::tools::ToolError;
use crate::tools::load::{LoadOptions, load_path};

/// 阅读器内的整本书。
///
/// `Serialize` / `Deserialize` 供 [`super::cache`] 落盘复用，不参与其它协议。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaderBook {
    pub title: String,
    pub author: String,
    pub chapters: Vec<ReaderChapter>,
    /// TXT 实际解码用的编码名（BOM 优先于显式标签）。EPUB 不走编码标签，恒为 `None`。
    pub encoding: Option<String>,
}

/// 单章正文。
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// 技术细节（系统错误原文 / 无法识别的编码标签），UI 用弱化小字展示。
    ///
    /// 没有可展示的细节时返回 `None`：`MissingFile` 之类的中文 `Display` 是给日志看的，
    /// 标题已经由 `message_key()` 翻译过，再印一遍既重复又绕过 `t!`。
    #[must_use]
    pub fn detail(&self) -> Option<String> {
        match self {
            Self::MissingFile | Self::UnsupportedFormat(_) | Self::NoContent | Self::Epub(_) => {
                None
            }
            Self::Io(error) => Some(error.to_string()),
            Self::Decode(_, message) | Self::InvalidEncoding(message) => Some(message.clone()),
        }
    }
}

/// 读一本书。TXT 会做一次「疑似乱码」检查（有损解码不会失败，只能反推）。
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
    load_book_impl(path, format, encoding, true)
}

/// 跳过「疑似乱码」检查的加载：错误面板上「仍要打开」用它。
///
/// 密度判断是启发式，猜错时不能把一本书永久挡在门外。
///
/// # Errors
///
/// 同 [`load_book`]，只是不再因替换字符密度报「疑似编码不匹配」。
pub fn load_book_relaxed(
    path: &Path,
    format: BookFormat,
    encoding: Option<&str>,
) -> Result<ReaderBook, ReaderError> {
    load_book_impl(path, format, encoding, false)
}

fn load_book_impl(
    path: &Path,
    format: BookFormat,
    encoding: Option<&str>,
    check_text: bool,
) -> Result<ReaderBook, ReaderError> {
    if !format.is_readable() {
        return Err(ReaderError::UnsupportedFormat(format));
    }
    if !path.is_file() {
        return Err(ReaderError::MissingFile);
    }
    let explicit = crate::tools::encoding::parse_encoding(encoding)
        .map_err(|_| ReaderError::InvalidEncoding(encoding.unwrap_or_default().to_string()))?;
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
    // 有损解码不会失败，只会产出替换字符；TXT 才需要按密度反推「编码选错了」。
    if check_text && format == BookFormat::Txt {
        check_decoded_text(format, &chapters)?;
    }
    // 记录解码器实际使用的编码（BOM 可能覆盖显式标签）；EPUB 不经过编码标签，恒为 `None`。
    let encoding = if format == BookFormat::Txt {
        source.encoding
    } else {
        None
    };
    Ok(ReaderBook {
        title: source.title,
        author: source.author,
        chapters,
        encoding,
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

/// 判断「解出来的字像是乱码」时最多数多少个字符：10 MB 的小说没必要全量扫描。
const REPLACEMENT_SAMPLE_LIMIT: usize = 200_000;
/// 替换字符占样本的千分之一以上，就认定编码不匹配（正确解码时基本上是 0）。
const REPLACEMENT_PER_MILLE_LIMIT: usize = 1;
/// 替换字符少于这个数量一律放过：正常文件偶尔带一个（编辑残留）不该被拦。
const REPLACEMENT_MIN_COUNT: usize = 8;

/// 有损解码不会失败，只会把解不出的字节换成 U+FFFD。这里按替换字符的密度反推
/// 「编码选错了」，让 UI 有机会给出「换编码重试」。
fn check_decoded_text(format: BookFormat, chapters: &[ReaderChapter]) -> Result<(), ReaderError> {
    let mut total = 0_usize;
    let mut broken = 0_usize;
    for line in chapters.iter().flat_map(|chapter| &chapter.paragraphs) {
        for ch in line.chars() {
            total += 1;
            if ch == '\u{FFFD}' {
                broken += 1;
            }
            if total >= REPLACEMENT_SAMPLE_LIMIT {
                break;
            }
        }
        if total >= REPLACEMENT_SAMPLE_LIMIT {
            break;
        }
    }
    // 全用整数比较：`usize as f32` 会撞 `cast_precision_loss`（无全局放行）。
    if total > 0
        && broken >= REPLACEMENT_MIN_COUNT
        && broken.saturating_mul(1000) > total.saturating_mul(REPLACEMENT_PER_MILLE_LIMIT)
    {
        return Err(ReaderError::Decode(
            format,
            t!("Reader.error.decode_detail", total = total, broken = broken).to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

    use super::*;

    // 每行之间都要有空行：标题行必须独占一个段落才会被识别成章节，
    // 否则「第一章 开始\n正文一」会被当成一段普通正文。
    const TXT: &str = "第一章 开始\n\n正文一\n\n第二章 继续\n\n正文二\n";

    fn write_epub(path: &std::path::Path) {
        let zip = ZipLibrary::new().unwrap();
        let mut builder = EpubBuilder::new(zip).unwrap();
        builder.metadata("title", "元数据书名").unwrap();
        builder.metadata("author", "作者乙").unwrap();
        builder
            .add_content(
                EpubContent::new(
                    "c1.xhtml",
                    "<html><body><h1>第一章</h1><p>正文一</p></body></html>".as_bytes(),
                )
                .title("第一章"),
            )
            .unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        builder.generate(&mut file).unwrap();
    }

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
        assert_eq!(book.encoding.as_deref(), Some("GBK"));
    }

    #[test]
    fn bom_wins_over_explicit_encoding_for_recording() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        // UTF-8 BOM + 正文；用户错误地显式选了 GBK。
        std::fs::write(
            &path,
            b"\xEF\xBB\xBF\xE7\xAC\xAC\xE4\xB8\x80\xE7\xAB\xA0\n\xE6\xAD\xA3\xE6\x96\x87",
        )
        .unwrap();

        let book =
            crate::reader::load_book(&path, crate::library::BookFormat::Txt, Some("GBK")).unwrap();
        assert_eq!(
            book.encoding.as_deref(),
            Some("UTF-8"),
            "应记录实际解码编码"
        );
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
    fn explicit_label_beats_detection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf8.txt");
        // UTF-8 字节 + 显式 GBK：探测会说是 UTF-8，只有真的按显式标签解码才会得到乱码。
        // 正文重复几遍，保证 GBK 解码出的替换字符数量能越过绝对下限、密度检查真正触发。
        std::fs::write(&path, TXT.repeat(10)).unwrap();

        // 对照：自动探测能正常读出，说明文件本身是合法 UTF-8。
        let auto = load_book(&path, BookFormat::Txt, None).unwrap();
        assert_eq!(auto.encoding.as_deref(), Some("UTF-8"));

        // 显式标签必须真正传给了解码器：按 GBK 解码 UTF-8 字节会产出大量替换字符，
        // 现在会报 Decode（而不是静默返回满屏乱码）。
        assert!(matches!(
            load_book(&path, BookFormat::Txt, Some("gbk")),
            Err(ReaderError::Decode(BookFormat::Txt, _))
        ));
    }

    #[test]
    fn loads_epub_with_chapters_and_no_encoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("书.epub");
        write_epub(&path);

        let book = load_book(&path, BookFormat::Epub, Some("gbk")).unwrap();
        assert_eq!(book.title, "元数据书名");
        assert_eq!(book.author, "作者乙");
        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.chapters[0].paragraphs, vec!["正文一"]);
        assert!(
            book.encoding.is_none(),
            "EPUB 不接受编码标签，不能报一个没用过的编码"
        );
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
    fn gbk_bytes_decoded_as_utf8_reports_decode_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gbk.txt");
        let (bytes, _, _) = encoding_rs::GBK.encode(TXT);
        std::fs::write(&path, &bytes).unwrap();

        // 有损解码不会失败：替换字符密度检查必须把这种「满屏乱码」变成 Decode 错误。
        assert!(matches!(
            load_book(&path, BookFormat::Txt, Some("UTF-8")),
            Err(ReaderError::Decode(BookFormat::Txt, _))
        ));
    }

    #[test]
    fn correct_encoding_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gbk.txt");
        let (bytes, _, _) = encoding_rs::GBK.encode(TXT);
        std::fs::write(&path, &bytes).unwrap();

        let book = load_book(&path, BookFormat::Txt, Some("GBK")).unwrap();
        assert_eq!(book.chapters[0].paragraphs, vec!["正文一"]);
    }

    #[test]
    fn utf8_text_has_no_replacement_chars() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf8.txt");
        std::fs::write(&path, TXT).unwrap();

        let book = load_book(&path, BookFormat::Txt, None).unwrap();
        let broken = book
            .chapters
            .iter()
            .flat_map(|chapter| &chapter.paragraphs)
            .flat_map(|line| line.chars())
            .filter(|ch| *ch == '\u{FFFD}')
            .count();
        assert_eq!(broken, 0, "正确解码的 UTF-8 不应被判成乱码");
    }

    #[test]
    fn one_replacement_char_in_short_text_still_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("short.txt");
        std::fs::write(&path, "第一章 开始\n\n正文\u{FFFD}正文\n").unwrap();

        assert!(
            load_book(&path, BookFormat::Txt, None).is_ok(),
            "少量替换字符（编辑残留）不该被当成编码错"
        );
    }

    #[test]
    fn relaxed_load_ignores_mojibake_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gbk.txt");
        let (bytes, _, _) = encoding_rs::GBK.encode(TXT);
        std::fs::write(&path, &bytes).unwrap();

        assert!(matches!(
            load_book(&path, BookFormat::Txt, Some("UTF-8")),
            Err(ReaderError::Decode(BookFormat::Txt, _))
        ));
        let book = load_book_relaxed(&path, BookFormat::Txt, Some("UTF-8")).unwrap();
        assert!(!book.chapters.is_empty());
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
    fn detail_only_exposes_technical_text() {
        // 中文 `Display` 只给日志看：标题已由 `message_key()` 翻译过，再当成 UI 细节就重复了。
        assert!(ReaderError::MissingFile.detail().is_none());
        assert!(ReaderError::NoContent.detail().is_none());
        assert!(
            ReaderError::UnsupportedFormat(BookFormat::Pdf)
                .detail()
                .is_none()
        );
        assert!(ReaderError::Epub("坏 zip".to_string()).detail().is_none());
        assert_eq!(
            ReaderError::Decode(BookFormat::Txt, "msg".to_string()).detail(),
            Some("msg".to_string())
        );
        assert_eq!(
            ReaderError::InvalidEncoding("klingon".to_string()).detail(),
            Some("klingon".to_string())
        );
        assert!(
            ReaderError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
                .detail()
                .is_some(),
            "系统错误原文仍要作为细节展示"
        );
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
