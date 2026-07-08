//! 阶段一: 详情 + 目录解析
//!
//! 返回 `(Book, Vec<Chapter>)`, 供 UI 层展示章节列表, 让用户选择范围后再
//! 调用 [`super::download::download_chapters`]。
//!
//! 详情 + 目录都包重试 (`retry_with_backoff`), 一次瞬态 HTTP 抖动不应让
//! 整本下载失败。

use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

use crate::config::AppConfig;
use crate::models::{Book, Chapter, Source};
use crate::parser::{BookError, SelectError, TocError, parse_book_detail, parse_toc};
use crate::utils::zhconv::convert_book_meta;

use super::download_options::CancelToken;
use super::retry::retry_with_backoff;

/// 调度层错误。
///
/// 业务方用 `From<...>` 透传: 详情 / 章节 / 目录 / 导出 / IO / 取消
/// 各自一个变体, 边界层 ([`crate::web::error::WebError`]) 收口映射。
///
/// # Examples
///
/// ```ignore
/// use crate::crawler::{CrawlerError, download_book};
/// match download_book(...).await {
///     Err(CrawlerError::Cancelled) => log.info("user cancelled"),
///     Err(CrawlerError::EmptyToc) => log.warn("no chapters"),
///     Err(e) => log.error!("{e:#}"),
///     Ok(p) => log.info!("done: {}", p.display()),
/// }
/// ```
#[derive(Debug, Error)]
pub enum CrawlerError {
    /// 详情页解析失败 (HTTP / CF / Parse / Selector 4 子类)。
    #[error("详情页解析失败: {0}")]
    Book(#[from] BookError),
    /// 目录解析失败 (HTTP / CF / Parse / Selector 4 子类)。
    #[error("目录解析失败: {0}")]
    Toc(#[from] TocError),
    /// 章节抓取全部失败 — 目录返回 0 章。
    #[error("章节抓取全部失败 — 目录返回 0 章")]
    EmptyToc,
    /// HTTP 客户端构造失败 (reqwest builder 抛错)。
    #[error("HTTP 客户端构造失败: {0}")]
    Client(String),
    /// 标准库 IO 错误 (写盘 / `create_dir_all` 等)。
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    /// 导出失败 (EPUB / TXT / PDF 等)。
    #[error("导出失败: {0}")]
    Export(#[from] crate::export::ExportError),
    /// 用户取消。
    #[error("用户取消")]
    Cancelled,
    /// 调用方传入的章节范围 (--from / --to) 非法。
    #[error("章节范围非法: {0}")]
    InvalidRange(String),
}

/// 阶段一: 获取书籍元信息 + 章节列表 (不开始下载)。
///
/// 返回 `(Book, Vec<Chapter>)`, 供 UI 层展示章节列表、让用户选择范围后再调用
/// `download_chapters`。
///
/// # Examples
///
/// ```ignore
/// let (book, chapters) = resolve_book(&cfg, &client, &source, &url, &cancel).await?;
/// println!("{}: {} 章", book.book_name, chapters.len());
/// ```
///
/// # Errors
///
/// - `CrawlerError::Book` — 详情页解析失败 (含重试后仍失败)
/// - `CrawlerError::Toc` — 目录解析失败
/// - `CrawlerError::Cancelled` — 取消令牌触发
#[tracing::instrument(
    name = "resolve_book",
    skip_all,
    fields(
        source_id = source.rule.id,
        source = %source.rule.name,
        %book_url,
    )
)]
pub async fn resolve_book(
    cfg: &AppConfig,
    client: &reqwest::Client,
    source: &Source,
    book_url: &str,
    cancel: &CancelToken,
) -> Result<(Book, Vec<Chapter>), CrawlerError> {
    let cf_bypass = if cfg.global.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(cfg.global.cf_bypass.as_str())
    };

    // 全局起点 cookie (整段粘贴), 仅供详情页末尾的 CoverUpdater 用:
    // 详情页 fetch 本身不附 Cookie, 与 Java 端语义一致。
    let qidian_cookie = if cfg.cookie.qidian_cookie.trim().is_empty() {
        None
    } else {
        Some(cfg.cookie.qidian_cookie.as_str())
    };

    let rule = source.rule.clone();
    let book_url_owned = book_url.to_string();
    let cf_bypass_owned: Option<Arc<str>> = cf_bypass.map(Arc::from);
    let qidian_cookie_owned = qidian_cookie.map(String::from);
    let eff = source.effective_crawl.clone();
    let max_attempts = if cfg.crawl.enable_retry {
        eff.max_retries
    } else {
        0
    };
    // 两次 retry 各需一份 eff (sleep_fn 是 move 闭包)。
    let eff_for_book = eff.clone();
    let eff_for_toc = eff;

    // 详情: 包重试 (与章节抓取同款 enable_retry 门控)。详情页一次瞬态 HTTP 抖动
    // 不应让整本下载失败。
    let book = retry_with_backoff(
        |_attempt| {
            let client = client.clone();
            let rule = rule.clone();
            let url = book_url_owned.clone();
            let cf = cf_bypass_owned.as_ref().map(Arc::clone);
            let qc = qidian_cookie_owned.clone();
            let cancel = cancel.clone();
            async move {
                if cancel.is_cancelled() {
                    return Err(BookError::Http("cancelled".to_string()));
                }
                parse_book_detail(&client, &rule, &url, cf.as_deref(), qc.as_deref()).await
            }
        },
        max_attempts,
        move |_attempt| {
            let dur = Duration::from_millis(crate::http::random_retry_interval_ms(&eff_for_book));
            async move {
                tokio::time::sleep(dur).await;
            }
        },
    )
    .await
    .map_err(CrawlerError::Book)?;

    if cancel.is_cancelled() {
        return Err(CrawlerError::Cancelled);
    }

    // 目录: 同样包重试。
    let toc: Vec<Chapter> = retry_with_backoff(
        |_attempt| {
            let client = client.clone();
            let rule = rule.clone();
            let url = book_url_owned.clone();
            let cf = cf_bypass_owned.as_ref().map(Arc::clone);
            let cancel = cancel.clone();
            async move {
                if cancel.is_cancelled() {
                    return Err(TocError::Selector(SelectError::JsFailed(
                        "cancelled".to_string(),
                    )));
                }
                parse_toc(&client, &rule, &url, cf.as_deref()).await
            }
        },
        max_attempts,
        move |_attempt| {
            let dur = Duration::from_millis(crate::http::random_retry_interval_ms(&eff_for_toc));
            async move {
                tokio::time::sleep(dur).await;
            }
        },
    )
    .await
    .map_err(CrawlerError::Toc)?;

    // 书名/作者/简介 简繁转换: 与章节正文 (`render_chapter`) 用同一 source/target 语义。
    // 在这里集中做一次, UI (任务页 / 书库)、缓存目录名 (`build_book_dir_name`)、
    // 导出器 (EPUB metadata + 文件名 + TXT 首页 + HTML zip 名) 全部共享转换结果,
    // 避免每个 exporter 各自重写一份 + 减少 drift 风险。
    //
    // 目标语言从界面语言 (`Language`) 推 —— 合并 UI/书源语言设置后用户只设一个
    // Language, 下载时的简繁转换目标即由此决定 (English → ZhCn 兜底)。
    let target_lang = cfg.global.language.to_book_target_lang();
    let book = convert_book_meta(&book, &source.rule.language, &target_lang);

    Ok((book, toc))
}
