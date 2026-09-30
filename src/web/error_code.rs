//! 错误码表
//!
//! 业务层错误的稳定编号 + i18n key 单点维护，翻译在 `locales/app.yml` 的
//! `WebErrors` 段。与 `web::WebErrorKind`（HTTP 状态码 + snake_case 短码）正交：
//! [`ErrorCode`] 回答「是什么错误」，`WebErrorKind` 回答「HTTP 怎么渲染」。
//! 编号段：1xxx 业务规则 / 2xxx 解析网络 / 3xxx 资源 / 4xxx 内部 / 5xxx 导出。
//! 数字码**稳定不变**，变更属 breaking change，需同步前端。

use std::fmt;

/// 业务层错误码。`ErrorCode` 1:1 对应 [`crate::web::error::WebError`] 的变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
#[allow(dead_code)] // 部分变体仅经 [`ErrorCode::code_str`] / `code` API 间接暴露
pub enum ErrorCode {
    // ─────────── 1xxx 业务规则 ───────────
    BookRuleMissing = 1001,
    MissingTitleOrAuthor = 1002,
    TocRuleMissing = 1003,
    ChapterRuleMissing = 1004,
    EmptyContent = 1005,
    SearchDisabled = 1006,
    SourceDisabled = 1007,
    EmptyToc = 1008,
    InvalidRange = 1009,
    Cancelled = 1010,

    // ─────────── 2xxx 解析/网络 ───────────
    BookHttp = 2001,
    BookCloudflare = 2002,
    BookParse = 2003,
    TocHttp = 2004,
    TocCloudflare = 2005,
    TocParse = 2006,
    ChapterHttp = 2007,
    ChapterCloudflare = 2008,
    ChapterParse = 2009,
    SearchHttp = 2010,
    SearchCloudflare = 2011,
    SearchParse = 2012,
    CrawlerClient = 2013,
    CrawlerIo = 2014,
    CrawlerExport = 2015,
    CrawlerBookAggregate = 2016,
    CrawlerTocAggregate = 2017,

    // ─────────── 3xxx 资源 ───────────
    NotFound = 3001,
    Conflict = 3002,
    BadRequest = 3003,
    DownloadPathEmpty = 3004,
    DownloadPathNotDir = 3005,
    TaskAlreadyFinished = 3006,

    // ─────────── 4xxx 内部 ───────────
    Internal = 4001,
    IoError = 4002,

    // ─────────── 5xxx 导出 ───────────
    ExportEmptyChaptersDir = 5001,
    ExportIo = 5002,
    ExportEpub = 5003,
    ExportZip = 5004,
    ExportEncoding = 5005,
    ExportPdf = 5006,
}

impl ErrorCode {
    /// 数字码 (e.g. `1001`)。稳定不变。
    #[allow(dead_code)] // public API —— 供外部 (桌面 / 第三方脚本) 取数字码
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// 字符串码 (e.g. `"1001"`)，给 HTTP `ErrorBody.code` / 日志检索用。
    pub const fn code_str(self) -> &'static str {
        match self {
            Self::BookRuleMissing => "1001",
            Self::MissingTitleOrAuthor => "1002",
            Self::TocRuleMissing => "1003",
            Self::ChapterRuleMissing => "1004",
            Self::EmptyContent => "1005",
            Self::SearchDisabled => "1006",
            Self::SourceDisabled => "1007",
            Self::EmptyToc => "1008",
            Self::InvalidRange => "1009",
            Self::Cancelled => "1010",
            Self::BookHttp => "2001",
            Self::BookCloudflare => "2002",
            Self::BookParse => "2003",
            Self::TocHttp => "2004",
            Self::TocCloudflare => "2005",
            Self::TocParse => "2006",
            Self::ChapterHttp => "2007",
            Self::ChapterCloudflare => "2008",
            Self::ChapterParse => "2009",
            Self::SearchHttp => "2010",
            Self::SearchCloudflare => "2011",
            Self::SearchParse => "2012",
            Self::CrawlerClient => "2013",
            Self::CrawlerIo => "2014",
            Self::CrawlerExport => "2015",
            Self::CrawlerBookAggregate => "2016",
            Self::CrawlerTocAggregate => "2017",
            Self::NotFound => "3001",
            Self::Conflict => "3002",
            Self::BadRequest => "3003",
            Self::DownloadPathEmpty => "3004",
            Self::DownloadPathNotDir => "3005",
            Self::TaskAlreadyFinished => "3006",
            Self::Internal => "4001",
            Self::IoError => "4002",
            Self::ExportEmptyChaptersDir => "5001",
            Self::ExportIo => "5002",
            Self::ExportEpub => "5003",
            Self::ExportZip => "5004",
            Self::ExportEncoding => "5005",
            Self::ExportPdf => "5006",
        }
    }

    /// i18n key (`"WebErrors.<variant 的 snake_case>"`)，翻译文本在
    /// `locales/app.yml` 的 `WebErrors` 段下。
    ///
    /// 加新变体必须同时加 `app.yml` 翻译 + 在 `i18n::tests::WEB_ERROR_KEYS` 注册，
    /// 否则 `web_errors_translated_in_all_three_locales` 测试 fail。
    pub const fn key(self) -> &'static str {
        match self {
            Self::BookRuleMissing => "WebErrors.book_rule_missing",
            Self::MissingTitleOrAuthor => "WebErrors.missing_title_or_author",
            Self::TocRuleMissing => "WebErrors.toc_rule_missing",
            Self::ChapterRuleMissing => "WebErrors.chapter_rule_missing",
            Self::EmptyContent => "WebErrors.empty_content",
            Self::SearchDisabled => "WebErrors.search_disabled",
            Self::SourceDisabled => "WebErrors.source_disabled",
            Self::EmptyToc => "WebErrors.empty_toc",
            Self::InvalidRange => "WebErrors.invalid_range",
            Self::Cancelled => "WebErrors.cancelled",
            Self::BookHttp => "WebErrors.book_http",
            Self::BookCloudflare => "WebErrors.book_cloudflare",
            Self::BookParse => "WebErrors.book_parse",
            Self::TocHttp => "WebErrors.toc_http",
            Self::TocCloudflare => "WebErrors.toc_cloudflare",
            Self::TocParse => "WebErrors.toc_parse",
            Self::ChapterHttp => "WebErrors.chapter_http",
            Self::ChapterCloudflare => "WebErrors.chapter_cloudflare",
            Self::ChapterParse => "WebErrors.chapter_parse",
            Self::SearchHttp => "WebErrors.search_http",
            Self::SearchCloudflare => "WebErrors.search_cloudflare",
            Self::SearchParse => "WebErrors.search_parse",
            Self::CrawlerClient => "WebErrors.crawler_client",
            Self::CrawlerIo => "WebErrors.crawler_io",
            Self::CrawlerExport => "WebErrors.crawler_export",
            Self::CrawlerBookAggregate => "WebErrors.crawler_book_aggregate",
            Self::CrawlerTocAggregate => "WebErrors.crawler_toc_aggregate",
            Self::NotFound => "WebErrors.not_found",
            Self::Conflict => "WebErrors.conflict",
            Self::BadRequest => "WebErrors.bad_request",
            Self::DownloadPathEmpty => "WebErrors.download_path_empty",
            Self::DownloadPathNotDir => "WebErrors.download_path_not_dir",
            Self::TaskAlreadyFinished => "WebErrors.task_already_finished",
            Self::Internal => "WebErrors.internal",
            Self::IoError => "WebErrors.io_error",
            Self::ExportEmptyChaptersDir => "WebErrors.export_empty_chapters_dir",
            Self::ExportIo => "WebErrors.export_io",
            Self::ExportEpub => "WebErrors.export_epub",
            Self::ExportZip => "WebErrors.export_zip",
            Self::ExportEncoding => "WebErrors.export_encoding",
            Self::ExportPdf => "WebErrors.export_pdf",
        }
    }

    /// Per-locale 翻译查找 —— 不读 / 不写 `rust_i18n::locale()` 全局 atomic，
    /// 并发请求互不干扰。Web handler 热路径专用（每次 yaml lookup + alloc）。
    pub fn message_for(self, locale: &str) -> String {
        crate::i18n::ts_for_locale(locale, self.key())
    }

    /// 全局 locale 翻译查找（`rust_i18n::locale()` 当前值）。
    ///
    /// **仅** `Display` / 测试 / 一次性日志用 —— web handler **必须**用
    /// `message_for(locale)`，否则并发请求之间 locale 互相踩。
    pub fn message(self) -> String {
        self.message_for(&rust_i18n::locale())
    }

    /// 数字码前缀段 (`1xxx`/`2xxx`/...)，错误大类归类用。
    #[allow(dead_code)] // public API —— 当前仅 Display 用，但保留供外部消费
    pub const fn category(self) -> ErrorCategory {
        match self.code() {
            1000..=1999 => ErrorCategory::Rule,
            2000..=2999 => ErrorCategory::Parse,
            3000..=3999 => ErrorCategory::Resource,
            5000..=5999 => ErrorCategory::Export,
            _ => ErrorCategory::Internal,
        }
    }
}

/// 错误大类 (由 `ErrorCode` 数字段派生)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)] // public API —— 供外部 (日志聚合 / 监控) 消费
pub enum ErrorCategory {
    /// 业务规则 (1xxx)。
    Rule,
    /// 解析/网络 (2xxx)。
    Parse,
    /// 资源 (3xxx)。
    Resource,
    /// 内部 (4xxx)。
    Internal,
    /// 导出 (5xxx)。
    Export,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display 没有 locale 上下文，全局 locale 是唯一选项。
        write!(f, "{}: {}", self.code_str(), self.message())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn code_str_matches_numeric_code() {
        // 防加新变体时改了数字忘改字符串（或反之）
        for variant in [
            ErrorCode::BookRuleMissing,
            ErrorCode::Cancelled,
            ErrorCode::BookHttp,
            ErrorCode::NotFound,
            ErrorCode::DownloadPathEmpty,
            ErrorCode::Internal,
            ErrorCode::ExportPdf,
        ] {
            assert_eq!(
                variant.code_str(),
                variant.code().to_string(),
                "code_str/code drift at {variant:?}"
            );
        }
    }

    #[test]
    fn category_matches_prefix() {
        assert_eq!(ErrorCode::BookRuleMissing.category(), ErrorCategory::Rule);
        assert_eq!(ErrorCode::BookHttp.category(), ErrorCategory::Parse);
        assert_eq!(ErrorCode::NotFound.category(), ErrorCategory::Resource);
        assert_eq!(ErrorCode::Internal.category(), ErrorCategory::Internal);
        assert_eq!(ErrorCode::ExportPdf.category(), ErrorCategory::Export);
        assert_eq!(
            ErrorCode::DownloadPathEmpty.category(),
            ErrorCategory::Resource
        );
        assert_eq!(
            ErrorCode::TaskAlreadyFinished.category(),
            ErrorCategory::Resource
        );
    }

    #[test]
    fn key_matches_translation_table() {
        // 关键不变量：`key()` 返回的 i18n key 在 3 locale 下都必须有非空翻译。
        for variant in [
            ErrorCode::BookRuleMissing,
            ErrorCode::MissingTitleOrAuthor,
            ErrorCode::Cancelled,
            ErrorCode::BookHttp,
            ErrorCode::NotFound,
            ErrorCode::DownloadPathEmpty,
            ErrorCode::DownloadPathNotDir,
            ErrorCode::TaskAlreadyFinished,
            ErrorCode::Internal,
            ErrorCode::IoError,
            ErrorCode::ExportPdf,
        ] {
            for locale in ["en", "zh-CN", "zh-TW"] {
                let msg = variant.message_for(locale);
                assert!(
                    !msg.is_empty() && msg != variant.key(),
                    "{variant:?} 在 locale={locale} 缺翻译：got {msg:?}"
                );
            }
        }
    }

    #[test]
    fn display_format_uses_global_locale() {
        rust_i18n::set_locale("zh-CN");
        let s = ErrorCode::BookRuleMissing.to_string();
        assert_eq!(s, "1001: 书源没有 book 规则");
        rust_i18n::set_locale("en");
    }
}
