//! 搜索结果 (PR #10 文档化, 2026-07-08)
//!
//! 聚合搜索 (PR #6 加 `search_filter`) 把多书源结果去重 + 排序, 产出
//! `SearchResult` 列表给 UI 展示。

use serde::{Deserialize, Serialize};

/// 单条搜索结果。对应 Java `model.SearchResult`。
///
/// 字段命名跟 `Book` 故意保持一致 — UI 层统一渲染逻辑 (复用组件)。
/// `source_id` 跟 `source_name` 是冗余的 (用 id 取 Rule 也能拿 name), 但搜索结果
/// 缓存要尽量自包含, 减少后续点击时的二次查表。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    /// 书源 ID (回查 Rule 用)。
    pub source_id: i32,
    /// 书源名称 (UI 直接展示, 避免二次查表)。
    pub source_name: String,
    /// 详情页 URL (点击进 detail 用)。
    pub url: String,
    /// 书名。
    pub book_name: String,
    /// 作者 (部分书源搜索结果不显示作者, 缺失时为 `None`)。
    pub author: Option<String>,
    /// 简介 (部分书源搜索页不显示简介, 缺失)。
    pub intro: Option<String>,
    /// 分类 (同 `Book::category`, 可能缺失)。
    pub category: Option<String>,
    /// 最新章节标题 (仅展示, 搜索页一般不跳)。
    pub latest_chapter: Option<String>,
    /// 最后更新时间 (字符串, 原始值, 不做时区解析)。
    pub last_update_time: Option<String>,
    /// 连载状态 (同 `Book::status`)。
    pub status: Option<String>,
    /// 字数 (字符串, 部分书源提供, 不强求)。
    pub word_count: Option<String>,
}
