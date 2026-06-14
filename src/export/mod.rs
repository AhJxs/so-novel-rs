//! 导出层。
//!
//! - `render`：章节正文 → 目标格式字符串（阶段 3a）；
//! - `exporter`：`Exporter` trait + `write_chapter_files` 等共用工具（阶段 3b）；
//! - `txt` / `html` / `epub`：三种导出器实现（阶段 3b）。
//!
//! 不实现 PDF（详见 audit §6.4），命中时降级 Html。

pub mod epub;
pub mod exporter;
pub mod html;
pub mod render;
pub mod txt;

pub use exporter::{
    build_book_dir_name, exporter_for, sort_chapter_files, write_chapter_files, ExportError,
    Exporter, RenderedChapter,
};
pub use render::{render_chapter, RenderTarget};
