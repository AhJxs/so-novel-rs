//! 本地文件 → [`SourceBook`]。按扩展名分派，所有文本都归一化为纯文本段落。

pub mod html;
pub mod markdown;
pub mod txt;

use std::path::Path;

use encoding_rs::Encoding;

use super::ToolError;
use super::source_book::SourceBook;
use super::split;

/// 加载选项。
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadOptions<'a> {
    /// 用户显式指定的源编码；`None` = BOM / 自动探测。
    pub source_encoding: Option<&'static Encoding>,
    /// 用户自定义章节正则；`None` = 内置规则。
    pub chapter_regex: Option<&'a str>,
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
        _ => Err(ToolError::UnsupportedFormat(ext)),
    }
}

pub(super) fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}
