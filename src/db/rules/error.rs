//! 规则模块错误类型 (PR #17 拆分, 2026-07-08).

use std::path::PathBuf;

use thiserror::Error;

/// 规则加载/解析错误。
#[derive(Debug, Error)]
pub enum RulesError {
    /// 规则路径不存在。
    #[error("规则路径不存在: {0}")]
    NotFound(PathBuf),
    /// 规则文件读取失败 (含 IO 错误源)。
    #[error("规则文件读取失败 {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// 规则文件解析失败 (json / json5 错误信息已格式化)。
    #[error("规则文件解析失败 {path}: {message}")]
    Parse { path: PathBuf, message: String },
}
