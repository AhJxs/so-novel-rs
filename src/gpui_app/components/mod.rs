//! 共享 GPUI 组件 / 格式化工具。
//!
//! 严守"小工具 + 复用 gpui-component"原则：
//! - **不**重新实现 button / input / popup / icon font。
//! - **不**复制 theme palette — 颜色全部从 `cx.theme()` 取。
//! - 仅暴露：EmptyState / Pagination / `PageHeader` / `StatusBadge` / formatting 工具。
//!
//! 页面专属卡片（搜索结果行 / 下载任务行 / 书源行）在各 page 模块里建，
//! 避免本模块变成"第二个 design system"。

pub mod empty_state;
pub mod page_header;
pub mod pagination;
pub mod status_badge;

pub use crate::utils::formatting::{format_size, truncate};
pub use empty_state::EmptyState;
pub use page_header::PageHeader;
pub use pagination::{PAGE_SIZE, PageSlice, Pagination, compute_page_window};
pub use status_badge::{StatusBadge, StatusKind};
