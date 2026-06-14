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

use rand::Rng;
use thiserror::Error;
use tokio::sync::{mpsc, Semaphore};

use crate::config::AppConfig;
use crate::export::{
    build_book_dir_name, exporter_for, write_chapter_files, ExportError, RenderTarget,
    RenderedChapter,
};
use crate::http::client::{build_blocking_client, ClientOptions};
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

/// 入口：抓一本书并导出。**异步**：内部用 spawn_blocking 调用同步 parser。
///
/// 返回成品文件路径（一个 `.epub` / `.txt` / `.zip`）。
pub async fn download_book(
    cfg: &AppConfig,
    source: &Source,
    book_url: &str,
    opts: DownloadOptions,
) -> Result<PathBuf, CrawlerError> {
    let DownloadOptions { progress, cancel } = opts;

    // 1. HTTP 客户端 — 阻塞客户端，parser 是同步的
    let client_options = ClientOptions {
        unsafe_ssl: source.rule.ignore_ssl,
    };
    let client = build_blocking_client(cfg, &client_options)
        .map_err(|e| CrawlerError::Client(format!("{e:#}")))?;

    let cf_bypass = if cfg.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(cfg.cf_bypass.as_str())
    };

    // 2. 详情 + 目录（同步调用，但放 spawn_blocking 避免阻塞 runtime）
    let rule = source.rule.clone();
    let book_url_owned = book_url.to_string();
    let cf_bypass_owned = cf_bypass.map(String::from);
    let client_for_meta = client.clone();
    let book = {
        let rule = rule.clone();
        let url = book_url_owned.clone();
        let cf = cf_bypass_owned.clone();
        tokio::task::spawn_blocking(move || {
            parse_book_detail(&client_for_meta, &rule, &url, cf.as_deref())
        })
        .await
        .map_err(|e| CrawlerError::Client(format!("spawn_blocking: {e}")))??
    };

    if cancel.is_cancelled() {
        let _ = progress.send(Progress::Cancelled);
        return Err(CrawlerError::Cancelled);
    }

    let toc: Vec<Chapter> = {
        let rule = rule.clone();
        let url = book_url_owned.clone();
        let cf = cf_bypass_owned.clone();
        let client_for_toc = client.clone();
        tokio::task::spawn_blocking(move || parse_toc(&client_for_toc, &rule, &url, cf.as_deref()))
            .await
            .map_err(|e| CrawlerError::Client(format!("spawn_blocking: {e}")))??
    };

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

    // 3. 准备 chapters 目录
    let book_dir_name = build_book_dir_name(&book, cfg.ext_name);
    let chapters_dir = std::path::Path::new(&cfg.download_path).join(&book_dir_name);
    std::fs::create_dir_all(&chapters_dir)?;

    // 4. 并发抓章节
    let max_concurrent = compute_concurrency(source, toc.len());
    let render_target: RenderTarget = cfg.ext_name.into();
    let rule_chapter = rule
        .chapter
        .clone()
        .ok_or(CrawlerError::Toc(TocError::TocRuleMissing))?;

    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let eff = source.effective_crawl.clone();
    let mut handles = Vec::with_capacity(toc.len());

    for chapter in toc {
        if cancel.is_cancelled() {
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

            // 章节内随机间隔（爬取礼貌性）
            let interval = {
                let lo = eff.min_interval_ms.max(1) as u64;
                let hi = eff.max_interval_ms.max(eff.min_interval_ms + 1) as u64;
                rand::thread_rng().gen_range(lo..=hi)
            };
            tokio::time::sleep(Duration::from_millis(interval)).await;

            if cancel.is_cancelled() {
                return ChapterOutcome::Cancelled(chapter);
            }

            // 抓 + 重试
            let max_attempts = if enable_retry { eff.max_retries } else { 0 };
            let retry_min = eff.retry_min_interval_ms.max(1);
            let order = chapter.order;
            let title = chapter.title.clone();

            let result = retry::retry_with_backoff(
                |_attempt| {
                    if cancel.is_cancelled() {
                        // 用一个特殊错误表示取消，下面会被忽略
                        return Err(ChapterError::Http("cancelled".to_string()));
                    }
                    parse_chapter(&client, &rule, &chapter, cf.as_deref())
                },
                max_attempts,
                |attempt| async move {
                    let base = retry_min as u64;
                    let dur = retry::linear_backoff(base, attempt);
                    tokio::time::sleep(dur).await;
                },
            )
            .await;

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

    // 5. 收尾
    let mut rendered: Vec<RenderedChapter> = Vec::new();
    for h in handles {
        match h.await {
            Ok(ChapterOutcome::Done(r)) => rendered.push(r),
            Ok(ChapterOutcome::Failed { .. }) => {
                // 已通过 progress 报告失败；不计入 rendered。
            }
            Ok(ChapterOutcome::Cancelled(_)) => {
                // 被取消的章节略过；最终是否取消由 cancel flag 决定。
            }
            Err(join_err) => {
                tracing::warn!("章节任务 join 失败: {join_err}");
            }
        }
    }

    if cancel.is_cancelled() {
        let _ = progress.send(Progress::Cancelled);
        return Err(CrawlerError::Cancelled);
    }
    if rendered.is_empty() {
        return Err(CrawlerError::EmptyToc);
    }

    // 按 order 排序（spawn 出来的 Future 完成顺序不保证）
    rendered.sort_by_key(|r| r.order);

    // 6. 写章节文件 + 导出
    write_chapter_files(&chapters_dir, &rendered, cfg.ext_name)
        .map_err(|e| CrawlerError::Client(format!("write chapters: {e:#}")))?;

    // 封面：仅 EPUB 才下载；失败 soft-skip。
    let cover_bytes = if matches!(render_target, RenderTarget::Epub) {
        download_cover(&client, book.cover_url.as_deref())
    } else {
        None
    };

    let exporter = exporter_for(cfg.ext_name, &cfg.txt_encoding);
    let out_dir = std::path::Path::new(&cfg.download_path);
    let final_path =
        exporter.merge_with_cover(&book, &chapters_dir, out_dir, cover_bytes.as_deref())?;

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

fn download_cover(client: &reqwest::blocking::Client, cover_url: Option<&str>) -> Option<Vec<u8>> {
    let url = cover_url?.trim();
    if url.is_empty() {
        return None;
    }
    match client
        .get(url)
        .timeout(Duration::from_secs(15))
        .send()
        .and_then(|r| r.bytes())
    {
        Ok(b) if !b.is_empty() => Some(b.to_vec()),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("封面下载失败（已忽略，将不带封面导出）: {e}");
            None
        }
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
