//! 错误码表
//!
//! # 设计
//!
//! 业务层错误的稳定编号 + 中文短消息, 单点维护。原本散落在
//! `web::WebError::message()` 的 60+ 个 `match` 字符串, 现在统一进这张表。
//!
//! 与 `web::WebErrorKind` 是**正交**关系:
//!
//! | 维度 | 类型 | 用途 |
//! |---|---|---|
//! | 是什么错误 | [`ErrorCode`] (数字) | 业务侧唯一标识, 日志检索, 前端按码分流 |
//! | HTTP 怎么渲染 | `WebErrorKind` (枚举) | HTTP 状态码 + `snake_case` 短码 |
//!
//! # 编号规则
//!
//! - `1xxx` — 业务规则 (规则缺失/字段为空/书源禁用/任务取消)
//! - `2xxx` — 解析/网络 (HTTP/CF/解析失败/IO 聚合)
//! - `3xxx` — 资源 (NotFound/Conflict/BadRequest)
//! - `4xxx` — 内部 (Internal/IoError 兜底)
//! - `5xxx` — 导出 (EPUB/PDF/ZIP/编码)
//!
//! 编号**稳定不变** — 短码变更属 breaking change, 需同步前端。

use std::fmt;

/// 业务层错误码。`ErrorCode` 1:1 对应 [`crate::web::error::WebError`] 的变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ErrorCode {
    // ─────────── 1xxx 业务规则 ───────────
    /// 1001: 详情页书源没有 `book` 规则。
    BookRuleMissing = 1001,
    /// 1002: 详情页书名或作者解析为空。
    MissingTitleOrAuthor = 1002,
    /// 1003: 目录页书源没有 `toc` 规则。
    TocRuleMissing = 1003,
    /// 1004: 章节页书源没有 `chapter` 规则。
    ChapterRuleMissing = 1004,
    /// 1005: 章节正文为空 (被规则过滤掉了或源页就是空)。
    EmptyContent = 1005,
    /// 1006: 书源未启用搜索功能。
    SearchDisabled = 1006,
    /// 1007: 书源已被用户禁用。
    SourceDisabled = 1007,
    /// 1008: 目录解析后 0 章。
    EmptyToc = 1008,
    /// 1009: 章节范围非法 (起始 > 结束 或超出总章节数)。
    InvalidRange = 1009,
    /// 1010: 任务被用户取消。
    Cancelled = 1010,

    // ─────────── 2xxx 解析/网络 ───────────
    /// 2001: 详情页 HTTP 请求失败。
    BookHttp = 2001,
    /// 2002: 详情页命中 Cloudflare 验证。
    BookCloudflare = 2002,
    /// 2003: 详情页 HTML 解析失败。
    BookParse = 2003,
    /// 2004: 目录页 HTTP 请求失败。
    TocHttp = 2004,
    /// 2005: 目录页命中 Cloudflare 验证。
    TocCloudflare = 2005,
    /// 2006: 目录页 HTML 解析失败。
    TocParse = 2006,
    /// 2007: 章节页 HTTP 请求失败。
    ChapterHttp = 2007,
    /// 2008: 章节页命中 Cloudflare 验证。
    ChapterCloudflare = 2008,
    /// 2009: 章节页 HTML 解析失败。
    ChapterParse = 2009,
    /// 2010: 搜索 HTTP 请求失败。
    SearchHttp = 2010,
    /// 2011: 搜索页命中 Cloudflare 验证。
    SearchCloudflare = 2011,
    /// 2012: 搜索结果 HTML 解析失败。
    SearchParse = 2012,
    /// 2013: HTTP 客户端构造失败。
    CrawlerClient = 2013,
    /// 2014: 任务文件 IO 失败。
    CrawlerIo = 2014,
    /// 2015: 导出失败 (聚合自 `ExportError`)。
    CrawlerExport = 2015,
    /// 2016: 书源解析失败 (聚合自 `BookError`, 嵌套源不展开)。
    CrawlerBookAggregate = 2016,
    /// 2017: 目录解析失败 (聚合自 `TocError`, 嵌套源不展开)。
    CrawlerTocAggregate = 2017,

    // ─────────── 3xxx 资源 ───────────
    /// 3001: 资源未找到 (书源/任务/文件)。
    NotFound = 3001,
    /// 3002: 资源状态冲突 (重复添加/操作与状态不符)。
    Conflict = 3002,
    /// 3003: 请求参数错误。
    BadRequest = 3003,

    // ─────────── 4xxx 内部 ───────────
    /// 4001: 内部错误, 不应发生。
    Internal = 4001,
    /// 4002: IO 错误兜底 (避免泄漏内部路径)。
    IoError = 4002,

    // ─────────── 5xxx 导出 ───────────
    /// 5001: 章节缓存目录为空。
    ExportEmptyChaptersDir = 5001,
    /// 5002: 导出文件 IO 失败。
    ExportIo = 5002,
    /// 5003: EPUB 生成失败。
    ExportEpub = 5003,
    /// 5004: ZIP 打包失败。
    ExportZip = 5004,
    /// 5005: 编码转换失败。
    ExportEncoding = 5005,
    /// 5006: PDF 生成失败。
    ExportPdf = 5006,
}

impl ErrorCode {
    /// 数字码 (e.g. `1001`)。稳定不变。
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// 字符串码 (e.g. `"1001"`), 给 HTTP `ErrorBody.code` / 日志检索用。
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

    /// 中文短消息 (HTTP `ErrorBody.message` 用)。
    ///
    /// 长度 ≤ 30 字, 不可变长 (要进 4xx body, 防止被滥用为信息泄漏通道)。
    pub const fn message(self) -> &'static str {
        match self {
            Self::BookRuleMissing => "书源没有 book 规则",
            Self::MissingTitleOrAuthor => "详情页书名或作者为空",
            Self::TocRuleMissing => "书源没有 toc 规则",
            Self::ChapterRuleMissing => "书源没有 chapter 规则",
            Self::EmptyContent => "章节正文为空",
            Self::SearchDisabled => "书源未启用搜索",
            Self::SourceDisabled => "书源已被禁用",
            Self::EmptyToc => "目录返回 0 章",
            Self::InvalidRange => "章节范围非法",
            Self::Cancelled => "任务已取消",
            Self::BookHttp => "书源 HTTP 请求失败",
            Self::BookCloudflare => "书源命中 Cloudflare 验证",
            Self::BookParse => "详情页 HTML 解析失败",
            Self::TocHttp => "目录 HTTP 请求失败",
            Self::TocCloudflare => "目录页命中 Cloudflare 验证",
            Self::TocParse => "目录页 HTML 解析失败",
            Self::ChapterHttp => "章节 HTTP 请求失败",
            Self::ChapterCloudflare => "章节页命中 Cloudflare 验证",
            Self::ChapterParse => "章节 HTML 解析失败",
            Self::SearchHttp => "搜索 HTTP 请求失败",
            Self::SearchCloudflare => "搜索页命中 Cloudflare 验证",
            Self::SearchParse => "搜索结果 HTML 解析失败",
            Self::CrawlerClient => "HTTP 客户端构造失败",
            Self::CrawlerIo => "任务文件 IO 失败",
            Self::CrawlerExport => "导出失败",
            Self::CrawlerBookAggregate => "书源解析失败",
            Self::CrawlerTocAggregate => "目录解析失败",
            Self::NotFound => "资源未找到",
            Self::Conflict => "资源状态冲突",
            Self::BadRequest => "请求参数错误",
            Self::Internal => "内部错误",
            Self::IoError => "IO 错误",
            Self::ExportEmptyChaptersDir => "章节缓存目录为空",
            Self::ExportIo => "导出文件 IO 失败",
            Self::ExportEpub => "EPUB 生成失败",
            Self::ExportZip => "ZIP 打包失败",
            Self::ExportEncoding => "编码转换失败",
            Self::ExportPdf => "PDF 生成失败",
        }
    }

    /// 数字码前缀段 (`1xxx`/`2xxx`/...), 错误大类归类用。
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
        write!(f, "{}: {}", self.code_str(), self.message())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn code_str_matches_numeric_code() {
        // 验证 code() 和 code_str() 同步 — 防加新变体时改数字忘了改字符串 (或反之)
        for variant in [
            ErrorCode::BookRuleMissing,
            ErrorCode::Cancelled,
            ErrorCode::BookHttp,
            ErrorCode::NotFound,
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
    }

    #[test]
    fn message_is_short_enough() {
        // 防 message 长度爆炸: 30 字上限
        for variant in [
            ErrorCode::BookRuleMissing,
            ErrorCode::InvalidRange,
            ErrorCode::CrawlerBookAggregate,
            ErrorCode::ExportEmptyChaptersDir,
        ] {
            assert!(
                variant.message().chars().count() <= 30,
                "{:?} message too long: {}",
                variant,
                variant.message()
            );
        }
    }

    #[test]
    fn display_format() {
        assert_eq!(
            ErrorCode::BookRuleMissing.to_string(),
            "1001: 书源没有 book 规则"
        );
    }
}
