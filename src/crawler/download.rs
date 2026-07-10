//! 阶段二: 章节下载 + 导出
//!
//! 主流程: `download_book` 调 `resolve_book` 拿到 Book + 章节列表,
//! 然后调 `download_chapters` 并发抓取 + 导出。
//!
//! 关键路径: Semaphore 限并发 / per-task write / 取消用 `tokio::select`
//! 配合 reqwest future drop 实现零延迟中断。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rand::RngExt;
use tokio::sync::Semaphore;

use crate::config::AppConfig;
use crate::export::{RenderTarget, build_book_dir_name, exporter_for, write_single_chapter};
use crate::models::{Book, Chapter, Source};
use crate::parser::{ChapterError, TocError, parse_chapter};

use super::download_options::DownloadOptions;
use super::progress::Progress;
use super::resolve::{CrawlerError, resolve_book};
use super::retry as retry_mod;

/// 入口: 抓一本书并导出。**全 async**: 内部直接 await 各级 parser,
/// 配合 `tokio::select!` 让外部 `cancel` 信号瞬间中断 in-flight HTTP (reqwest 关闭连接)。
///
/// 返回成品文件路径 (一个 `.epub` / `.txt` / `.zip`)。
///
/// # Examples
///
/// ```ignore
/// let path = download_book(&cfg, &client, &source, &url, opts).await?;
/// println!("saved: {}", path.display());
/// ```
///
/// # Errors
///
/// - `CrawlerError::Book` / `Toc` — 详情/目录解析失败
/// - `CrawlerError::EmptyToc` — 目录返回 0 章
/// - `CrawlerError::Cancelled` — 用户取消
/// - `CrawlerError::Export` — 导出失败
#[tracing::instrument(
    name = "download_book",
    skip_all,
    fields(
        source_id = source.rule.id,
        source = %source.rule.name,
        %book_url,
    )
)]
pub async fn download_book(
    cfg: &AppConfig,
    client: &reqwest::Client,
    source: &Source,
    book_url: &str,
    opts: DownloadOptions,
) -> Result<PathBuf, CrawlerError> {
    let cancel = opts.cancel.clone();
    let progress = opts.progress.clone();
    let notify = opts.notify.clone();

    let (book, toc) = resolve_book(cfg, client, source, book_url, &cancel).await?;

    if toc.is_empty() {
        // 真实失败: 书源返回 0 章, 无法继续。warn 让 trace_id 里有原因可查。
        tracing::warn!("download_book: 章节列表为空");
        return Err(CrawlerError::EmptyToc);
    }

    let _ = progress.send(Progress::BookResolved {
        book: Box::new(book.clone()),
        total_chapters: toc.len(),
    });
    if let Some(ref n) = notify {
        n();
    }

    if cancel.is_cancelled() {
        let _ = progress.send(Progress::Cancelled);
        if let Some(ref n) = notify {
            n();
        }
        return Err(CrawlerError::Cancelled);
    }

    download_chapters(cfg, client, source, &book, toc, opts).await
}

/// 阶段二: 下载指定章节 + 导出。
///
/// `chapters` 已由调用方按用户选择过滤过范围。`DownloadOptions` 中需传入
/// progress sender 和 cancel token。
///
/// # Examples
///
/// ```ignore
/// let path = download_chapters(&cfg, &client, &source, &book, chapters, opts).await?;
/// ```
///
/// # Errors
///
/// - `CrawlerError::Cancelled` — 用户取消 (含 dispatch 阶段 + drain 阶段)
/// - `CrawlerError::EmptyToc` — 全部章节失败 / 目录空
/// - `CrawlerError::Export` — 导出失败
/// - `CrawlerError::Io` — 写盘失败
#[tracing::instrument(
    name = "download_chapters",
    skip_all,
    fields(
        source_id = source.rule.id,
        source = %source.rule.name,
        book = %book.book_name,
        chapters = chapters.len(),
    )
)]
pub async fn download_chapters(
    cfg: &AppConfig,
    client: &reqwest::Client,
    source: &Source,
    book: &Book,
    chapters: Vec<Chapter>,
    opts: DownloadOptions,
) -> Result<PathBuf, CrawlerError> {
    let DownloadOptions {
        progress,
        cancel,
        notify,
    } = opts;

    if chapters.is_empty() {
        return Err(CrawlerError::EmptyToc);
    }

    let cf_bypass = if cfg.global.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(cfg.global.cf_bypass.as_str())
    };

    let rule = Arc::new(source.rule.clone());
    let cf_bypass_owned: Option<Arc<str>> = cf_bypass.map(Arc::from);

    // 准备 chapters 目录
    let book_dir_name = build_book_dir_name(book, cfg.download.ext_name);
    let chapters_dir = std::path::Path::new(&cfg.download.download_path).join(&book_dir_name);
    std::fs::create_dir_all(&chapters_dir)?;

    // 并发抓章节
    let max_concurrent = compute_concurrency(source, chapters.len());
    let render_target: RenderTarget = cfg.download.ext_name.into();
    let rule_chapter = Arc::new(
        rule.chapter
            .clone()
            .ok_or(CrawlerError::Toc(TocError::TocRuleMissing))?,
    );

    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let eff = source.effective_crawl.clone();
    let mut handles = Vec::with_capacity(chapters.len());
    let chapter_count = chapters.len();

    let format = cfg.download.ext_name;
    let digit_count = chapters.len().to_string().len().max(3);

    for chapter in chapters {
        if cancel.is_cancelled() {
            // dispatch 阶段被取消: dir 里还没写过任何章节文件, 安全清理。
            cleanup_chapters_dir_if_empty(&chapters_dir);
            let _ = progress.send(Progress::Cancelled);
            if let Some(ref n) = notify {
                n();
            }
            return Err(CrawlerError::Cancelled);
        }
        let permit = Arc::clone(&semaphore);
        let client = client.clone();
        let rule = Arc::clone(&rule);
        let rule_chapter = Arc::clone(&rule_chapter);
        let cf = cf_bypass_owned.as_ref().map(Arc::clone);
        let progress = progress.clone();
        let cancel = cancel.clone();
        let eff = eff.clone();
        let enable_retry = cfg.crawl.enable_retry;
        // 简繁转换需要源/目标语言。`source` / `cfg` 是借用, 闭包 'static 要求 owned
        // 值; clone Rule (Arc 浅拷贝, 开销小) 和 target LangType (Copy)。
        // 目标语言从界面语言 (`Language`) 推导 —— 合并设置后用户只设 Language。
        let rule_lang = source.rule.language.clone();
        let target_lang = cfg.global.language.to_book_target_lang();

        // per-task write path
        let ch_dir = chapters_dir.clone();
        let ch_format = format;
        let ch_digit_count = digit_count;
        let ch_notify = notify.clone();

        handles.push(tokio::spawn(async move {
            // 限并发。`acquire_owned` 当前 tokio 实现在 Semaphore 不被 close 的情况
            // 下永远成功, 但 API 返回 Result; 防御性: 万一未来切换实现/close 信号进来,
            // 把这一章记为 Failed 而不是 panic 把整个下载任务搞炸。
            let _permit = match permit.acquire_owned().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("semaphore acquire 失败: {e}");
                    return ChapterOutcome::Failed;
                }
            };

            // 章节内随机间隔 (爬取礼貌性)。
            // 用 select! 配合 cancel — interval 一般是 100ms-2s, 整段 sleep
            // 期间用户取消应当瞬间响应, 不能傻等到 sleep 结束。
            let interval = {
                let lo = eff.min_interval_ms.max(1) as u64;
                let hi = eff.max_interval_ms.max(eff.min_interval_ms + 1) as u64;
                rand::rng().random_range(lo..=hi)
            };
            tokio::select! {
                biased;
                () = cancel.wait_cancelled() => return ChapterOutcome::Cancelled,
                () = tokio::time::sleep(Duration::from_millis(interval)) => {}
            }

            if cancel.is_cancelled() {
                return ChapterOutcome::Cancelled;
            }

            // 抓 + 重试
            let max_attempts = if enable_retry { eff.max_retries } else { 0 };
            let order = chapter.order;
            let title = chapter.title.clone();

            // 把章节抓取整段 (含重试) 和 cancel 信号 race; cancel 一来 future
            // 立刻被 drop, reqwest::Client::send 关闭底层连接 — 真·零延迟。
            let fetch_future = retry_mod::retry_with_backoff(
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
                        //       ^^^^^^^ Arc<Rule> 自动 deref coercion 到 &Rule
                    }
                },
                max_attempts,
                // 重试间隔用 random_retry_interval_ms (同时受 retry-min / retry-max 约束,
                // 与 Java 端 randomInterval(config, true) 一致)。原 linear_backoff 不读
                // retry-max, 会让 retry-max-interval 配置形同虚设。
                move |_attempt| {
                    let dur = Duration::from_millis(crate::http::random_retry_interval_ms(&eff));
                    async move {
                        tokio::time::sleep(dur).await;
                    }
                },
            );

            let result = tokio::select! {
                biased;
                () = cancel.wait_cancelled() => {
                    return ChapterOutcome::Cancelled;
                }
                r = fetch_future => r,
            };

            match result {
                Ok(parsed) => {
                    let (final_title, body) = crate::export::render_chapter(
                        &parsed,
                        &rule_chapter,
                        render_target,
                        &rule_lang,
                        target_lang,
                    );
                    // per-chapter write: persist immediately
                    if let Err(e) = write_single_chapter(
                        &ch_dir,
                        order,
                        &final_title,
                        &body,
                        ch_format,
                        ch_digit_count,
                    ) {
                        tracing::warn!(order, error = %e, "chapter file write failed (skipped)");
                    }
                    let _ = progress.send(Progress::ChapterDone {
                        index: order,
                        title: final_title,
                    });
                    if let Some(ref n) = ch_notify {
                        n();
                    }
                    ChapterOutcome::Done
                }
                Err(e) => {
                    let reason = format!("{e:#}");
                    // `sub=chapter:{order}` 让 grep `trace_id=N` 后能直接看到
                    // 这次下载里所有失败的章节序号 (按 sub 排序)。
                    tracing::warn!(
                        order = order,
                        sub = %format!("chapter:{order}"),
                        title = %title,
                        error = %reason,
                        "章节解析失败"
                    );
                    let _ = progress.send(Progress::ChapterFailed {
                        index: order,
                        title: title.clone(),
                        reason,
                    });
                    if let Some(ref n) = ch_notify {
                        n();
                    }
                    ChapterOutcome::Failed
                }
            }
        }));
    }

    // 5. 收尾。同时用 select! race "全部 join" 与 "cancel":
    //    - 用户取消 → 立即 abort 所有 chapter handle、发 Progress::Cancelled、return
    //    - 正常完成 → 收 rendered
    let mut rendered_count = 0usize;
    let drain_handles = async {
        for h in &mut handles {
            match h.await {
                Ok(ChapterOutcome::Done) => rendered_count += 1,
                Ok(ChapterOutcome::Failed | ChapterOutcome::Cancelled) => {}
                Err(join_err) => {
                    tracing::warn!("章节任务 join 失败: {join_err}");
                }
            }
        }
    };

    tokio::select! {
        biased;
        () = cancel.wait_cancelled() => {
            // 主动 abort 还在跑的章节 — 它们的 future 被 drop 后 reqwest 立刻关连接
            for h in &handles {
                h.abort();
            }
            // 章节文件在 task 内部已完成写盘 (或未能写入),
            // 所以取消时 chapters_dir 一定是空的 (除非用户事先放过文件) — 直接清理。
            cleanup_chapters_dir_if_empty(&chapters_dir);
            let _ = progress.send(Progress::Cancelled);
            if let Some(ref n) = notify { n() }
            return Err(CrawlerError::Cancelled);
        }
        () = drain_handles => {}
    }

    if cancel.is_cancelled() {
        cleanup_chapters_dir_if_empty(&chapters_dir);
        let _ = progress.send(Progress::Cancelled);
        if let Some(ref n) = notify {
            n();
        }
        return Err(CrawlerError::Cancelled);
    }
    if rendered_count == 0 {
        // 所有章节都失败: 跟取消同等处理 — dir 还是空的, 清掉。
        cleanup_chapters_dir_if_empty(&chapters_dir);
        return Err(CrawlerError::EmptyToc);
    }

    // 6. 导出 (章节文件已在每个 task 中写入 chapters_dir)

    // 封面: 仅 EPUB 才下载; 失败 soft-skip。
    let cover_bytes = if matches!(render_target, RenderTarget::Epub) {
        download_cover(client, book.cover_url.as_deref()).await
    } else {
        None
    };

    let exporter = exporter_for(cfg.download.ext_name, &cfg.download.txt_encoding);
    let out_dir = std::path::Path::new(&cfg.download.download_path);
    let final_path =
        exporter.merge_with_cover(book, &chapters_dir, out_dir, cover_bytes.as_deref())?;

    // 7. 清理章节临时目录
    if !cfg.download.preserve_chapter_cache
        && let Err(e) = std::fs::remove_dir_all(&chapters_dir)
    {
        tracing::warn!(
            "清理章节缓存目录失败（已忽略）: {} — {e}",
            chapters_dir.display()
        );
    }

    let _ = progress.send(Progress::Finished {
        output_path: final_path.clone(),
    });
    if let Some(ref n) = notify {
        n();
    }

    // 终态日志已由 ops/download.rs 的 `下载任务终止 outcome=ok` 覆盖;
    // 这里留 debug 级用于排查"为什么声称 1454 章只渲染了 1400 章"之类的细节。
    // `book` / `chapters` 已在 `#[tracing::instrument]` 的 span 字段里 —— 不重复带。
    tracing::debug!(
        rendered = rendered_count,
        requested = chapter_count,
        output = %final_path.display(),
        "download_chapters: 渲染完成"
    );

    Ok(final_path)
}

/// 章节 task 的内部结果。
enum ChapterOutcome {
    Done,
    /// 失败原因已通过 `Progress::ChapterFailed` 推给 UI, 这里无需再带载荷。
    Failed,
    /// 取消时章节对象不再使用, 用 unit 变体。
    Cancelled,
}

/// 与 Java `Crawler` 中并发数计算一致:
/// - `concurrency = Some(n)`: 直接用, 但 ≤ 100;
/// - `None` (即 -1): 自动 = `min(50, toc.len())`;
/// - 任何情况下不超过章节数。
fn compute_concurrency(source: &Source, toc_len: usize) -> usize {
    let configured = source.effective_crawl.concurrency;
    let raw = match configured {
        Some(n) if n > 0 => n.min(100) as usize,
        _ => 50.min(toc_len.max(1)),
    };
    raw.min(toc_len.max(1)).max(1)
}

/// 取消 / 提前失败时清理 `chapters_dir`: **只删空目录**。
///
/// - 目录里已经写过章节文件 (用户成功跑过部分章节但中途取消, 下一轮"重试"想接着用)
///   或用户事先在那放了别的文件 → 保留;
/// - 目录是空的 (典型场景: 取消时还没来得及 `write_chapter_files`) → 删掉, 不留垃圾。
///
/// 用 `read_dir().next().is_none()` 判空, 比 `remove_dir_all` 安全得多。
/// 任何 IO 错误都只 `tracing::warn!` —— 清理失败不影响 cancel 主流程, 下次启动用户
/// 自己删也不痛。
fn cleanup_chapters_dir_if_empty(dir: &std::path::Path) {
    let Ok(mut entries) = std::fs::read_dir(dir) else {
        // 目录不存在 / 读不了 — 没什么可清的
        return;
    };
    if entries.next().is_some() {
        // 非空: 有人放过东西, 保留
        return;
    }
    // read_dir 拿到的迭代器在某些平台会持有目录句柄, 必须显式 drop 再 remove,
    // 否则 Windows 上偶发 "目录正在使用" 错误。
    drop(entries);
    if let Err(e) = std::fs::remove_dir(dir) {
        tracing::warn!("清理空章节目录失败（已忽略）: {} — {e}", dir.display());
    } else {
        tracing::debug!("已清理空章节目录: {}", dir.display());
    }
}

/// 下载封面字节 (EPUB 专属), 失败 soft-skip。
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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
        // 0 章节也至少给 1 (避免 Semaphore::new(0))
        assert_eq!(compute_concurrency(&s, 0), 1);
    }

    #[test]
    fn concurrency_respects_rule_override_and_caps_at_100() {
        let s = make_source(Some(5));
        assert_eq!(compute_concurrency(&s, 200), 5);

        let huge = make_source(Some(500));
        assert_eq!(compute_concurrency(&huge, 200), 100);

        // 即使配置了大值, 也不能超过 toc 长度
        let s2 = make_source(Some(50));
        assert_eq!(compute_concurrency(&s2, 3), 3);
    }
}
