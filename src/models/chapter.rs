use serde::{Deserialize, Serialize};

/// 单章数据。对应 Java `model.Chapter`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    pub url: String,
    pub title: String,
    pub content: String,
    /// 序号（从 1 开始），用于落盘文件名前缀补零排序。
    pub order: u32,
}
