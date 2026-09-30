//! 详情页解析后的书籍数据。Java 端复用 `Rule.Book` 承载"规则 + 数据", 这里拆开: 规则 →
//! `crate::models::rule::RuleBook`, 数据 → 本结构体 `Book`。`Book` 兼 PO (落 `task_record`) 与
//! DTO (Web API `/book` 端点); 字段名沿用 camelCase, 与 web-ui 前端对齐。

use serde::{Deserialize, Serialize};

/// 详情页解析后的书籍数据。字段几乎全是 `Option` / 带默认值: 各书源详情页结构差异大, 没有哪个
/// 字段是所有书源都填的; `Default` 让未完整解析的 `Book` 也能安全构造, 业务层用
/// `book.book_name.is_empty()` 判 "详情页失败"。
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
    /// 最后更新时间 (字符串原始值, 不做时区解析); 格式因书源而异 (如 `"2 hours ago"` / `"昨天 18:30"`)。
    pub last_update_time: Option<String>,
    /// 连载状态 (如 "连载中" / "已完结" / "完本")。由书源文案决定, 不做枚举归一化。
    pub status: Option<String>,
    /// 书源语言 (如 `zh-CN`、`zh-TW`)，由解析时从 rule.language 填入。
    /// 决定下载章节正文的目标语言变体 (见 `crate::config::Language::to_book_target_lang`)。
    #[serde(default)]
    pub language: String,
}
