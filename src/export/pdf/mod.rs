//! PDF 导出 —— 对应 Java `handle.PdfMergeHandler`。
//!
//! 读 `chapters_dir` 下按文件名升序的章节 HTML (每章一个, Pdf 模式由 `write_chapter_files`
//! 写出), 用 `pdf_oxide::DocumentBuilder` 直接构建 PDF —— **不走** `from_html_css` 的
//! HTML→DOM→Taffy 管道: 该管道对中文小说排版问题多 (字号/行距/缩进/分页不受控)。
//!
//! 子模块: [`document`] (主流程 + 排版常量)、[`chapters`] (HTML → 结构化内容)、
//! [`fonts`] (CJK 字体发现 + 量宽)。无 CJK 字体时中文是 tofu (建议装 Noto Sans CJK)。

pub mod chapters;
pub mod document;
pub mod fonts;

pub use document::PdfExporter;
