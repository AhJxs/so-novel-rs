//! `search` 子命令：单源或聚合搜索，统一以人类可读 / JSON 两种格式输出。
//!
//! 进度：走 `search_streaming` + `tokio::spawn` 独立 task（生产者），主线程同步
//! 排空 mpsc 排空 outcomes（消费者）。两者并发，否则流式 = 批量。
//! 详见 [[streaming-search-lesson]]。

use std::io::{IsTerminal, Write, stderr};

use anyhow::{Context, Result};

use crate::config::{AppConfig, ConfigPaths};
use crate::crawler;
use crate::crawler::search::SourceSearchOutcome;
use crate::models::{SearchResult, Source};

use super::util::load_active_sources;

pub(crate) fn run_search(
    cfg: &AppConfig,
    paths: &ConfigPaths,
    keyword: String,
    source: Option<i32>,
    limit: Option<usize>,
    json: bool,
    quiet: bool,
) -> Result<()> {
    let sources = load_active_sources(cfg, paths)?;
    let target_sources: Vec<Source> = if let Some(id) = source {
        sources.into_iter().filter(|s| s.rule.id == id).collect()
    } else {
        sources
    };
    if target_sources.is_empty() {
        anyhow::bail!("没有可用的书源（检查 config.toml / rules 目录）");
    }
    // streaming task 把 target_sources move 走，先把总数存出来用于进度。
    let total_sources = target_sources.len();

    let cf_bypass = if cfg.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(cfg.cf_bypass.clone())
    };

    // 与 GUI（app/ops/search.rs:71-74）一致：--limit 优先；否则用 config.search_limit。
    let limit = limit.or_else(|| {
        cfg.search_limit
            .map(|v| v.max(0) as usize)
            .filter(|v| *v > 0)
    });

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("so-novel-cli")
        .build()
        .context("build tokio runtime")?;

    // CLI 临时构造共享 HTTP client 集合 —— 与 GUI AppModel::new 一致路径：
    // 工厂函数被 HttpClients 取代后，CLI 不能再直接调 build_async_client。
    // 一次构造、走所有 source（search_streaming 内部按 rule.ignore_ssl 选）。
    let http = std::sync::Arc::new(crate::http::HttpClients::new(cfg)?);

    // 生产者：tokio::spawn 独立 task 跑 streaming，否则主线程被它的 `.await` 阻塞
    // 后 mpsc 永远没机会排空，进度一次性跳到 N/N（[[streaming-search-lesson]] 警告）。
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SourceSearchOutcome>();
    let producer = rt.spawn({
        let http = std::sync::Arc::clone(&http);
        let kw = keyword.clone();
        async move {
            crawler::search::search_streaming(http, target_sources, kw, limit, cf_bypass, tx).await
        }
    });

    // 是否走原地刷新：TTY + 非 quiet。管道 / 重定向 / 静默模式退回逐行。
    let in_place = !quiet && stderr().is_terminal();
    let mut outcomes: Vec<SourceSearchOutcome> = Vec::with_capacity(total_sources);
    while let Some(outcome) = rt.block_on(async { rx.recv().await }) {
        let done = outcomes.len() + 1;
        // --json 模式不显示进度，保持 stdout 仅含 JSON。
        if !quiet && !json {
            if in_place {
                print_in_place(done, total_sources, &keyword);
            } else {
                eprintln!(
                    "  ✓ {done}/{total_sources} 源  {name}#{id}",
                    name = outcome.source_name,
                    id = outcome.source_id
                );
            }
        }
        outcomes.push(outcome);
    }
    rt.block_on(producer)
        .context("streaming search task join 失败")?;
    // in-place 进度行不留痕迹：清掉当前行（不写 \n），
    // 让结果列表紧跟在前一行输出之后，无空行 / 残留搜索提示。
    // 非 in-place 模式本就没在 stderr 上留进度，无需清理。
    if in_place {
        eprint!("\r\x1b[K");
        let _ = stderr().flush();
    }
    // 与 `search_aggregated` 行为对齐：按 source_id 升序，输出顺序稳定。
    outcomes.sort_by_key(|o| o.source_id);

    // 拍平成单一列表 + 收集失败源（保留每条的 source_id/source_name）。
    let mut flat: Vec<SearchResult> = Vec::new();
    let mut failed: Vec<(i32, String, String)> = Vec::new();
    for o in outcomes {
        match o.result {
            Ok(list) => flat.extend(list),
            Err(e) => failed.push((o.source_id, o.source_name, e.to_string())),
        }
    }

    // 与 GUI 一致：config.search_filter 为真时按相似度过滤 + 去重 + 排序。
    if cfg.search_filter {
        flat = crate::parser::filter_sort(&flat, &keyword);
    }

    if json {
        // 机器可读：仅 JSON 到 stdout，人类可读信息（失败源）走 stderr。
        println!("{}", serde_json::to_string(&flat)?);
        print_failed_sources(&failed, quiet);
        drop(rt);
        return Ok(());
    }

    // 人类可读：单一列表 + 全局序号。
    for (i, r) in flat.iter().enumerate() {
        println!(
            "{}. {}  作者:{}  最新:{}  [{}#{}]  {}",
            i + 1,
            r.book_name,
            r.author.as_deref().unwrap_or("-"),
            r.latest_chapter.as_deref().unwrap_or("-"),
            r.source_name,
            r.source_id,
            r.url
        );
    }
    print!("\n共 {} 条结果（关键词：{keyword}）", flat.len());
    if failed.is_empty() {
        println!();
    } else {
        let summary = failed
            .iter()
            .map(|(id, name, _)| format!("{name}#{id}"))
            .collect::<Vec<_>>()
            .join(", ");
        println!("  失败源: {summary}");
    }
    // 失败源详情走 stderr，不污染 stdout 纯结果区。
    print_failed_sources(&failed, quiet);

    // Windows console 坑：in-place 进度用 `\r\x1b[K` 改写光标位置后，
    // 程序退出时 cmd/PowerShell 不会自动重绘 prompt（要按一次 Enter）。
    // 在末尾补一个 \n，把光标推到下一行，让 shell prompt 立刻回来。
    // 非 in-place 模式光标本来就干净，跳过。
    if in_place {
        eprintln!();
    }
    drop(rt);
    Ok(())
}

/// 把失败源写到 stderr。
/// - `quiet=true` → 完全跳过（脚本管道友好）。
/// - 失败 ≤3 → 逐条打印 `✗ name#id 失败: err`。
/// - 失败 >3 → 前 3 条逐条打印 + `… 还有 N 条失败` 摘要，避免刷屏。
fn print_failed_sources(failed: &[(i32, String, String)], quiet: bool) {
    if quiet || failed.is_empty() {
        return;
    }
    const TOP_N: usize = 3;
    for (id, name, err) in failed.iter().take(TOP_N) {
        eprintln!("✗ {name}#{id} 失败: {err}");
    }
    if failed.len() > TOP_N {
        eprintln!(
            "… 还有 {} 条失败（用 --source 单源排查）",
            failed.len() - TOP_N
        );
    }
}

/// TTY 模式下原地单行搜索进度。
fn print_in_place(done: usize, total: usize, keyword: &str) {
    let kw_short = crate::utils::formatting::truncate(keyword, 16 + 1);
    crate::utils::tty::print_in_place_line(
        "🔍 搜索中…",
        done as u64,
        total,
        &format!("关键词:《{kw_short}》"),
    );
}
