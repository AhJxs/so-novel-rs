//! 下载进度事件 (PR #17 拆分, 2026-07-08).
//!
//! `Progress` 是调度层 → UI 的消息协议, 通过 `mpsc::UnboundedSender<Progress>`
//! 推送, UI 端 ([`crate::app::events`]) 排空后触发重绘。

use std::path::PathBuf;

use crate::models::Book;

/// 调度层用户可见的进度事件。
///
/// # Examples
///
/// ```ignore
/// tx.send(Progress::BookResolved { book: Box::new(b), total_chapters: 100 })?;
/// tx.send(Progress::ChapterDone { index: 1, title: "第1章".into() })?;
/// tx.send(Progress::Finished { output_path: out_path })?;
/// ```
#[derive(Debug, Clone)]
pub enum Progress {
    /// 详情解析完成, 得到书籍元信息。
    BookResolved {
        /// 已解析的 Book (heap-allocated 避免单事件大拷贝)。
        book: Box<Book>,
        /// 目录总章节数 (用户界面估算进度用)。
        total_chapters: usize,
    },
    /// 一章完成 (成功), index 是 1-based 顺序号。
    ChapterDone { index: u32, title: String },
    /// 一章失败 (已用尽重试, 但不中断整本下载)。
    ChapterFailed {
        index: u32,
        title: String,
        reason: String,
    },
    /// 导出完成, 文件已落盘。
    Finished { output_path: PathBuf },
    /// 用户取消 (在某章完成 / 失败之后的下一次检查点观测到)。
    Cancelled,
    /// 下载失败 (详情/目录/写盘/导出等终态错误)。让 UI 能区分"取消"与"失败"。
    Failed { reason: String },
}
