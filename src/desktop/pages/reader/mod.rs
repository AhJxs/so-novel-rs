//! 应用内阅读器：覆盖式视图，不作为第 7 个导航页。
//!
//! 注意区分：本模块是阅读器的桌面 UI 外壳，数据层仍是 [`crate::reader`]。

mod bookmarks_panel;
mod content_view;
mod search_panel;
mod toc;
mod typography;

pub mod reader_page;
pub mod theme;

pub use reader_page::{CloseReaderFn, ReaderPage};
pub use theme::reader_colors;
