//! `download` 子命令: 单本书下载, stderr 实时打印进度。
//!
//! TTY 下用 `\r\x1b[K` 原地刷新单行进度; 管道 / 重定向退回逐行打印 (可 grep);
//! `--quiet` 只留终态汇总。SIGINT 接到 `CancelToken`, 让 crawler 干净退出而非硬杀。
//! 带 `--from` / `--to` 走 `resolve_book` + 切片 + `download_chapters`, 否则走全本路径。

use std::io::{IsTerminal, stderr};

use anyhow::{Context, Result};

use crate::config::{AppConfig, ConfigPaths};
use crate::core::bootstrap::{effective_cfg, load_active_sources, validate_range};
use crate::core::{search as core_search, sources as core_sources};
use crate::crawler::{self, CancelToken, CrawlerError, Progress, download_chapters, resolve_book};
use crate::models::{Chapter, Source};
use crate::utils::system::open_path;

use super::build_cli_runtime;
use super::print_progress_line;

/// 单行进度模板里最多保留多少字符的章节标题（防止刷屏）。
const TITLE_DISPLAY_MAX: usize = 24;

// 9 个参数全部由 clap 子命令字段 1:1 透传 (`mod.rs` 的 match 分支也按字段顺序解构),
// 抽成 `DownloadArgs` 只是把列表从签名挪到字段、纯增加 diff; 目前没有第三个调用点。
// 与 GUI 侧 `spawn_download_range` 同款取舍。
#[allow(clippy::too_many_arguments)]
pub fn run_download(
    cfg: &AppConfig,
    paths: &ConfigPaths,
    url: String,
    source: Option<i32>,
    output: Option<String>,
    format: Option<String>,
    from: Option<usize>,
    to: Option<usize>,
    quiet: bool,
) -> Result<()> {
    let cfg = effective_cfg(cfg.clone(), output, format);
    let rules = load_active_sources(paths)?;
    if rules.is_empty() {
        anyhow::bail!("没有可用的书源");
    }
    let chosen: Source = if let Some(id) = source {
        core_search::select_sources(&rules, &cfg, Some(id))
            .into_iter()
            .next()
            .with_context(|| format!("找不到 ID={id} 的书源"))?
    } else {
        // 按 URL origin 自动匹配 (匹配逻辑收敛在 `core::sources::match_source_by_url`, 与
        // web / 桌面复用)。这里临时 clone 成 `Vec<Source>`: CLI 启动一次、N ≤ 20, 开销可忽略。
        let sources: Vec<Source> = rules
            .iter()
            .cloned()
            .map(|r| Source::from(r, &cfg))
            .collect();
        match core_sources::match_source_by_url(&sources, &url) {
            Some(s) => s.clone(),
            None => Source::from(rules.into_iter().next().context("没有可用的书源")?, &cfg),
        }
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Progress>();
    let cancel = CancelToken::new();
    let opts = crawler::DownloadOptions {
        progress: tx,
        cancel: cancel.clone(),
        notify: None,
    };

    // 是否走范围下载路径: range producer 自己会打印全本 + 范围, consumer 据此跳过"共 N 章"。
    let is_range = from.is_some() || to.is_some();

    let cfg_for_task = cfg;
    let url_for_task = url;
    let source_for_task = chosen;
    let rt = build_cli_runtime()?;

    // 共享 HTTP client 集合 (与 search 子命令同款); `download_book` 内部按
    // `source.rule.ignore_ssl` 选 safe / unsafe_ssl 通道。
    let http_for_task = std::sync::Arc::new(crate::http::HttpClients::new(&cfg_for_task)?);

    let download_task = if is_range {
        // 范围路径：先解析 TOC → 切片 → 走 download_chapters
        let http_for_task = std::sync::Arc::clone(&http_for_task);
        rt.spawn(async move {
            let client = http_for_task.for_rule(&source_for_task.rule);
            run_range_download(
                &cfg_for_task,
                &client,
                &source_for_task,
                &url_for_task,
                from,
                to,
                opts,
            )
            .await
        })
    } else {
        // 全本路径：直接 download_book（与历史行为完全一致）
        let http_for_task = std::sync::Arc::clone(&http_for_task);
        rt.spawn(async move {
            let client = http_for_task.for_rule(&source_for_task.rule);
            crawler::download_book(
                &cfg_for_task,
                &client,
                &source_for_task,
                &url_for_task,
                opts,
            )
            .await
        })
    };

    // TTY 探测一次性做: Ctrl-C 注册与 in_place 进度都依赖它 (管道 / 重定向退回逐行)。
    let stderr_is_tty = stderr().is_terminal();

    // Ctrl-C → cancel: 让 crawler 走 Cancelled 干净退出而非硬杀; 仅在 TTY 下注册
    // (管道 / 后台运行时 Ctrl-C 通常发给父 shell)。
    if stderr_is_tty {
        let cancel_for_signal = cancel;
        rt.spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancel_for_signal.cancel();
                eprintln!("\n⚠ 收到 Ctrl-C，正在取消…");
            }
        });
    }

    let in_place = !quiet && stderr_is_tty;
    let mut last_completed: u32 = 0;
    let mut total_chapters: usize = 0;
    let mut rx = rx;
    let mut saw_cancelled = false;
    while let Some(ev) = rt.block_on(async { rx.recv().await }) {
        match ev {
            Progress::BookResolved {
                book,
                total_chapters: total,
            } => {
                total_chapters = total;
                // range 路径 producer 已打印过全本 + 范围, 这里跳过避免重复行。
                if !is_range {
                    eprintln!(
                        "《{}》by {} — 共 {total_chapters} 章",
                        book.book_name, book.author
                    );
                }
            }
            Progress::ChapterDone { index, title } => {
                // 非 in-place 去重 (crawler 重试同一章会重发事件)。
                let is_new = index != last_completed;
                last_completed = index;
                if in_place {
                    print_in_place(last_completed, total_chapters, &title);
                } else if !quiet && is_new {
                    eprintln!("  ✓ 第 {index} 章 《{title}》");
                }
            }
            Progress::ChapterFailed {
                index,
                title,
                reason,
            } => {
                if !quiet {
                    if in_place {
                        // 失败行换行打印 (不抢进度行), 下一次 ChapterDone 会重写进度行。
                        eprintln!("\r  ✗ 第 {index} 章 《{title}》 — {reason}\x1b[K");
                    } else {
                        eprintln!("  ✗ 第 {index} 章 《{title}》 — {reason}");
                    }
                }
            }
            Progress::Finished { output_path } => {
                if in_place {
                    // 关闭进度行：写新行收尾。
                    eprintln!();
                }
                eprintln!("\n✅ 已生成: {}", output_path.display());
                let _ = open_path(&output_path);
                break;
            }
            Progress::Cancelled => {
                saw_cancelled = true;
                if in_place {
                    eprintln!();
                }
                eprintln!("\n⚠ 已取消");
                break;
            }
            Progress::Failed { reason } => {
                if in_place {
                    eprintln!();
                }
                eprintln!("\n❌ 下载失败: {reason}");
                break;
            }
        }
    }

    let result = rt.block_on(download_task).context("下载任务 join 失败")?;
    if let Err(e) = result
        && !saw_cancelled
    {
        eprintln!("\n❌ 下载失败: {e:#}");
        std::process::exit(1);
    }
    drop(rt);
    Ok(())
}

/// 范围下载: 先 `resolve_book` 拿全 TOC, 按 from/to 切片, 再走 `download_chapters`。
///
/// 与 `crawler::download_book` (全本) 的区别: 多一次切片; `BookResolved` 带的是**切片后**
/// 章数 (让百分比准确); 切片前先打印"全 M 章, 下载 A-B"让用户看到全本规模。
async fn run_range_download(
    cfg: &AppConfig,
    client: &reqwest::Client,
    source: &crate::models::Source,
    book_url: &str,
    from: Option<usize>,
    to: Option<usize>,
    opts: crawler::DownloadOptions,
) -> Result<std::path::PathBuf, CrawlerError> {
    let cancel = opts.cancel.clone();
    let progress = opts.progress.clone();

    let (book, toc) = resolve_book(cfg, client, source, book_url, &cancel).await?;
    if toc.is_empty() {
        return Err(CrawlerError::EmptyToc);
    }
    let total_book = toc.len();
    let (from, to) = validate_range(from, to, total_book)
        .map_err(|e| CrawlerError::InvalidRange(e.to_string()))?;
    let count = to - from + 1;

    // 打印"全本 + 范围"上下文（consumer 收到 BookResolved 时会跳过它的"共 N 章"行）。
    eprintln!(
        "📖 《{}》by {} — 全 {total_book} 章，下载 {from}-{to}（共 {count} 章）",
        book.book_name, book.author
    );

    let mut filtered: Vec<Chapter> = Vec::with_capacity(to - from + 1);
    for c in toc {
        let order = c.order as usize;
        if order >= from && order <= to {
            filtered.push(c);
        }
    }
    if filtered.is_empty() {
        // 理论上 validate_range 通过后 filtered 不会空；防御性兜底。
        return Err(CrawlerError::EmptyToc);
    }

    let _ = progress.send(Progress::BookResolved {
        book: Box::new(book.clone()),
        total_chapters: filtered.len(),
    });

    if cancel.is_cancelled() {
        let _ = progress.send(Progress::Cancelled);
        return Err(CrawlerError::Cancelled);
    }

    download_chapters(cfg, client, source, &book, filtered, opts).await
}

/// TTY 模式下的原地单行下载进度。
fn print_in_place(done: u32, total: usize, latest_title: &str) {
    let title_short = crate::utils::formatting::truncate(latest_title, TITLE_DISPLAY_MAX + 1);
    print_progress_line("⏳ 已完成", done, total, &format!("最新:《{title_short}》"));
}
