//! 下载调度层。对应 Java `core.Crawler` + `parse.ChapterParser` 的重试逻辑
//! + `handle.CrawlerPostHandler` 的导出+清理逻辑。
//!
//! 入口：`download_book(cfg, source, book_url, opts) -> Result<PathBuf, CrawlerError>`。
//!
//! 流程：
//! 1. `parse_book_detail` 拿到详情；
//! 2. `parse_toc` 拿到完整章节列表；
//! 3. 用 tokio Semaphore 限并发；每章 spawn async task 调
//!    `parse_chapter` + `render_chapter`，失败按 `EffectiveCrawl::max_retries` 重试；
//! 4. 章节字符串写入 `<download_path>/<bookDirName>/`；
//! 5. EPUB 时下载封面（soft-skip），其它格式不下载；
//! 6. `Exporter::merge_with_cover` 产出最终文件；
//! 7. `preserve_chapter_cache=false` 时清掉 chapters 临时目录。
//!
//! 进度：通过 `mpsc::UnboundedSender<Progress>` 推送事件给 UI；`events::drain` 排空。
//!
//! 取消：`Arc<AtomicBool>`；在每章入口检查；正在跑的章节会跑完才退出
//! （非 hyper 连接级中断；当前用 JoinHandle::abort + CancelToken 轮询实现任务级取消）。

pub mod cover_updater;
pub mod health;
mod retry;
pub mod search;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rand::RngExt;
use thiserror::Error;
use tokio::sync::{Notify, Semaphore, mpsc};

use crate::config::AppConfig;
use crate::export::{
    ExportError, RenderTarget, RenderedChapter, build_book_dir_name, exporter_for,
    write_chapter_files,
};
use crate::models::{Book, Chapter};
use crate::parser::{
    BookError, ChapterError, SelectError, TocError, parse_book_detail, parse_chapter, parse_toc,
};
use crate::rules::Source;
use crate::util::zhconv::convert_book_meta;
use retry::retry_with_backoff;

/// 调度层用户可见的进度事件。
#[derive(Debug, Clone)]
pub enum Progress {
    /// 详情解析完成，得到书籍元信息。
    BookResolved {
        book: Box<Book>,
        total_chapters: usize,
    },
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
    /// 下载失败（详情/目录/写盘/导出等终态错误）。让 UI 能区分"取消"与"失败"。
    Failed { reason: String },
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
///
/// 内部同时持有 `AtomicBool`（同步检查）和 `tokio::sync::Notify`（异步唤醒）。
/// `cancel()` 设置 flag 并唤醒所有 `wait_cancelled()` 等待者，响应 <1ms。
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
    /// 异步等待取消信号。比 50ms poll 循环快得多（<1ms 响应）。
    pub async fn wait_cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }
}

impl Clone for CancelToken {
    fn clone(&self) -> Self {
        Self {
            flag: Arc::clone(&self.flag),
            notify: Arc::clone(&self.notify),
        }
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
    client: &reqwest::Client,
    source: &Source,
    book_url: &str,
    opts: DownloadOptions,
) -> Result<PathBuf, CrawlerError> {
    let cancel = opts.cancel.clone();
    let progress = opts.progress.clone();

    let started = std::time::Instant::now();
    tracing::info!(
        source_id = source.rule.id,
        source = %source.rule.name,
        book_url = %book_url,
        "download_book: 开始",
    );

    let (book, toc) = resolve_book(cfg, client, source, book_url, &cancel).await?;

    if toc.is_empty() {
        tracing::warn!(
            source_id = source.rule.id,
            book_url = %book_url,
            "download_book: 章节列表为空",
        );
        return Err(CrawlerError::EmptyToc);
    }

    let _ = progress.send(Progress::BookResolved {
        book: Box::new(book.clone()),
        total_chapters: toc.len(),
    });

    if cancel.is_cancelled() {
        let _ = progress.send(Progress::Cancelled);
        return Err(CrawlerError::Cancelled);
    }

    let out = download_chapters(cfg, client, source, book_url, &book, toc, opts).await;
    match &out {
        Ok(path) => tracing::info!(
            source_id = source.rule.id,
            book = %book.book_name,
            output = %path.display(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "download_book: 完成",
        ),
        Err(e) => tracing::warn!(
            source_id = source.rule.id,
            book = %book.book_name,
            elapsed_ms = started.elapsed().as_millis() as u64,
            error = %format!("{e:#}"),
            "download_book: 失败",
        ),
    }
    out
}

/// 阶段一：获取书籍元信息 + 章节列表（不开始下载）。
///
/// 返回 `(Book, Vec<Chapter>)`，供 UI 层展示章节列表、让用户选择范围后再调用
/// `download_chapters`。
pub async fn resolve_book(
    cfg: &AppConfig,
    client: &reqwest::Client,
    source: &Source,
    book_url: &str,
    cancel: &CancelToken,
) -> Result<(Book, Vec<Chapter>), CrawlerError> {
    let started = std::time::Instant::now();
    tracing::info!(source_id = source.rule.id, source = %source.rule.name, book_url = %book_url, "resolve_book: 抓取详情 + 章节列表");

    let cf_bypass = if cfg.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(cfg.cf_bypass.as_str())
    };

    // 全局起点 cookie（整段粘贴），仅供详情页末尾的 CoverUpdater 用：
    // 详情页 fetch 本身不附 Cookie，与 Java 端语义一致。
    let qidian_cookie = if cfg.qidian_cookie.trim().is_empty() {
        None
    } else {
        Some(cfg.qidian_cookie.as_str())
    };

    let rule = source.rule.clone();
    let book_url_owned = book_url.to_string();
    let cf_bypass_owned: Option<Arc<str>> = cf_bypass.map(Arc::from);
    let qidian_cookie_owned = qidian_cookie.map(String::from);
    let eff = source.effective_crawl.clone();
    let max_attempts = if cfg.enable_retry { eff.max_retries } else { 0 };
    // 两次 retry 各需一份 eff（sleep_fn 是 move 闭包）。
    let eff_for_book = eff.clone();
    let eff_for_toc = eff;

    // 详情：包重试（与章节抓取同款 enable_retry 门控）。详情页一次瞬态 HTTP 抖动
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

    // 目录：同样包重试。
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

    // 书名/作者/简介 简繁转换：与章节正文 (`render_chapter`) 用同一 source/target 语义。
    // 在这里集中做一次，UI（任务页 / 书库）、缓存目录名 (`build_book_dir_name`)、
    // 导出器（EPUB metadata + 文件名 + TXT 首页 + HTML zip 名）全部共享转换结果，
    // 避免每个 exporter 各自重写一份 + 减少 drift 风险。
    //
    // 目标语言从界面语言 (`Language`) 推 —— 合并 UI/书源语言设置后用户只设一个
    // Language，下载时的简繁转换目标即由此决定（English → ZhCn 兜底）。
    let target_lang = cfg.language.to_book_target_lang();
    let book = convert_book_meta(&book, &source.rule.language, &target_lang);

    tracing::info!(source_id = source.rule.id, book = %book.book_name, chapters = toc.len(), elapsed_ms = started.elapsed().as_millis() as u64, "resolve_book: 完成");

    Ok((book, toc))
}

/// 阶段二：下载指定章节 + 导出。
///
/// `chapters` 已由调用方按用户选择过滤过范围。`DownloadOptions` 中需传入
/// progress sender 和 cancel token。
pub async fn download_chapters(
    cfg: &AppConfig,
    client: &reqwest::Client,
    source: &Source,
    _book_url: &str,
    book: &Book,
    chapters: Vec<Chapter>,
    opts: DownloadOptions,
) -> Result<PathBuf, CrawlerError> {
    let DownloadOptions { progress, cancel } = opts;

    let started = std::time::Instant::now();
    tracing::info!(source_id = source.rule.id, book = %book.book_name, chapters = chapters.len(), max_concurrent = compute_concurrency(source, chapters.len()), "download_chapters: 开始");

    if chapters.is_empty() {
        return Err(CrawlerError::EmptyToc);
    }

    let cf_bypass = if cfg.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(cfg.cf_bypass.as_str())
    };

    let rule = Arc::new(source.rule.clone());
    let cf_bypass_owned: Option<Arc<str>> = cf_bypass.map(Arc::from);

    // 准备 chapters 目录
    let book_dir_name = build_book_dir_name(book, cfg.ext_name);
    let chapters_dir = std::path::Path::new(&cfg.download_path).join(&book_dir_name);
    std::fs::create_dir_all(&chapters_dir)?;

    // 并发抓章节
    let max_concurrent = compute_concurrency(source, chapters.len());
    let render_target: RenderTarget = cfg.ext_name.into();
    let rule_chapter = Arc::new(
        rule.chapter
            .clone()
            .ok_or(CrawlerError::Toc(TocError::TocRuleMissing))?,
    );

    let semaphore = Arc::new(Semaphore::new(max_concurrent));
    let eff = source.effective_crawl.clone();
    let mut handles = Vec::with_capacity(chapters.len());
    let chapter_count = chapters.len();

    for chapter in chapters {
        if cancel.is_cancelled() {
            // dispatch 阶段被取消：dir 里还没写过任何章节文件，安全清理。
            cleanup_chapters_dir_if_empty(&chapters_dir);
            let _ = progress.send(Progress::Cancelled);
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
        let enable_retry = cfg.enable_retry;
        // 简繁转换需要源/目标语言。`source` / `cfg` 是借用，闭包 'static 要求 owned
        // 值；clone Rule（Arc 浅拷贝，开销小）和 target LangType（Copy）。
        // 目标语言从界面语言 (`Language`) 推导 —— 合并设置后用户只设 Language。
        let rule_lang = source.rule.language.clone();
        let target_lang = cfg.language.to_book_target_lang();

        handles.push(tokio::spawn(async move {
            // 限并发。`acquire_owned` 当前 tokio 实现在 Semaphore 不被 close 的情况
            // 下永远成功，但 API 返回 Result；防御性：万一未来切换实现/close 信号进来，
            // 把这一章记为 Failed 而不是 panic 把整个下载任务搞炸。
            let _permit = match permit.acquire_owned().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("semaphore acquire 失败: {e}");
                    return ChapterOutcome::Failed;
                }
            };

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
                _ = cancel.wait_cancelled() => return ChapterOutcome::Cancelled,
                _ = tokio::time::sleep(Duration::from_millis(interval)) => {}
            }

            if cancel.is_cancelled() {
                return ChapterOutcome::Cancelled;
            }

            // 抓 + 重试
            let max_attempts = if enable_retry { eff.max_retries } else { 0 };
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
                        //       ^^^^^^^ Arc<Rule> 自动 deref coercion 到 &Rule
                    }
                },
                max_attempts,
                // 重试间隔用 random_retry_interval_ms（同时受 retry-min / retry-max 约束，
                // 与 Java 端 randomInterval(config, true) 一致）。原 linear_backoff 不读
                // retry-max，会让 retry-max-interval 配置形同虚设。
                move |_attempt| {
                    let dur = Duration::from_millis(crate::http::random_retry_interval_ms(&eff));
                    async move {
                        tokio::time::sleep(dur).await;
                    }
                },
            );

            let result = tokio::select! {
                biased;
                _ = cancel.wait_cancelled() => {
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
                        &target_lang,
                    );
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
                    let reason = format!("{e:#}");
                    tracing::warn!(order = order, title = %title, error = %reason, "章节解析失败");
                    let _ = progress.send(Progress::ChapterFailed {
                        index: order,
                        title: title.clone(),
                        reason,
                    });
                    ChapterOutcome::Failed
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
                Ok(ChapterOutcome::Failed) => {}
                Ok(ChapterOutcome::Cancelled) => {}
                Err(join_err) => {
                    tracing::warn!("章节任务 join 失败: {join_err}");
                }
            }
        }
    };

    tokio::select! {
        biased;
        _ = cancel.wait_cancelled() => {
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
        download_cover(client, book.cover_url.as_deref()).await
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

    let rendered_count = rendered.len();
    tracing::info!(
        source_id = source.rule.id,
        book = %book.book_name,
        rendered = rendered_count,
        requested = chapter_count,
        output = %final_path.display(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "download_chapters: 完成",
    );

    Ok(final_path)
}

enum ChapterOutcome {
    Done(RenderedChapter),
    /// 失败原因已通过 `Progress::ChapterFailed` 推给 UI，这里无需再带载荷。
    Failed,
    /// 取消时章节对象不再使用，用 unit 变体。
    Cancelled,
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
        tracing::warn!("清理空章节目录失败（已忽略）: {} — {e}", dir.display());
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

    #[tokio::test]
    async fn wait_cancelled_immediate_after_cancel() {
        let t = CancelToken::new();
        t.cancel();
        // 已经 cancel → wait_cancelled 应立即返回，不阻塞
        let start = std::time::Instant::now();
        t.wait_cancelled().await;
        assert!(
            start.elapsed() < Duration::from_millis(10),
            "should return immediately"
        );
    }

    #[tokio::test]
    async fn wait_cancelled_waits_for_cancel_signal() {
        let t = CancelToken::new();
        let t2 = t.clone();
        let handle = tokio::spawn(async move {
            t2.wait_cancelled().await;
        });
        // 等一小会儿再 cancel
        tokio::time::sleep(Duration::from_millis(10)).await;
        t.cancel();
        // handle 应该很快完成（< 100ms）
        let result = tokio::time::timeout(Duration::from_millis(100), handle).await;
        assert!(
            result.is_ok(),
            "wait_cancelled should complete promptly after cancel"
        );
    }
}
