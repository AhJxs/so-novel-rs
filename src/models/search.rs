use serde::{Deserialize, Serialize};

/// 单条搜索结果。对应 Java `model.SearchResult`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub source_id: i32,
    pub source_name: String,
    pub url: String,
    pub book_name: String,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub category: Option<String>,
    pub latest_chapter: Option<String>,
    pub last_update_time: Option<String>,
    pub status: Option<String>,
    pub word_count: Option<String>,
}
