//! 全局错误根类型 (PR #2, 2026-07-08)
//!
//! # 设计原则
//!
//! 1. **领域错误保留**: 各业务域 (`ExportError`、`WebError`、`BookError`/...) 继续
//!    在自己的模块里定义, 不强行合并。它们的变体携带具体业务上下文 (e.g.
//!    `ExportError::EmptyChaptersDir(PathBuf)`), 抹平会丢信息。
//!
//! 2. **统一归一**: 业务层编排函数返回 [`AppResult<T>`] (即 [`Result<T, AppError>`])。
//!    领域错误通过 `From` 自动归一, 调用方一个 `?` 就能传透。
//!
//! 3. **边界翻译**: 进程入口 (`main.rs`、CLI 顶层、HTTP handler) 仍可保留或转回
//!    自己的边界错误类型 (e.g. `WebError` 的 `IntoResponse` impl), 不强制
//!    全部用 `AppError` 渲染给用户。
//!
//! # 迁移策略
//!
//! - 本 PR (#2): 新建 `AppError` + 公共 `From` (原始错误 + `ExportError`)。
//! - PR #7~#9: 逐模块把 `Result<T, String>` / 散乱 `anyhow::Result` 改成
//!   `AppResult<T>`。每批一个模块, 编译期 + 测试期双重验证。
//! - main.rs / 测试 setup 保留 `anyhow::Result`, 不强求替换 —
//!   `AppError: From<anyhow::Error>` 已支持从 anyhow 反归一。
//!
//! # 错误码语义
//!
//! - `Config` / `Http` / `Parse` / `Export` / `Db`: 子领域错误, 携带 `#[from]` 链路
//! - `Business` / `InvalidArgument` / `NotFound` / `Conflict`: 业务侧明确错误
//! - `Internal`: 兜底, 应该是真正的"不该发生"场景, 出现需排查
//! - `Js`: boa 引擎错误, 业务上属"书源规则运行失败"
//!
//! # 错误日志分级
//!
//! 配合 `tracing`: 业务层编排函数遇到错误时, 用 `?` 向上传; 边界函数决定
//! 是否 `tracing::error!` (持久性错误) 或 `tracing::warn!` (可恢复错误)。
//! `AppError` 不自动打日志 — 那是决策不是机械动作。

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

/// 业务层标准 `Result` 别名。所有 service/dao 编排函数应返回 `AppResult<T>`。
pub type AppResult<T> = Result<T, AppError>;

// -------------------------------------------------------------------------------------
// 构造函数 — 比直接写 `AppError::Xxx(s.to_string())` 干净
// -------------------------------------------------------------------------------------
impl AppError {
    /// 构造 `Config` 错误。
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// 构造 `Http` 错误。
    pub fn http(msg: impl Into<String>) -> Self {
        Self::Http(msg.into())
    }

    /// 构造 `Parse` 错误。
    pub fn parse(msg: impl Into<String>) -> Self {
        Self::Parse(msg.into())
    }

    /// 构造 `Db` 错误。
    pub fn db(msg: impl Into<String>) -> Self {
        Self::Db(msg.into())
    }

    /// 构造 `Js` 错误。
    pub fn js(msg: impl Into<String>) -> Self {
        Self::Js(msg.into())
    }

    /// 构造 `Business` 错误。
    pub fn business(msg: impl Into<String>) -> Self {
        Self::Business(msg.into())
    }

    /// 构造 `InvalidArgument` 错误。
    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::InvalidArgument(msg.into())
    }

    /// 构造 `NotFound` 错误。
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    /// 构造 `Conflict` 错误。
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::Conflict(msg.into())
    }

    /// 构造 `Internal` 错误。
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
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

// -------------------------------------------------------------------------------------
// anyhow 反归一 — main.rs / 测试 setup 用, `?` 一步到位
// -------------------------------------------------------------------------------------
//
// 注意: 这个 From 是 **有损** 的 — anyhow 的 chain context 全部丢成字符串。
// 仅在 main.rs / 测试 setup / 跨 crate 边界用, 业务层不推荐。
impl From<anyhow::Error> for AppError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(format!("{e:#}"))
    }
}

// -------------------------------------------------------------------------------------
// 单元测试
// -------------------------------------------------------------------------------------
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
        assert!(matches!(AppError::invalid("a"), AppError::InvalidArgument(_)));
        assert!(matches!(AppError::not_found("a"), AppError::NotFound(_)));
        assert!(matches!(AppError::conflict("a"), AppError::Conflict(_)));
        assert!(matches!(AppError::internal("a"), AppError::Internal(_)));
    }

    #[test]
    fn app_result_alias_works() {
        let ok: AppResult<u32> = Ok(42);
        assert_eq!(ok.unwrap(), 42);

        let err: AppResult<u32> = Err(AppError::invalid("bad"));
        assert!(err.is_err());
    }
}
