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
pub mod zhconv;

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

/// 工具运行阶段；决定进度条文案与权重。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStage {
    /// 读取并解析源文件。
    Loading,
    /// 逐章渲染正文。
    Rendering,
    /// 合并导出 / 写盘。
    Writing,
}

impl ToolStage {
    /// 阶段文案的 i18n key（`Tools.stage.*`）。
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Loading => "Tools.stage.loading",
            Self::Rendering => "Tools.stage.rendering",
            Self::Writing => "Tools.stage.writing",
        }
    }
}

/// 一次进度回报：阶段 + 阶段内完成量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolProgress {
    pub stage: ToolStage,
    pub done: usize,
    pub total: usize,
}

impl ToolProgress {
    /// 构造一次进度回报。
    #[must_use]
    pub const fn new(stage: ToolStage, done: usize, total: usize) -> Self {
        Self { stage, done, total }
    }

    /// 映射到整条流程的 0.0~1.0 进度。
    ///
    /// 权重按耗时分配：读取 15%、逐章渲染 75%、写盘 10%；没有分章的文件（编码 / 简繁）
    /// 只有读取与写盘两段，中间直接跨过。
    #[must_use]
    pub fn fraction(self) -> f32 {
        // 先在整数域算千分比，再精确转 f32，避免 usize→f32 的精度损失告警。
        let permille = self
            .done
            .min(self.total)
            .saturating_mul(1000)
            .checked_div(self.total)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(0);
        let ratio = f32::from(permille) / 1000.0;
        match self.stage {
            ToolStage::Loading => 0.15 * ratio,
            ToolStage::Rendering => 0.75_f32.mul_add(ratio, 0.15),
            ToolStage::Writing => 0.10_f32.mul_add(ratio, 0.90),
        }
    }

    /// 百分比（0.0~100.0），直接喂给 `Progress::value`。
    #[must_use]
    pub fn percent(self) -> f32 {
        self.fraction() * 100.0
    }
}

/// 执行一次工具请求，返回产物路径（不关心进度）。
///
/// # Errors
///
/// 透传各业务流程的 [`ToolError`]。
pub fn run(request: ToolRequest) -> Result<PathBuf, ToolError> {
    run_with_progress(request, |_| {})
}

/// 执行一次工具请求，并回报进度。
///
/// # Errors
///
/// 透传各业务流程的 [`ToolError`]。
pub fn run_with_progress(
    request: ToolRequest,
    mut on_progress: impl FnMut(ToolProgress),
) -> Result<PathBuf, ToolError> {
    let on_progress: &mut dyn FnMut(ToolProgress) = &mut on_progress;
    match request {
        ToolRequest::Convert {
            source,
            out_dir,
            target,
            txt_encoding,
            source_encoding,
            chapter_regex,
        } => convert::run_convert(
            &source,
            &out_dir,
            target,
            &txt_encoding,
            source_encoding.as_deref(),
            chapter_regex.as_deref(),
            on_progress,
        ),
        ToolRequest::Encoding {
            source,
            out_dir,
            source_encoding,
            target_encoding,
        } => encoding::run_encoding(
            &source,
            &out_dir,
            source_encoding.as_deref(),
            &target_encoding,
            on_progress,
        ),
        ToolRequest::Zhconv {
            source,
            out_dir,
            target,
        } => zhconv::run_zhconv(&source, &out_dir, target, on_progress),
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

    #[test]
    fn run_dispatches_convert() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "第1章 开始\n\n正文\n").unwrap();

        let path = super::run(super::ToolRequest::Convert {
            source,
            out_dir: tmp.path().join("out"),
            target: crate::config::ExportFormat::Txt,
            txt_encoding: "UTF-8".to_string(),
            source_encoding: None,
            chapter_regex: None,
        })
        .unwrap();

        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("txt"));
    }

    #[test]
    fn progress_walks_stages_monotonically_to_full() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "第1章 开始\n\n正文\n\n第2章 继续\n\n正文\n").unwrap();

        let mut samples = Vec::new();
        super::run_with_progress(
            super::ToolRequest::Convert {
                source,
                out_dir: tmp.path().join("out"),
                target: crate::config::ExportFormat::Txt,
                txt_encoding: "UTF-8".to_string(),
                source_encoding: None,
                chapter_regex: None,
            },
            |progress| samples.push(progress),
        )
        .unwrap();

        assert!(samples.iter().all(|p| (0.0..=100.0).contains(&p.percent())));
        assert!(
            samples
                .windows(2)
                .all(|pair| pair[0].percent() <= pair[1].percent()),
            "进度不应回退: {samples:?}"
        );
        let first = samples.first().unwrap();
        let last = samples.last().unwrap();
        assert_eq!(first.stage, super::ToolStage::Loading);
        assert_eq!(last.stage, super::ToolStage::Writing);
        assert!((last.percent() - 100.0).abs() < f32::EPSILON);
        assert!(
            samples
                .iter()
                .any(|p| p.stage == super::ToolStage::Rendering)
        );
    }

    #[test]
    fn progress_fraction_weights_stages() {
        use super::{ToolProgress as P, ToolStage as S};
        assert!(P::new(S::Loading, 0, 1).percent().abs() < f32::EPSILON);
        assert!((P::new(S::Loading, 1, 1).percent() - 15.0).abs() < 0.001);
        assert!((P::new(S::Rendering, 0, 10).percent() - 15.0).abs() < 0.001);
        assert!((P::new(S::Rendering, 10, 10).percent() - 90.0).abs() < 0.001);
        assert!((P::new(S::Writing, 1, 1).percent() - 100.0).abs() < 0.001);
        // total 为 0 时不除零，按阶段起点算。
        assert!((P::new(S::Rendering, 0, 0).percent() - 15.0).abs() < 0.001);
    }
}
