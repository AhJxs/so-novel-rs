//! 详情页解析后的书籍数据 (PR #10 文档化, 2026-07-08)
//!
//! Java 端把"详情规则"和"详情数据"都叫做 `Rule.Book` 复用一个类, 本 Rust 端拆开:
//! - 规则 → `crate::models::rule::RuleBook`
//! - 数据 → 本结构体 `Book`
//!
//! `Book` 同时承担 **持久化对象 (PO)** 和 **传输对象 (DTO)** 双重角色:
//! - PO 角色: 下载任务完成时落 `task_record` 关联
//! - DTO 角色: Web API `/book` 端点 JSON 序列化
//!
//! 字段命名沿用 `camelCase` (与 `Rule` 一致), 跟现有 web-ui 前端字段对齐。

use serde::{Deserialize, Serialize};

/// 详情页解析后的书籍数据。
///
/// 字段全部 `Option<String>` 或默认值, 原因是不同书源详情页结构差异大 ——
/// 没有哪个字段是所有书源都填的。`Default` derive 让未完整解析的 Book 也能
/// 安全构造, 业务层用 `book.book_name.is_empty()` 判 "详情页失败"。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Book {
    /// 详情页 URL (书源唯一标识, 跟 Rule.url 对齐)。
    pub url: String,
    /// 书名 (必填, 详情页核心数据)。
    pub book_name: String,
    /// 作者 (必填, 详情页核心数据)。
    pub author: String,
    /// 简介 / 内容说明。可能缺失或被书源脱敏为空。
    pub intro: Option<String>,
    /// 分类 (如 "玄幻" / "都市" / "科幻")。部分书源没分类。
    pub category: Option<String>,
    /// 封面图 URL。下载时由 export 层去拉字节。
    pub cover_url: Option<String>,
    /// 最新章节标题 (仅展示用, 跳转链接用 `latest_chapter_url`)。
    pub latest_chapter: Option<String>,
    /// 最新章节详情页 URL。`latest_chapter` 跟 `latest_chapter_url` 必须同时存在
    /// 或同时缺失, UI 点击才能跳。
    pub latest_chapter_url: Option<String>,
    /// 最后更新时间 (字符串, 原始值, 不做时区解析)。格式因书源而异
    /// (`"2024-01-15"` / `"2 hours ago"` / `"昨天 18:30"` 等)。
    pub last_update_time: Option<String>,
    /// 连载状态 (如 "连载中" / "已完结" / "完本")。由书源文案决定,
    /// 不做枚举归一化 (因为不同书源用词不同)。
    pub status: Option<String>,
    /// 书源语言 (如 `zh-CN`、`zh-TW`)，由解析时从 rule.language 填入。
    /// 决定下载章节正文的目标语言变体 (见 `crate::config::Language::to_book_target_lang`)。
    #[serde(default)]
    pub language: String,
}
