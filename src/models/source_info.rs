//! 书源健康信息 (PR #10 文档化, 2026-07-08)
//!
//! 聚合 `Rule` + 健康检查结果, 给书源管理页面用的视图模型。
//! **不是** 持久化对象, 只在内存里流转。

use serde::{Deserialize, Serialize};

/// 书源摘要 (用于书源管理页 / 聚合搜索连通性面板)。对应 Java `model.SourceInfo`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceInfo {
    /// 书源 ID (跟 `Rule.id` 对齐)。
    pub id: i32,
    /// 书源名称 (`Rule.name`)。
    pub name: String,
    /// 书源 URL (`Rule.url`)。
    pub url: String,
    /// 书源备注 (`Rule.comment`)。
    pub comment: Option<String>,
    /// 是否需要 HTTP 代理 (`Rule.need_proxy`)。
    #[serde(default)]
    pub need_proxy: bool,
    /// 是否被用户禁用 (`Rule.disabled`)。
    #[serde(default)]
    pub disabled: bool,
    /// HEAD 请求耗时 (ms); -1 表示失败。
    pub delay_ms: Option<i32>,
    /// HTTP 状态码; -1 表示失败。
    pub http_status: Option<i32>,
}
