//! 章节模型 (PR #10 文档化, 2026-07-08)
//!
//! 章节是下载任务的核心数据单元。同时承担 PO 角色 (落盘到 `chapters/` 目录)
//! 和 DTO 角色 (Web SSE 推送 `ProgressEvent` / Web API 响应)。

use serde::{Deserialize, Serialize};

/// 单章数据。对应 Java `model.Chapter`。
///
/// `content` 是 HTML (原始解析结果), 过滤 (filter.rs) + 格式化 (formatter.rs) 在
/// export 阶段做。`content.is_empty()` 表示 "章节正文为空", 是 `ChapterError::EmptyContent`
/// 错误的判定依据。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    /// 章节详情页 URL (书源内唯一)。
    pub url: String,
    /// 章节标题 (用户展示用, 可能在 export 阶段被 truncate)。
    pub title: String,
    /// 章节正文 (HTML, 解析后)。
    pub content: String,
    /// 序号 (从 1 开始), 用于落盘文件名前缀补零排序 (e.g. `0001-第1章.html`)。
    pub order: u32,
}
