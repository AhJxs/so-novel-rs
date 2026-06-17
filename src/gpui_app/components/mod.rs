//! Stage 5：共享 GPUI 组件 / 格式化工具。
//!
//! 严守"小工具 + 复用 gpui-component"原则：
//! - **不**重新实现 button / input / popup / icon font。
//! - **不**复制 theme palette — 颜色全部从 `cx.theme()` 取。
//! - 仅暴露：EmptyState / Pagination / PageHeader / StatusBadge / formatting 工具。
//!
//! 页面专属卡片（BookResultCard / DownloadTaskCard / SourceCard）留到
//! Stage 7/8/10 在各自 page 模块里建，避免本模块变成"第二个 design system"。

pub mod empty_state;
pub mod formatting;
pub mod page_header;
pub mod pagination;
pub mod status_badge;

pub use empty_state::EmptyState;
pub use formatting::{format_size, truncate};
pub use page_header::PageHeader;
pub use pagination::Pagination;
pub use status_badge::{StatusBadge, StatusKind};
