use serde::{Deserialize, Serialize};

/// 书源摘要（用于书源管理页 / 聚合搜索连通性面板）。
/// 对应 Java `model.SourceInfo`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub comment: Option<String>,
    #[serde(default)]
    pub need_proxy: bool,
    #[serde(default)]
    pub disabled: bool,
    /// HEAD 请求耗时（ms）；-1 表示失败。
    pub delay_ms: Option<i32>,
    /// HTTP 状态码；-1 表示失败。
    pub http_status: Option<i32>,
}
