use std::path::PathBuf;

use super::RulesError;

/// 顶层 dao 错误: 业务层用 `?` 一步透传到 [`crate::error::AppError::Db`]；`RulesError` 仍保留
/// (有路径 + 原因, 不能丢), 通过 `From<RulesError> for DaoError` 归一。
#[derive(Debug, thiserror::Error)]
pub enum DaoError {
    /// IO 错误 (读 / 写 / 文件锁 / 权限)。
    #[error("dao IO 错误 {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// JSON 反序列化失败。
    #[error("dao JSON 错误 {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// 规则加载错误 (来自 `RulesError`)。
    #[error("dao 规则错误: {0}")]
    Rules(#[from] RulesError),
    /// 资源不存在 (文件 / 目录)。
    #[error("dao 资源不存在: {0}")]
    NotFound(PathBuf),
}

impl From<DaoError> for crate::error::AppError {
    fn from(e: DaoError) -> Self {
        Self::db(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn dao_error_io_includes_path() {
        let e = DaoError::Io {
            path: PathBuf::from("/x/y"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };
        let s = e.to_string();
        assert!(s.contains("/x/y"));
        assert!(s.contains("missing"));
    }

    #[test]
    fn dao_error_from_rules_error() {
        let r = RulesError::NotFound(PathBuf::from("/rules"));
        let d: DaoError = r.into();
        assert!(matches!(d, DaoError::Rules(_)));
    }

    #[test]
    fn dao_error_to_app_error() {
        let d = DaoError::NotFound(PathBuf::from("/missing"));
        let a: crate::error::AppError = d.into();
        assert!(matches!(a, crate::error::AppError::Db(_)));
    }
}
