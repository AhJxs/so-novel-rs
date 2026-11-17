//! 本地小说文件工具：格式转换 / 编码转换 / 简繁转换。
//!
//! UI 只依赖本模块的 [`ToolRequest`] / [`ToolError`]；具体实现按职责拆到
//! `load`（输入解析）、`split`（切章）、`render`（章节渲染）、`convert` /
//! `encoding` / `zhconv`（三条流程）。

pub mod convert;
pub mod encoding;
pub mod load;
pub mod render;
pub mod source_book;
pub mod split;

use std::path::PathBuf;

use thiserror::Error;

use crate::config::{ExportFormat, LangType};
use crate::export::ExportError;

pub use encoding::detect_file_encoding;
pub use source_book::{SourceBook, SourceChapter};

/// 一次工具请求（UI → 业务层）。
#[derive(Debug, Clone)]
pub enum ToolRequest {
    /// 本地文件 → EPUB / TXT / HTML / PDF / Markdown。
    Convert {
        source: PathBuf,
        out_dir: PathBuf,
        target: ExportFormat,
        txt_encoding: String,
        source_encoding: Option<String>,
        chapter_regex: Option<String>,
    },
    /// TXT 文件编码转换。
    Encoding {
        source: PathBuf,
        out_dir: PathBuf,
        source_encoding: Option<String>,
        target_encoding: String,
    },
    /// TXT / Markdown 简繁转换。
    Zhconv {
        source: PathBuf,
        out_dir: PathBuf,
        target: LangType,
    },
}

/// 工具业务层错误。
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("不支持的文件格式: {0}")]
    UnsupportedFormat(String),
    #[error("读取文件失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("解码文件失败: {0}")]
    Decode(String),
    #[error("章节正则无效: {0}")]
    InvalidRegex(String),
    #[error("ZIP 结构异常: {0}")]
    ZipStructure(String),
    #[error("EPUB 结构异常: {0}")]
    EpubStructure(String),
    #[error("文件没有可转换的正文")]
    NoContent,
    #[error("目标编码无法表示全部字符: {0}")]
    Unencodable(String),
    #[error("未知编码: {0}")]
    UnknownEncoding(String),
    #[error("导出失败: {0}")]
    Export(#[from] ExportError),
}

impl ToolError {
    /// 对应的 i18n key（`Tools.error.*`），UI 用它展示三语错误摘要。
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::UnsupportedFormat(_) => "Tools.error.unsupported_format",
            Self::Io(_) => "Tools.error.io",
            Self::Decode(_) => "Tools.error.decode",
            Self::InvalidRegex(_) => "Tools.error.invalid_regex",
            Self::ZipStructure(_) => "Tools.error.zip_structure",
            Self::EpubStructure(_) => "Tools.error.epub_structure",
            Self::NoContent => "Tools.error.no_content",
            Self::Unencodable(_) => "Tools.error.unencodable",
            Self::UnknownEncoding(_) => "Tools.error.unknown_encoding",
            Self::Export(_) => "Tools.error.export",
        }
    }

    /// 技术细节（可能含路径 / 正则错误），UI 以弱化小字展示。
    #[must_use]
    pub fn detail(&self) -> String {
        self.to_string()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::ToolError;

    #[test]
    fn every_error_has_message_key() {
        assert_eq!(ToolError::NoContent.message_key(), "Tools.error.no_content");
        assert_eq!(
            ToolError::InvalidRegex("x".to_string()).message_key(),
            "Tools.error.invalid_regex"
        );
        assert_eq!(
            ToolError::UnknownEncoding("x".to_string()).message_key(),
            "Tools.error.unknown_encoding"
        );
    }
}
