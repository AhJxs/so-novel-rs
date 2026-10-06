//! 全局错误根类型。
//!
//! 设计：各业务域错误 (`ExportError` / `SearchError` / ...) 保留在自己模块里，
//! 变体携带具体业务上下文；业务层编排函数统一返回 [`AppResult<T>`]，领域错误经 `From`
//! 自动归一，调用方一个 `?` 即可透传；边界层 (`main.rs`) 仍可保留或转回
//! 自己的边界错误类型。
//!
//! `Internal` 是兜底分支，属于「不该发生」场景，出现需排查。`AppError` 不自动打日志 ——
//! 那是决策不是机械动作，由边界函数决定 `tracing::error!`（持久性）还是 `warn!`（可恢复）。

use std::io;

/// 项目根错误。所有业务层 `Result` 类型的 `Err` 端应为此类型。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 配置加载/解析/校验失败。
    #[error("配置错误: {0}")]
    Config(String),

    /// HTTP 请求/响应失败 (含 reqwest / 反爬拦截 / 编码兜底)。
    #[error("网络错误: {0}")]
    Http(String),

    /// HTML / DOM / 选择器 / 章节正文解析失败。
    #[error("解析错误: {0}")]
    Parse(String),

    /// 导出失败 (EPUB/PDF/HTML/TXT/ZIP)。`ExportError` 的强类型透传。
    #[error("导出错误: {0}")]
    Export(#[from] crate::export::ExportError),

    /// 数据访问层失败 (规则/书源/任务的持久化)。
    #[error("数据库/持久化错误: {0}")]
    Db(String),

    /// 标准库 IO 错误。`std::io::Error` 透传, 调用方按 `ErrorKind` 分流。
    #[error("IO 错误: {0}")]
    Io(#[from] io::Error),

    /// JSON 序列化/反序列化失败 (书源 / 任务记录)。
    #[error("JSON 错误: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML 解析/编辑失败 (配置文件读写)。
    #[error("TOML 错误: {0}")]
    Toml(#[from] toml_edit::TomlError),

    /// JS 引擎 (boa) 执行失败 (书源 `@js:` 后处理 / 加密)。
    #[error("JS 引擎错误: {0}")]
    Js(String),

    /// 业务逻辑错误 (编排过程中的不可恢复判断, e.g. 书源规则缺失)。
    #[error("业务错误: {0}")]
    Business(String),

    /// 请求参数错误 (handler 层捕获, 不应进入业务编排)。
    #[error("参数错误: {0}")]
    InvalidArgument(String),

    /// 资源不存在 (书源/任务/文件)。调用方应映射为 404。
    #[error("未找到: {0}")]
    NotFound(String),

    /// 资源状态冲突 (e.g. 重复添加书源)。调用方应映射为 409。
    #[error("冲突: {0}")]
    Conflict(String),

    /// 内部错误, 不应发生。出现必须排查。
    #[error("内部错误: {0}")]
    Internal(String),
}

// 手写 `PartialEq`：`ExportError` / `io::Error` / `serde_json::Error` / `toml_edit::TomlError`
// 都没实现 `PartialEq`，`derive` 不可用。比较策略：消息文本相同即视为相等。
impl PartialEq for AppError {
    fn eq(&self, other: &Self) -> bool {
        self.message() == other.message()
    }
}

/// 业务层标准 `Result` 别名。所有 service/dao 编排函数应返回 `AppResult<T>`。
pub type AppResult<T> = Result<T, AppError>;

// 构造函数 —— 比直接写 `AppError::Xxx(s.to_string())` 干净。
impl AppError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn http(msg: impl Into<String>) -> Self {
        Self::Http(msg.into())
    }

    pub fn parse(msg: impl Into<String>) -> Self {
        Self::Parse(msg.into())
    }

    pub fn db(msg: impl Into<String>) -> Self {
        Self::Db(msg.into())
    }

    pub fn js(msg: impl Into<String>) -> Self {
        Self::Js(msg.into())
    }

    pub fn business(msg: impl Into<String>) -> Self {
        Self::Business(msg.into())
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// IO 错误加业务前缀 —— 业务侧常要「读取文件失败: No such file」这种带前缀的字符串。
    pub fn io_msg(e: &std::io::Error, prefix: impl AsRef<str>) -> Self {
        Self::Internal(format!("{}: {e}", prefix.as_ref()))
    }

    /// 返回错误的消息文本 (不含结构化字段), 便于日志和 HTTP 响应。
    pub fn message(&self) -> String {
        match self {
            Self::Config(s)
            | Self::Http(s)
            | Self::Parse(s)
            | Self::Db(s)
            | Self::Js(s)
            | Self::Business(s)
            | Self::InvalidArgument(s)
            | Self::NotFound(s)
            | Self::Conflict(s)
            | Self::Internal(s) => s.clone(),
            Self::Export(e) => e.to_string(),
            Self::Io(e) => e.to_string(),
            Self::Json(e) => e.to_string(),
            Self::Toml(e) => e.to_string(),
        }
    }
}

// anyhow 反归一 —— main.rs / 测试 setup 用。**有损**：anyhow 的 chain context 全丢成字符串，
// 仅在 main.rs / 测试 setup / 跨 crate 边界用，业务层不推荐。
impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(format!("{e:#}"))
    }
}

// 领域错误归一 —— 让 `?` 跨域自动透传。`SearchError` 内含 Book/Toc/Chapter 子域错误，
// 统一归一为 `AppError::Internal`，由边界层决定怎么渲染 (toast / log / HTTP status)。
impl From<crate::parser::SearchError> for AppError {
    fn from(e: crate::parser::SearchError) -> Self {
        Self::Internal(format!("{e:#}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn from_io_error() {
        let e = io::Error::new(io::ErrorKind::NotFound, "missing");
        let a: AppError = e.into();
        assert!(matches!(a, AppError::Io(_)));
        assert!(a.message().contains("missing"));
    }

    #[test]
    fn from_export_error() {
        let e = crate::export::ExportError::EmptyChaptersDir(PathBuf::from("/x"));
        let a: AppError = e.into();
        assert!(matches!(a, AppError::Export(_)));
        assert!(a.message().contains("/x"));
    }

    #[test]
    fn constructors_produce_expected_variant() {
        assert!(matches!(AppError::config("a"), AppError::Config(_)));
        assert!(matches!(AppError::http("a"), AppError::Http(_)));
        assert!(matches!(AppError::parse("a"), AppError::Parse(_)));
        assert!(matches!(AppError::db("a"), AppError::Db(_)));
        assert!(matches!(AppError::js("a"), AppError::Js(_)));
        assert!(matches!(AppError::business("a"), AppError::Business(_)));
        assert!(matches!(
            AppError::invalid("a"),
            AppError::InvalidArgument(_)
        ));
        assert!(matches!(AppError::not_found("a"), AppError::NotFound(_)));
        assert!(matches!(AppError::conflict("a"), AppError::Conflict(_)));
        assert!(matches!(AppError::internal("a"), AppError::Internal(_)));
    }

    #[test]
    fn app_result_alias_works() {
        let ok: AppResult<u32> = Ok(42);
        // 已是 Ok, 这里只是断言 identity —— 免被 clippy 误判成可失败解包。
        assert!(matches!(ok, Ok(42)));

        let err: AppResult<u32> = Err(AppError::invalid("bad"));
        assert!(err.is_err());
    }
}
