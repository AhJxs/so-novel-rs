//! 规则模块错误类型

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
        /// 出问题的规则文件路径。
        path: PathBuf,
        /// 底层 IO 错误。
        #[source]
        source: std::io::Error,
    },
    /// 规则文件解析失败 (json / json5 错误信息已格式化)。
    #[error("规则文件解析失败 {path}: {message}")]
    Parse {
        /// 出问题的规则文件路径。
        path: PathBuf,
        /// 已格式化的解析错误信息。
        message: String,
    },
}
