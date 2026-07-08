//! Web API 统一错误类型。
//!
//! 解决 2 个问题：
//! 1. **内部错误细节泄漏**：原 handler 一律 `format!("{e:#}")` 把 anyhow / thiserror
//!    的完整 cause 链（包含内部路径、库函数名、堆栈）拼进 500 body 返回前端。
//!    攻击面 = 内网监听 / 公网部署时的栈 / 库版本指纹泄漏。
//! 2. **错误码不分类**：原 handler 一律 `INTERNAL_SERVER_ERROR`，
//!    前端无法区分"书源 URL 拼错"（4xx） vs "网络失败"（5xx） vs "CF 命中"（5xx）。
//!
//! 新协议（response body 形态）：
//! ```json
//! { "error": { "code": "<stable_id>", "message": "<short human CN/EN>" } }
//! ```
//!
//! 短码见 [`WebErrorKind::code`]，稳定不变（前端可以 switch 分支），message 是 i18n key
//! 或短中文，前端可选用 `t()` 翻译或直接展示。
//!
//! 用法（迁移现有 handler）：
//! ```ignore
//! // 旧
//! .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?;
//!
//! // 新
//! .map_err(WebError::from)?;
//! // → 编译期要求 handler 返回 Result<_, WebError>
//! ```

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::crawler::CrawlerError;
use crate::export::ExportError;
use crate::parser::{BookError, ChapterError, SearchError, TocError};

/// 业务层错误的稳定短码（前端 switch 用 + 日志搜调用栈用）。
///
/// 注意：短码变更属于 breaking change，发布前要同步前端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebErrorKind {
    /// 4xx：请求参数 / URL / 解析目标本身有问题（书源规则缺失、字段为空、选择器解析错）
    BadRequest,
    /// 4xx：书源未找到 / 任务未找到 / 文件不存在
    NotFound,
    /// 4xx：操作与当前状态冲突（任务已结束想取消）
    Conflict,
    /// 5xx：上游书源 HTTP 失败 / 网络断
    UpstreamUnavailable,
    /// 5xx：上游命中 Cloudflare 且未配 bypass
    Cloudflare,
    /// 5xx：其他未分类服务端错误（解析失败 / 选择器错 / JS 失败 / 导出失败 / IO）
    Internal,
}

/// Web API 错误包装。所有业务 handler 统一返 `Result<_, WebError>`。
///
/// 设计：
/// - 业务错误（BookError / `TocError` / 等）→ 对应分类
/// - `std::io::Error` → Internal + 简短 "`io_error`" message
/// - 锁 poison / SSE 内部 stream 错误**不**走这里（lock.rs 维持 `(StatusCode, String)`,
///   那是不同语义:网络层 vs 业务层）
#[allow(dead_code)] // Conflict / BadRequest 留作未来 task_cancel / settings_put 业务流用
#[derive(Debug)]
pub enum WebError {
    /// 业务解析失败（书名/作者为空、规则缺失、选择器错等）
    Book(BookError),
    /// TOC 解析失败
    Toc(TocError),
    /// 章节抓取失败
    Chapter(ChapterError),
    /// 搜索失败
    Search(SearchError),
    /// 爬虫编排失败（Book + Toc + Chapter + Export + IO 聚合）
    Crawler(CrawlerError),
    /// 导出失败
    Export(ExportError),
    /// 显式 not found（书源/任务/文件）
    NotFound(&'static str),
    /// 显式 conflict
    Conflict(&'static str),
    /// 显式 bad request
    BadRequest(&'static str),
    /// 显式内部错误（catch-all，message 不含内部 cause）
    Internal(&'static str),
}

impl WebErrorKind {
    /// `短码（snake_case`，**稳定**）。
    pub const fn code(self) -> &'static str {
        match self {
            Self::BadRequest => "bad_request",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::UpstreamUnavailable => "upstream_unavailable",
            Self::Cloudflare => "cloudflare_challenge",
            Self::Internal => "internal_error",
        }
    }

    /// HTTP 状态码。
    pub const fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::UpstreamUnavailable => StatusCode::BAD_GATEWAY,
            Self::Cloudflare => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl WebError {
    /// 错误码 (数字, e.g. `1001`)。走 [`crate::constant::error_code::ErrorCode`]
    /// 单点维护, 不在此处硬编码。
    pub const fn code(&self) -> crate::constant::error_code::ErrorCode {
        use crate::constant::error_code::ErrorCode;
        match self {
            // BookError
            Self::Book(BookError::BookRuleMissing) => ErrorCode::BookRuleMissing,
            Self::Book(BookError::MissingTitleOrAuthor) => ErrorCode::MissingTitleOrAuthor,
            Self::Book(BookError::Http(_)) => ErrorCode::BookHttp,
            Self::Book(BookError::Cloudflare(_)) => ErrorCode::BookCloudflare,
            Self::Book(BookError::Parse(_) | BookError::Selector(_)) => ErrorCode::BookParse,

            // TocError
            Self::Toc(TocError::TocRuleMissing) => ErrorCode::TocRuleMissing,
            Self::Toc(TocError::Http(_)) => ErrorCode::TocHttp,
            Self::Toc(TocError::Cloudflare(_)) => ErrorCode::TocCloudflare,
            Self::Toc(TocError::Parse(_) | TocError::Selector(_)) => ErrorCode::TocParse,

            // ChapterError
            Self::Chapter(ChapterError::ChapterRuleMissing) => ErrorCode::ChapterRuleMissing,
            Self::Chapter(ChapterError::Http(_)) => ErrorCode::ChapterHttp,
            Self::Chapter(ChapterError::Cloudflare(_)) => ErrorCode::ChapterCloudflare,
            Self::Chapter(ChapterError::EmptyContent(_)) => ErrorCode::EmptyContent,
            Self::Chapter(ChapterError::Parse(_) | ChapterError::Selector(_)) => {
                ErrorCode::ChapterParse
            }

            // SearchError
            Self::Search(SearchError::SearchDisabled) => ErrorCode::SearchDisabled,
            Self::Search(SearchError::SourceDisabled) => ErrorCode::SourceDisabled,
            Self::Search(SearchError::Http(_)) => ErrorCode::SearchHttp,
            Self::Search(SearchError::Cloudflare(_)) => ErrorCode::SearchCloudflare,
            Self::Search(SearchError::Parse(_) | SearchError::Selector(_)) => {
                ErrorCode::SearchParse
            }

            // CrawlerError
            Self::Crawler(CrawlerError::EmptyToc) => ErrorCode::EmptyToc,
            Self::Crawler(CrawlerError::Client(_)) => ErrorCode::CrawlerClient,
            Self::Crawler(CrawlerError::Io(_)) => ErrorCode::CrawlerIo,
            Self::Crawler(CrawlerError::Export(_)) => ErrorCode::CrawlerExport,
            Self::Crawler(CrawlerError::Cancelled) => ErrorCode::Cancelled,
            Self::Crawler(CrawlerError::InvalidRange(_)) => ErrorCode::InvalidRange,
            Self::Crawler(CrawlerError::Book(_)) => ErrorCode::CrawlerBookAggregate,
            Self::Crawler(CrawlerError::Toc(_)) => ErrorCode::CrawlerTocAggregate,

            // ExportError
            Self::Export(ExportError::EmptyChaptersDir(_)) => ErrorCode::ExportEmptyChaptersDir,
            Self::Export(ExportError::Io(_)) => ErrorCode::ExportIo,
            Self::Export(ExportError::Epub(_)) => ErrorCode::ExportEpub,
            Self::Export(ExportError::Zip(_)) => ErrorCode::ExportZip,
            Self::Export(ExportError::Encoding(_)) => ErrorCode::ExportEncoding,
            Self::Export(ExportError::Pdf(_)) => ErrorCode::ExportPdf,

            // 显式 (WebError 自带的 4 类)
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::Conflict(_) => ErrorCode::Conflict,
            Self::BadRequest(_) => ErrorCode::BadRequest,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }

    /// 暴露的 message（**不含**内部 cause / 库错误细节）。
    /// 文案走 [`crate::constant::error_code::ErrorCode::message`] 单点维护。
    ///
    /// 例外: `NotFound/Conflict/BadRequest/Internal` 4 个显式变体接受调用方传
    /// 入的动态消息 (e.g. `"task_id=42 not found"`), 不进 `ErrorCode` 表。
    pub const fn message(&self) -> &'static str {
        match self {
            Self::NotFound(msg)
            | Self::Conflict(msg)
            | Self::BadRequest(msg)
            | Self::Internal(msg) => msg,
            _ => self.code().message(),
        }
    }

    /// 内部 `tracing::warn!` 用的详细 cause（**不**进 response body）。
    pub fn internal_cause(&self) -> String {
        match self {
            Self::Book(e) => format!("{e:#}"),
            Self::Toc(e) => format!("{e:#}"),
            Self::Chapter(e) => format!("{e:#}"),
            Self::Search(e) => format!("{e:#}"),
            Self::Crawler(e) => format!("{e:#}"),
            Self::Export(e) => format!("{e:#}"),
            Self::NotFound(_) | Self::Conflict(_) | Self::BadRequest(_) | Self::Internal(_) => {
                String::new()
            }
        }
    }
}

impl WebError {
    /// 把 `WebError` 分类到 HTTP 错误类型。
    pub const fn classify(&self) -> WebErrorKind {
        match self {
            Self::Book(BookError::BookRuleMissing | BookError::MissingTitleOrAuthor)
            | Self::Toc(TocError::TocRuleMissing)
            | Self::Chapter(ChapterError::ChapterRuleMissing | ChapterError::EmptyContent(_))
            | Self::Search(SearchError::SearchDisabled)
            | Self::Crawler(CrawlerError::InvalidRange(_))
            | Self::BadRequest(_) => WebErrorKind::BadRequest,
            Self::Book(BookError::Http(_))
            | Self::Toc(TocError::Http(_))
            | Self::Chapter(ChapterError::Http(_))
            | Self::Search(SearchError::Http(_)) => WebErrorKind::UpstreamUnavailable,
            Self::Book(BookError::Cloudflare(_))
            | Self::Toc(TocError::Cloudflare(_))
            | Self::Chapter(ChapterError::Cloudflare(_))
            | Self::Search(SearchError::Cloudflare(_)) => WebErrorKind::Cloudflare,
            Self::NotFound(_) => WebErrorKind::NotFound,
            Self::Conflict(_) => WebErrorKind::Conflict,
            _ => WebErrorKind::Internal,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let kind = self.classify();
        let status = kind.status();
        let code = kind.code();
        let message = self.message();
        let body = ErrorEnvelope {
            error: ErrorBody { code, message },
        };

        // 业务层错误（Book/Toc/...）走 warn 级（用户操作触发，但 5xx 时运维要看到）；
        // 4xx 走 info 级（用户/前端误用，不污染 warn 流）。
        if status.is_server_error() {
            tracing::warn!(
                code = code,
                cause = self.internal_cause().as_str(),
                "web API server error"
            );
        } else {
            tracing::info!(code = code, message = message, "web API client error");
        }
        (status, Json(body)).into_response()
    }
}

// ── From 转换：让 `?` 自动装箱 ──────────────────────────

impl From<BookError> for WebError {
    fn from(e: BookError) -> Self {
        Self::Book(e)
    }
}
impl From<TocError> for WebError {
    fn from(e: TocError) -> Self {
        Self::Toc(e)
    }
}
impl From<ChapterError> for WebError {
    fn from(e: ChapterError) -> Self {
        Self::Chapter(e)
    }
}
impl From<SearchError> for WebError {
    fn from(e: SearchError) -> Self {
        Self::Search(e)
    }
}
impl From<CrawlerError> for WebError {
    fn from(e: CrawlerError) -> Self {
        Self::Crawler(e)
    }
}
impl From<ExportError> for WebError {
    fn from(e: ExportError) -> Self {
        Self::Export(e)
    }
}
impl From<std::io::Error> for WebError {
    fn from(e: std::io::Error) -> Self {
        // 内部 io 错误不暴露路径（路径里可能有用户名等），只留类型标签
        tracing::warn!("web API io error: {e:#}");
        Self::Internal("io_error")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn classify_maps_book_rule_missing_to_400() {
        let err = WebError::Book(BookError::BookRuleMissing);
        assert_eq!(err.classify(), WebErrorKind::BadRequest);
    }

    #[test]
    fn classify_maps_http_to_502() {
        let err = WebError::Book(BookError::Http("net".into()));
        assert_eq!(err.classify(), WebErrorKind::UpstreamUnavailable);
    }

    #[test]
    fn classify_maps_cloudflare_to_503() {
        let err = WebError::Toc(TocError::Cloudflare("url".into()));
        assert_eq!(err.classify(), WebErrorKind::Cloudflare);
    }

    #[test]
    fn classify_maps_not_found_variant() {
        let err = WebError::NotFound("书源未找到");
        assert_eq!(err.classify(), WebErrorKind::NotFound);
    }

    #[test]
    fn message_does_not_leak_internal_cause() {
        // 构造一个含敏感路径的 cause，验证 message 不含它
        let err = WebError::Book(BookError::Parse(
            "C:\\Users\\admin\\secrets\\config.json".into(),
        ));
        let msg = err.message();
        assert!(!msg.contains("admin"), "message leaked path: {msg}");
        assert!(!msg.contains("C:\\"), "message leaked path: {msg}");
        assert_eq!(msg, "详情页 HTML 解析失败");
    }

    #[test]
    fn into_response_uses_correct_status() {
        let err = WebError::NotFound("任务未找到");
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
