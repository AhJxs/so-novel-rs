//! 导出层。
//!
//! - `render`：章节正文 → 目标格式字符串（阶段 3a）；
//! - `exporter`：`Exporter` trait + `write_chapter_files` 等共用工具（阶段 3b）；
//! - `txt` / `html` / `epub` / `pdf`：四种导出器实现（阶段 3b + 阶段 7 PDF）。

pub mod epub;
pub mod exporter;
pub mod html;
pub mod pdf;
pub mod render;
pub mod txt;

pub use exporter::{
    ExportError, Exporter, RenderedChapter, build_book_dir_name, exporter_for, sort_chapter_files,
    write_chapter_files,
};
pub use render::{RenderTarget, render_chapter};
