use serde::{Deserialize, Serialize};

/// 详情页解析后的书籍数据。
///
/// Java 端把"详情规则"和"详情数据"都叫做 `Rule.Book` 复用一个类，本 Rust 端拆开：
/// - 规则 → `crate::models::rule::RuleBook`
/// - 数据 → 本结构体 `Book`
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Book {
    pub url: String,
    pub book_name: String,
    pub author: String,
    pub intro: Option<String>,
    pub category: Option<String>,
    pub cover_url: Option<String>,
    pub latest_chapter: Option<String>,
    pub latest_chapter_url: Option<String>,
    pub last_update_time: Option<String>,
    pub status: Option<String>,
    /// 书源语言（如 `zh-CN`、`zh-TW`），由解析时从 rule.language 填入。
    #[serde(default)]
    pub language: String,
}
