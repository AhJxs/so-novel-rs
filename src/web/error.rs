//! Web API 统一错误类型。
//!
//! 只暴露稳定短码 + 按请求 locale 翻译的 message，**绝不**把 anyhow/thiserror
//! 的 cause 链（内部路径、库名、堆栈）拼进 response body。短码变更属 breaking
//! change；handler 返 `Result<_, WebError>`（`?` 自动装箱），渲染走
//! `into_response_for_locale(locale)` 拿 per-request 翻译。

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
    /// 4xx：请求参数 / URL / 解析目标本身有问题
    BadRequest,
    /// 4xx：书源未找到 / 任务未找到 / 文件不存在
    NotFound,
    /// 4xx：操作与当前状态冲突（任务已结束想取消）
    Conflict,
    /// 5xx：上游书源 HTTP 失败 / 网络断
    UpstreamUnavailable,
    /// 5xx：上游命中 Cloudflare 且未配 bypass
    Cloudflare,
    /// 5xx：其他未分类服务端错误（解析 / JS / 导出 / IO）
    Internal,
}

/// Web API 错误包装。所有业务 handler 统一返 `Result<_, WebError>`。
///
/// 业务错误（BookError / `TocError` / 等）→ 对应分类；`std::io::Error` → Internal。
/// 锁 poison / SSE 内部 stream 错误**不**走这里（lock.rs 保持 `(StatusCode, String)`，
/// 那是网络层语义，与业务层不同）。
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
    /// 显式 not found（书源/任务/文件）。内部字符串**忽略**，避免泄漏内部 id / 路径。
    NotFound(&'static str),
    /// 显式 conflict。内部字符串**忽略**，统一翻译成 `WebErrors.conflict`。
    Conflict(&'static str),
    /// 显式 bad request。内部字符串**忽略**，统一翻译成 `WebErrors.bad_request`。
    BadRequest(&'static str),
    /// 显式内部错误（catch-all，message 不含内部 cause）。内部字符串**忽略**。
    Internal(&'static str),
    /// settings PUT: `download_path` 是空串 → 400。
    DownloadPathEmpty,
    /// settings PUT: `download_path` 不是已存在目录 → 400。
    DownloadPathNotDir,
    /// `task_cancel`: 任务已结束 → 409。
    TaskAlreadyFinished,
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
    /// 错误码 (数字, e.g. `1001`)。走 [`super::error_code::ErrorCode`]
    /// 单点维护，不在此处硬编码。
    pub const fn code(&self) -> super::error_code::ErrorCode {
        use super::error_code::ErrorCode;
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

            // 显式 (WebError 自带的 4 类 + 3 新增具体子类型)
            Self::NotFound(_) => ErrorCode::NotFound,
            Self::Conflict(_) => ErrorCode::Conflict,
            Self::BadRequest(_) => ErrorCode::BadRequest,
            Self::Internal(_) => ErrorCode::Internal,
            Self::DownloadPathEmpty => ErrorCode::DownloadPathEmpty,
            Self::DownloadPathNotDir => ErrorCode::DownloadPathNotDir,
            Self::TaskAlreadyFinished => ErrorCode::TaskAlreadyFinished,
        }
    }

    /// 暴露的 message（**不含**内部 cause / 库错误细节）。
    ///
    /// 用全局 locale，仅 `IntoResponse` / 测试场景用；web handler **必须**走
    /// [`Self::into_response_for_locale`] 拿 per-request 翻译，避免并发互相踩。
    #[allow(dead_code)] // public API + `IntoResponse` 间接使用，clippy 检测不到
    pub fn message(&self) -> String {
        self.code().message()
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
            Self::NotFound(_)
            | Self::Conflict(_)
            | Self::BadRequest(_)
            | Self::Internal(_)
            | Self::DownloadPathEmpty
            | Self::DownloadPathNotDir
            | Self::TaskAlreadyFinished => String::new(),
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
            | Self::BadRequest(_)
            | Self::DownloadPathEmpty
            | Self::DownloadPathNotDir => WebErrorKind::BadRequest,
            Self::Book(BookError::Http(_))
            | Self::Toc(TocError::Http(_))
            | Self::Chapter(ChapterError::Http(_))
            | Self::Search(SearchError::Http(_)) => WebErrorKind::UpstreamUnavailable,
            Self::Book(BookError::Cloudflare(_))
            | Self::Toc(TocError::Cloudflare(_))
            | Self::Chapter(ChapterError::Cloudflare(_))
            | Self::Search(SearchError::Cloudflare(_)) => WebErrorKind::Cloudflare,
            Self::NotFound(_) => WebErrorKind::NotFound,
            Self::Conflict(_) | Self::TaskAlreadyFinished => WebErrorKind::Conflict,
            _ => WebErrorKind::Internal,
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    /// `WebErrorKind` snake_case 短码。前端 dispatch **不要**用这个 —— 多个
    /// variant 共用同一 kind，无法细分；用 `code_id`。
    code: &'static str,
    /// 业务层稳定数字码（`3004` / `3005` / ...）。前端按这个 dispatch，
    /// 加新 variant 必须同步前端。
    code_id: &'static str,
    message: String,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

impl WebError {
    /// 把 `WebError` 渲染成 axum `Response`，**使用指定 locale** 翻译 `message`。
    ///
    /// web handler 的规范路径：locale 从 `Locale` extractor 拿，避免并发请求
    /// 之间全局 locale 互相踩。`IntoResponse` 转调这里，传全局 locale。
    pub fn into_response_for_locale(self, locale: &str) -> Response {
        let kind = self.classify();
        let status = kind.status();
        let code = kind.code();
        let message = self.code().message_for(locale);
        let body = ErrorEnvelope {
            error: ErrorBody {
                code,
                code_id: self.code().code_str(),
                message,
            },
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
            tracing::info!(
                code = code,
                message = body.error.message.as_str(),
                "web API client error"
            );
        }
        (status, Json(body)).into_response()
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        // 全局 locale —— 不带 Locale extractor 的 handler / 测试场景用。
        // 生产 web handler 走 `into_response_for_locale(locale)` 拿 per-request 翻译。
        let locale = (*rust_i18n::locale()).to_string();
        self.into_response_for_locale(&locale)
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
        // 内部 io 错误不暴露路径（可能含用户名），只留类型标签
        tracing::warn!("web API io error: {e:#}");
        Self::Internal("io_error")
    }
}

// ── 锁 / 通用 String→WebError blanket impl ────────────────────
//
// 让 `?` 自动把锁毒化（`rw_read_or` 返回 `Result<_, String>`）装箱到
// `WebError::Internal`：响应体只给稳定短码 `"internal_error"`，动态消息进日志。
// 锁毒化是 500（服务端状态损坏），**不能**静默转成 404。

impl From<String> for WebError {
    fn from(s: String) -> Self {
        tracing::warn!("web API internal/lock error: {s}");
        Self::Internal("internal_error")
    }
}

impl From<&str> for WebError {
    fn from(s: &str) -> Self {
        Self::from(s.to_string())
    }
}

// ── 锁 / 内部错误统一收纳 ──────────────────────────────
//
// handler 入口常要拿 1-3 个共享状态锁，逐句 `.map_err(..)` 重复且易漏。
// `label` 与 `rw_read_or` 内部日志一一对应（一个 helper 一个 label，便于 grep）。

/// 非-SSE handler 专用：拿锁 / 读共享状态，失败 → `WebError::Internal("internal_error")`。
///
/// 闭包返回 `Result<T, String>`，与 `rw_read_or` / `mutex_or` 直接对接，`?` 即可：
/// 失败记 warn 并返回 `WebError::Internal("internal_error")`（500 + 稳定 envelope）。
pub fn read_state_or_json<T, F>(label: &str, f: F) -> Result<T, WebError>
where
    F: FnOnce() -> Result<T, String>,
{
    match f() {
        Ok(v) => Ok(v),
        Err(msg) => {
            tracing::warn!("web handler {label} state read failed: {msg}");
            Err(WebError::from(msg))
        }
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
    fn classify_maps_task_already_finished_to_409() {
        let err = WebError::TaskAlreadyFinished;
        assert_eq!(err.classify(), WebErrorKind::Conflict);
    }

    #[test]
    fn classify_maps_download_path_empty_to_400() {
        let err = WebError::DownloadPathEmpty;
        assert_eq!(err.classify(), WebErrorKind::BadRequest);
    }

    #[test]
    fn classify_maps_download_path_not_dir_to_400() {
        let err = WebError::DownloadPathNotDir;
        assert_eq!(err.classify(), WebErrorKind::BadRequest);
    }

    #[test]
    fn message_does_not_leak_internal_cause() {
        let err = WebError::Book(BookError::Parse(
            "C:\\Users\\admin\\secrets\\config.json".into(),
        ));
        let msg = err.message();
        assert!(!msg.contains("admin"), "message leaked path: {msg}");
        assert!(!msg.contains("C:\\"), "message leaked path: {msg}");
        // 全局 locale 默认 en → 英文翻译
        assert_eq!(msg, "Book detail HTML parse failed");
    }

    #[test]
    fn message_ignores_internal_string_of_explicit_variants() {
        let err = WebError::NotFound("任务 id=42 私有路径 C:\\foo");
        let msg = err.code().message_for("zh-CN");
        assert!(!msg.contains("C:\\"), "message leaked path: {msg}");
        assert!(!msg.contains("42"), "message leaked id: {msg}");
        assert_eq!(msg, "资源未找到");
    }

    #[test]
    fn into_response_uses_correct_status() {
        let err = WebError::NotFound("任务未找到");
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn into_response_for_locale_uses_given_locale() {
        // 不变量：per-locale 翻译不读全局 locale
        rust_i18n::set_locale("en"); // 全局是 en
        let err = WebError::Book(BookError::BookRuleMissing);
        let resp = err.into_response_for_locale("zh-CN");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        rust_i18n::set_locale("en");
    }

    #[test]
    fn from_string_maps_to_internal_with_stable_label() {
        // 动态消息 (e.g. 锁毒化) → label 稳定，敏感信息只进日志。
        let err: WebError = String::from("rwlock 'web:config' poisoned at byte 42").into();
        assert_eq!(err.classify(), WebErrorKind::Internal);
        assert!(matches!(err, WebError::Internal("internal_error")));
        // 全局 locale 默认 en → 英文翻译
        assert_eq!(err.message(), "Internal server error");
    }

    #[test]
    fn from_str_works_same_as_string() {
        let err: WebError = "lock poison".into();
        assert_eq!(err.classify(), WebErrorKind::Internal);
        assert!(matches!(err, WebError::Internal("internal_error")));
    }

    #[test]
    fn from_string_via_question_mark_operator() {
        let result: Result<i32, String> = Err("poisoned lock at web:tasks".to_string());
        let web: Result<i32, WebError> = result.map_err(WebError::from);
        assert!(web.is_err());
        let err = web.unwrap_err();
        assert_eq!(err.classify(), WebErrorKind::Internal);
    }

    #[test]
    fn from_string_response_status_is_500() {
        let err: WebError = String::from("any internal msg").into();
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn into_response_for_locale_translates_per_request() {
        // 全局切到 en，per-locale 调用翻译成 zh-CN —— 证明不走全局
        rust_i18n::set_locale("en");
        let err = WebError::NotFound("ignored internal string");

        let resp_zh = err.into_response_for_locale("zh-CN");
        let body_zh = response_body_string(resp_zh);
        assert!(
            body_zh.contains("资源未找到"),
            "zh-CN body should contain 资源未找到: {body_zh}"
        );

        let resp_en = WebError::NotFound("ignored").into_response_for_locale("en");
        let body_en = response_body_string(resp_en);
        assert!(
            body_en.contains("Resource not found"),
            "en body should contain Resource not found: {body_en}"
        );

        let resp_tw = WebError::NotFound("ignored").into_response_for_locale("zh-TW");
        let body_tw = response_body_string(resp_tw);
        assert!(
            body_tw.contains("資源未找到"),
            "zh-TW body should contain 資源未找到: {body_tw}"
        );

        rust_i18n::set_locale("en");
    }

    #[test]
    fn new_variants_translate_correctly() {
        // 显式传 locale —— 不依赖全局 atomic（其它测试可能并行改全局 locale）
        let empty = WebError::DownloadPathEmpty;
        assert_eq!(
            empty.code().message_for("en"),
            "Download path cannot be empty"
        );
        let not_dir = WebError::DownloadPathNotDir;
        assert_eq!(
            not_dir.code().message_for("en"),
            "Download path is not an existing directory"
        );
        let finished = WebError::TaskAlreadyFinished;
        assert_eq!(
            finished.code().message_for("en"),
            "Task has already finished; cannot cancel"
        );
    }

    /// 抽 response body 成 String 用于断言 JSON 内容。
    fn response_body_string(resp: Response) -> String {
        use axum::body::to_bytes;
        let body = resp.into_body();
        let rt = tokio::runtime::Runtime::new().expect("rt");
        rt.block_on(async {
            let bytes = to_bytes(body, 4096).await.expect("body bytes");
            String::from_utf8(bytes.to_vec()).expect("utf8")
        })
    }
}
