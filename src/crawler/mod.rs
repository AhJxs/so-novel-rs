//! 下载调度层。对应 Java `core.Crawler` + `parse.ChapterParser` 的重试逻辑
//! + `handle.CrawlerPostHandler` 的导出+清理逻辑。
//!
//! 入口：`download_book(cfg, source, book_url, opts) -> Result<PathBuf, CrawlerError>`。
//!
//! 流程：
//! 1. `parse_book_detail` 拿到详情；
//! 2. `parse_toc` 拿到完整章节列表；
//! 3. 用 tokio Semaphore 限并发；每章 `spawn_blocking` 调
//!    `parse_chapter` + `render_chapter`，失败按 `EffectiveCrawl::max_retries` 重试；
//! 4. 章节字符串写入 `<download_path>/<bookDirName>/`；
//! 5. EPUB 时下载封面（soft-skip），其它格式不下载；
//! 6. `Exporter::merge_with_cover` 产出最终文件；
//! 7. `preserve_chapter_cache=false` 时清掉 chapters 临时目录。
//!
//! 进度：通过 `mpsc::UnboundedSender<Progress>` 推送事件给 UI；UI 在 egui
//! update 循环里 `try_recv`。
//!
//! 取消：`Arc<AtomicBool>`；在每章入口检查；正在跑的章节会跑完才退出
//! （彻底中断 HTTP 请求需要 hyper 层级取消，复杂度过高，暂不做）。

pub mod health;
mod retry;
pub mod search;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rand::RngExt;
use thiserror::Error;
use tokio::sync::{mpsc, Semaphore};

use crate::config::AppConfig;
use crate::export::{
    build_book_dir_name, exporter_for, write_chapter_files, ExportError, RenderTarget,
    RenderedChapter,
};
use crate::http::client::{build_async_client, ClientOptions};
use crate::models::{Book, Chapter};
use crate::parser::{
    parse_book_detail, parse_chapter, parse_toc, BookError, ChapterError, TocError,
};
use crate::rules::Source;

pub use retry::linear_backoff;

/// 调度层用户可见的进度事件。
#[derive(Debug, Clone)]
pub enum Progress {
    /// 详情解析完成，得到书籍元信息。
    BookResolved { book: Book, total_chapters: usize },
    /// 一章完成（成功），index 是 1-based 顺序号。
    ChapterDone { index: u32, title: String },
    /// 一章失败（已用尽重试，但不中断整本下载）。
    ChapterFailed {
        index: u32,
        title: String,
        reason: String,
    },
    /// 导出完成，文件已落盘。
    Finished { output_path: PathBuf },
    /// 用户取消（在某章完成 / 失败之后的下一次检查点观测到）。
    Cancelled,
}

#[derive(Debug, Error)]
pub enum CrawlerError {
    #[error("详情页解析失败: {0}")]
    Book(#[from] BookError),
    #[error("目录解析失败: {0}")]
    Toc(#[from] TocError),
    #[error("章节抓取全部失败 — 目录返回 0 章")]
    EmptyToc,
    #[error("HTTP 客户端构造失败: {0}")]
    Client(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("导出失败: {0}")]
    Export(#[from] ExportError),
    #[error("用户取消")]
    Cancelled,
}

/// 取消令牌：在 UI / CLI 侧 clone 一份，set 后下一次检查点会停止。
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// 控制下载行为的选项（与 AppConfig 的字段在 UI 层做映射）。
pub struct DownloadOptions {
    pub progress: mpsc::UnboundedSender<Progress>,
    pub cancel: CancelToken,
}

/// 入口：抓一本书并导出。**全 async**：内部直接 await 各级 parser，
/// 配合 `tokio::select!` 让外部 `cancel` 信号瞬间中断 in-flight HTTP（reqwest 关闭连接）。
///
/// 返回成品文件路径（一个 `.epub` / `.txt` / `.zip`）。
pub async fn download_book(
    cfg: &AppConfig,
    source: &Source,
    book_url: &str,
    opts: DownloadOptions,
) -> Result<PathBuf, CrawlerError> {
    let cancel = opts.cancel.clone();
    let progress = opts.progress.clone();

    let (book, toc) = resolve_book(cfg, source, book_url, &cancel).await?;

    if toc.is_empty() {
        return Err(CrawlerError::EmptyToc);
    }

    let _ = progress.send(Progress::BookResolved {
        book: book.clone(),
        total_chapters: toc.len(),
    });

    if cancel.is_cancelled() {
        let _ = progress.send(Progress::Cancelled);
        return Err(CrawlerError::Cancelled);
    }

    download_chapters(cfg, source, book_url, &book, toc, opts).await
}

/// 阶段一：获取书籍元信息 + 章节列表（不开始下载）。
///
/// 返回 `(Book, Vec<Chapter>)`，供 UI 层展示章节列表、让用户选择范围后再调用
/// `download_chapters`。
pub async fn resolve_book(
    cfg: &AppConfig,
    source: &Source,
    book_url: &str,
    cancel: &CancelToken,
) -> Result<(Book, Vec<Chapter>), CrawlerError> {
    // HTTP 客户端
    let client_options = ClientOptions {
        unsafe_ssl: source.rule.ignore_ssl,
    };
    let client = build_async_client(cfg, &client_options)
        .map_err(|e| CrawlerError::Client(format!("{e:#}")))?;

    let cf_bypass = if cfg.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(cfg.cf_bypass.as_str())
    };

    let rule = source.rule.clone();
    let book_url_owned = book_url.to_string();
    let cf_bypass_owned = cf_bypass.map(String::from);

    // 详情
    let book = parse_book_detail(&client, &rule, &book_url_owned, cf_bypass_owned.as_deref())
        .await
        .map_err(CrawlerError::Book)?;

    if cancel.is_cancelled() {
        return Err(CrawlerError::Cancelled);
    }

    // 目录
    let toc: Vec<Chapter> = parse_toc(&client, &rule, &book_url_owned, cf_bypass_owned.as_deref())
        .await
        .map_err(CrawlerError::Toc)?;

    Ok((book, toc))
}

/// 阶段二：下载指定章节 + 导出。
///
/// `chapters` 已由调用方按用户选择过滤过范围。`DownloadOptions` 中需传入
/// progress sender 和 cancel token。
pub async fn download_chapters(
    cfg: &AppConfig,
    source: &Source,
    _book_url: &str,
    book: &Book,
    chapters: Vec<Chapter>,
    opts: DownloadOptions,
) -> Result<PathBuf, CrawlerError> {
    let DownloadOptions { progress, cancel } = opts;

    if chapters.is_empty() {
        return Err(CrawlerError::EmptyToc);
    }

    // HTTP 客户端（导出封面等用）
    let client_options = ClientOptions {
        unsafe_ssl: source.rule.ignore_ssl,
    };
    let client = build_async_client(cfg, &client_options)
        .map_err(|e| CrawlerError::Client(format!("{e:#}")))?;

    let cf_bypass = if cfg.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(cfg.cf_bypass.as_str())
    };

    let rule = source.rule.clone();
    let cf_bypass_owned = cf_bypass.map(String::from);

    // 准备 chapters 目录
    let book_dir_name = build_book_dir_name(book, cfg.ext_name);
    let chapters_dir = std::path::Path::new(&cfg.download_path).join(&book_dir_name);
    std::fs::create_dir_all(&chapters_dir)?;

    // 并发抓章节
    let max_concurrent = compute_concurrency(source, chapters.len());
    let render_target: RenderTarget = cfg.ext_name.into();
    let rule_chapter = rule
        .chapter
        .clone()
        .ok_or(CrawlerError::Toc(TocError::TocRuleMissing))?;

    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let eff = source.effective_crawl.clone();
    let mut handles = Vec::with_capacity(chapters.len());

    for chapter in chapters {
        if cancel.is_cancelled() {
            // dispatch 阶段被取消：dir 里还没写过任何章节文件，安全清理。
            cleanup_chapters_dir_if_empty(&chapters_dir);
            let _ = progress.send(Progress::Cancelled);
            return Err(CrawlerError::Cancelled);
        }
        let permit = Arc::clone(&semaphore);
        let client = client.clone();
        let rule = rule.clone();
        let rule_chapter = rule_chapter.clone();
        let cf = cf_bypass_owned.clone();
        let progress = progress.clone();
        let cancel = cancel.clone();
        let eff = eff.clone();
        let enable_retry = cfg.enable_retry;

        handles.push(tokio::spawn(async move {
            // 限并发
            let _permit = permit.acquire_owned().await.expect("semaphore not closed");

            // 章节内随机间隔（爬取礼貌性）。
            // 用 select! 配合 cancel — interval 一般是 100ms-2s，整段 sleep
            // 期间用户取消应当瞬间响应，不能傻等到 sleep 结束。
            let interval = {
                let lo = eff.min_interval_ms.max(1) as u64;
                let hi = eff.max_interval_ms.max(eff.min_interval_ms + 1) as u64;
                rand::rng().random_range(lo..=hi)
            };
            tokio::select! {
                biased;
                _ = wait_cancelled(&cancel) => return ChapterOutcome::Cancelled(chapter),
                _ = tokio::time::sleep(Duration::from_millis(interval)) => {}
            }

            if cancel.is_cancelled() {
                return ChapterOutcome::Cancelled(chapter);
            }

            // 抓 + 重试
            let max_attempts = if enable_retry { eff.max_retries } else { 0 };
            let retry_min = eff.retry_min_interval_ms.max(1);
            let order = chapter.order;
            let title = chapter.title.clone();

            // 把章节抓取整段（含重试）和 cancel 信号 race；cancel 一来 future
            // 立刻被 drop，reqwest::Client::send 关闭底层连接 — 真·零延迟。
            let fetch_future = retry::retry_with_backoff(
                |_attempt| {
                    let client = client.clone();
                    let rule = rule.clone();
                    let chapter = chapter.clone();
                    let cf = cf.clone();
                    let cancel = cancel.clone();
                    async move {
                        if cancel.is_cancelled() {
                            return Err(ChapterError::Http("cancelled".to_string()));
                        }
                        parse_chapter(&client, &rule, &chapter, cf.as_deref()).await
                    }
                },
                max_attempts,
                |attempt| async move {
                    let base = retry_min as u64;
                    let dur = retry::linear_backoff(base, attempt);
                    tokio::time::sleep(dur).await;
                },
            );

            let result = tokio::select! {
                biased;
                _ = wait_cancelled(&cancel) => {
                    return ChapterOutcome::Cancelled(chapter);
                }
                r = fetch_future => r,
            };

            match result {
                Ok(parsed) => {
                    let (final_title, body) =
                        crate::export::render_chapter(&parsed, &rule_chapter, render_target);
                    let _ = progress.send(Progress::ChapterDone {
                        index: order,
                        title: final_title.clone(),
                    });
                    ChapterOutcome::Done(RenderedChapter {
                        order,
                        title: final_title,
                        body,
                    })
                }
                Err(e) => {
                    let reason = format!("{e}");
                    let _ = progress.send(Progress::ChapterFailed {
                        index: order,
                        title: title.clone(),
                        reason: reason.clone(),
                    });
                    ChapterOutcome::Failed {
                        order,
                        title,
                        reason,
                    }
                }
            }
        }));
    }

    // 5. 收尾。同时用 select! race "全部 join" 与 "cancel"：
    //    - 用户取消 → 立即 abort 所有 chapter handle、发 Progress::Cancelled、return
    //    - 正常完成 → 收 rendered
    let mut rendered: Vec<RenderedChapter> = Vec::new();
    let drain_handles = async {
        for h in &mut handles {
            match h.await {
                Ok(ChapterOutcome::Done(r)) => rendered.push(r),
                Ok(ChapterOutcome::Failed { .. }) => {}
                Ok(ChapterOutcome::Cancelled(_)) => {}
                Err(join_err) => {
                    tracing::warn!("章节任务 join 失败: {join_err}");
                }
            }
        }
    };

    tokio::select! {
        biased;
        _ = wait_cancelled(&cancel) => {
            // 主动 abort 还在跑的章节 — 它们的 future 被 drop 后 reqwest 立刻关连接
            for h in &handles {
                h.abort();
            }
            // 章节文件要等到 select! 之后的 write_chapter_files 才落盘，
            // 所以取消时 chapters_dir 一定是空的（除非用户事先放过文件） — 直接清理。
            cleanup_chapters_dir_if_empty(&chapters_dir);
            let _ = progress.send(Progress::Cancelled);
            return Err(CrawlerError::Cancelled);
        }
        () = drain_handles => {}
    }

    if cancel.is_cancelled() {
        cleanup_chapters_dir_if_empty(&chapters_dir);
        let _ = progress.send(Progress::Cancelled);
        return Err(CrawlerError::Cancelled);
    }
    if rendered.is_empty() {
        // 所有章节都失败：跟取消同等处理 — dir 还是空的，清掉。
        cleanup_chapters_dir_if_empty(&chapters_dir);
        return Err(CrawlerError::EmptyToc);
    }

    // 按 order 排序（spawn 出来的 Future 完成顺序不保证）
    rendered.sort_by_key(|r| r.order);

    // 6. 写章节文件 + 导出
    write_chapter_files(&chapters_dir, &rendered, cfg.ext_name)
        .map_err(|e| CrawlerError::Client(format!("write chapters: {e:#}")))?;

    // 封面：仅 EPUB 才下载；失败 soft-skip。
    let cover_bytes = if matches!(render_target, RenderTarget::Epub) {
        download_cover(&client, book.cover_url.as_deref()).await
    } else {
        None
    };

    let exporter = exporter_for(cfg.ext_name, &cfg.txt_encoding);
    let out_dir = std::path::Path::new(&cfg.download_path);
    let final_path =
        exporter.merge_with_cover(book, &chapters_dir, out_dir, cover_bytes.as_deref())?;

    // 7. 清理章节临时目录
    if !cfg.preserve_chapter_cache {
        if let Err(e) = std::fs::remove_dir_all(&chapters_dir) {
            tracing::warn!(
                "清理章节缓存目录失败（已忽略）: {} — {e}",
                chapters_dir.display()
            );
        }
    }

    let _ = progress.send(Progress::Finished {
        output_path: final_path.clone(),
    });
    Ok(final_path)
}

enum ChapterOutcome {
    Done(RenderedChapter),
    Failed {
        #[allow(dead_code)]
        order: u32,
        #[allow(dead_code)]
        title: String,
        #[allow(dead_code)]
        reason: String,
    },
    Cancelled(#[allow(dead_code)] Chapter),
}

/// 与 Java `Crawler` 中并发数计算一致：
/// - `concurrency = Some(n)`：直接用，但 ≤ 100；
/// - `None`（即 -1）：自动 = `min(50, toc.len())`；
/// - 任何情况下不超过章节数。
fn compute_concurrency(source: &Source, toc_len: usize) -> usize {
    let configured = source.effective_crawl.concurrency;
    let raw = match configured {
        Some(n) if n > 0 => n.min(100) as usize,
        _ => 50.min(toc_len.max(1)),
    };
    raw.min(toc_len.max(1)).max(1)
}

/// 取消 / 提前失败时清理 `chapters_dir`：**只删空目录**。
///
/// - 目录里已经写过章节文件（用户成功跑过部分章节但中途取消，下一轮"重试"想接着用）
///   或用户事先在那放了别的文件 → 保留；
/// - 目录是空的（典型场景：取消时还没来得及 `write_chapter_files`）→ 删掉，不留垃圾。
///
/// 用 `read_dir().next().is_none()` 判空，比 `remove_dir_all` 安全得多。
/// 任何 IO 错误都只 `tracing::warn!` —— 清理失败不影响 cancel 主流程，下次启动用户
/// 自己删也不痛。
fn cleanup_chapters_dir_if_empty(dir: &std::path::Path) {
    let Ok(mut entries) = std::fs::read_dir(dir) else {
        // 目录不存在 / 读不了 — 没什么可清的
        return;
    };
    if entries.next().is_some() {
        // 非空：有人放过东西，保留
        return;
    }
    // read_dir 拿到的迭代器在某些平台会持有目录句柄，必须显式 drop 再 remove，
    // 否则 Windows 上偶发 "目录正在使用" 错误。
    drop(entries);
    if let Err(e) = std::fs::remove_dir(dir) {
        tracing::warn!(
            "清理空章节目录失败（已忽略）: {} — {e}",
            dir.display()
        );
    } else {
        tracing::debug!("已清理空章节目录: {}", dir.display());
    }
}

async fn download_cover(client: &reqwest::Client, cover_url: Option<&str>) -> Option<Vec<u8>> {
    let url = cover_url?.trim();
    if url.is_empty() {
        return None;
    }
    let resp = match client
        .get(url)
        .timeout(Duration::from_secs(15))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("封面下载失败（已忽略，将不带封面导出）: {e}");
            return None;
        }
    };
    match resp.bytes().await {
        Ok(b) if !b.is_empty() => Some(b.to_vec()),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("封面下载失败（已忽略，将不带封面导出）: {e}");
            None
        }
    }
}

/// 把 future race "用户取消"。cancel 命中时返回 `Err(CrawlerError::Cancelled)`，
/// 同时主动发一次 `Progress::Cancelled`（保证 UI 立刻能看到）。
///
/// 用法是 `race_with_cancel(parse_*(...), &cancel, &progress).await?` —
/// 返回 `Result<R, CrawlerError>`，业务结果再用 `.map_err(...)` 转换。
#[allow(dead_code)]
async fn race_with_cancel<R, F>(
    fut: F,
    cancel: &CancelToken,
    progress: &mpsc::UnboundedSender<Progress>,
) -> Result<R, CrawlerError>
where
    F: std::future::Future<Output = R>,
{
    tokio::select! {
        biased;
        _ = wait_cancelled(cancel) => {
            let _ = progress.send(Progress::Cancelled);
            Err(CrawlerError::Cancelled)
        }
        r = fut => Ok(r),
    }
}

/// 把 `CancelToken` 适配成 future：每 50ms poll 一次 atomic flag。
///
/// 之所以走 polling 而不是 tokio::sync::Notify：CancelToken 已经是
/// `Arc<AtomicBool>`，在 UI / CLI / parser 多处共享，改 API 影响面太大。
/// 50ms 足够"零延迟"用户感知（一帧 16ms 的 UI 多 50ms 反应人眼看不出），
/// 并且 atomic load 几乎免费。
async fn wait_cancelled(cancel: &CancelToken) {
    loop {
        if cancel.is_cancelled() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Rule, RuleCrawl};

    fn make_source(concurrency: Option<u32>) -> Source {
        let cfg = AppConfig::default();
        let crawl = concurrency.map(|c| RuleCrawl {
            concurrency: Some(c),
            ..Default::default()
        });
        let rule = Rule {
            url: "https://x".into(),
            crawl,
            ..Rule::default()
        };
        Source::from(rule, &cfg)
    }

    #[test]
    fn concurrency_default_is_min_50_and_toc() {
        let s = make_source(None);
        // toc 较大时上限 50
        assert_eq!(compute_concurrency(&s, 200), 50);
        // toc 较小时被章节数压低
        assert_eq!(compute_concurrency(&s, 7), 7);
        // 0 章节也至少给 1（避免 Semaphore::new(0)）
        assert_eq!(compute_concurrency(&s, 0), 1);
    }

    #[test]
    fn concurrency_respects_rule_override_and_caps_at_100() {
        let s = make_source(Some(5));
        assert_eq!(compute_concurrency(&s, 200), 5);

        let huge = make_source(Some(500));
        assert_eq!(compute_concurrency(&huge, 200), 100);

        // 即使配置了大值，也不能超过 toc 长度
        let s2 = make_source(Some(50));
        assert_eq!(compute_concurrency(&s2, 3), 3);
    }

    #[test]
    fn cancel_token_starts_uncancelled_and_can_be_set() {
        let t = CancelToken::new();
        assert!(!t.is_cancelled());
        t.cancel();
        assert!(t.is_cancelled());
        // clone 后仍共享状态
        let t2 = t.clone();
        assert!(t2.is_cancelled());
    }
}
