//! 选章下载的目录（TOC）预取缓存与后台通道。
//!
//! 搜索页打开「选择章节」对话框时按 `(源 ID, 书 URL)` 预取书籍详情 + 目录，结果缓存在这里；
//! 对话框本身只是这条缓存的一个反应式读者，不拥有任何 TOC 状态。

use std::collections::HashMap;

use tokio::sync::mpsc;

use crate::models::{BookDetail, Chapter};

/// TOC 缓存键：书源 ID + 书籍详情页 URL。
pub type TocKey = (i32, String);

/// TOC 预取加载状态。
#[derive(Debug, Clone)]
pub enum TocState {
    /// 预取尚未返回。
    Pending,
    /// 已拿到书籍详情与章节目录。
    Loaded(Box<BookDetail>, Vec<Chapter>),
    /// 预取失败，附错误文本。
    Failed(String),
}

/// TOC 预取后台 → UI 通道事件。
#[derive(Debug)]
pub struct TocEvent {
    /// 完成预取的书源 ID。
    pub source_id: i32,
    /// 该书的详情页 URL，作为缓存键的另一半。
    pub url: String,
    /// 预取结果状态。
    pub state: TocState,
}

/// TOC 预取缓存与后台通道。
///
/// 只有 [`super::SearchState`] 持有实例；`clear_results_and_caches` 会连同换源时的缓存
/// 一起丢掉整张表（`SearchState::default()`）。
#[derive(Default)]
pub struct TocCache {
    /// 每个 `(源 ID, 书 URL)` 的预取状态。
    pub entries: HashMap<TocKey, TocState>,
    /// 预取后台通道；`drain_toc` 排空，`sender` 端 drop 后置回 `None`。
    pub rx: Option<mpsc::UnboundedReceiver<TocEvent>>,
}
