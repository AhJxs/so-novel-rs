//! 本地文件 → [`SourceBook`]。按扩展名分派，所有文本都归一化为纯文本段落。

pub mod epub;
pub mod html;
pub mod markdown;
pub mod txt;

use std::path::Path;

use encoding_rs::Encoding;

use super::source_book::SourceBook;
use super::{ToolError, split};

/// 加载选项。
#[derive(Debug, Clone, Copy)]
pub struct LoadOptions<'a> {
    /// 用户显式指定的源编码；`None` = BOM / 自动探测。
    pub source_encoding: Option<&'static Encoding>,
    /// 用户自定义章节正则；`None` = 内置规则。
    pub chapter_regex: Option<&'a str>,
    /// 是否解压 EPUB 封面；阅读器不显示封面，可跳过这次 ZIP 条目读取。
    pub include_cover: bool,
}

impl Default for LoadOptions<'_> {
    /// 默认读封面：工具箱 / 转换流程沿用原有行为，只有阅读器显式关掉。
    fn default() -> Self {
        Self {
            source_encoding: None,
            chapter_regex: None,
            include_cover: true,
        }
    }
}

/// 按扩展名加载文件。
///
/// # Errors
///
/// 扩展名不支持、解码失败、正则无效、文件无正文时返回 [`ToolError`]。
pub fn load_path(path: &Path, opts: &LoadOptions<'_>) -> Result<SourceBook, ToolError> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "txt" => txt::load(path, opts, &split::compile_pattern(opts.chapter_regex)?),
        "md" | "markdown" => {
            markdown::load(path, opts, &split::compile_pattern(opts.chapter_regex)?)
        }
        "html" | "htm" => html::load_file(path, opts, &split::compile_pattern(opts.chapter_regex)?),
        "zip" => html::load_zip(path),
        "epub" => epub::load(path, opts).map(|mut book| {
            book.chapters = split::normalize_chapters(book.chapters);
            book
        }),
        _ => Err(ToolError::UnsupportedFormat(ext)),
    }
}

pub(super) fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}
