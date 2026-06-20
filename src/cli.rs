//! CLI 子命令（5c）。复用现有 parser/crawler，跑在 `#[tokio::main]` runtime。
//!
//! 用法：
//! - `so-novel-rs search <关键词> [--source ID] [--json]` — 聚合或单源搜索
//! - `so-novel-rs download <详情页 URL> [--output DIR] [--format epub|txt|...]`
//! - `so-novel-rs sources [--json]` — 列出当前激活书源
//! - `so-novel-rs --version` / `-V` — 版本（clap 自动注入）
//!
//! 不带子命令 → 启动 GPUI GUI（见 `main.rs` 的分发逻辑）。

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::config::{AppConfig, ConfigPaths, ExportFormat, load_config};
use crate::crawler::{self, CancelToken, Progress};
use crate::db::Db;
use crate::models::{Rule, SearchResult};
use crate::rules::{Source, load_rules_from_db};
use crate::util::system::open_path;

/// so-novel-rs — 小说下载器（CLI）。
#[derive(Debug, Parser)]
#[command(
    name = "so-novel-rs",
    about = "So Novel — 简繁小说批量下载",
    long_about = None,
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// 搜索书源。默认聚合搜索；--source 指定单源。
    Search {
        /// 关键词（书名 / 作者）
        keyword: String,
        /// 指定书源 ID；省略则聚合所有启用书源
        #[arg(long)]
        source: Option<i32>,
        /// 每源最多返回条数（覆盖 config.toml 的 search-limit）
        #[arg(long)]
        limit: Option<usize>,
        /// 输出 JSON 到 stdout（机器可读，禁用人类可读格式）
        #[arg(long)]
        json: bool,
    },
    /// 通过详情页 URL 下载整本书
    Download {
        /// 详情页 URL
        url: String,
        /// 书源 ID（默认按 URL 自动匹配；未匹配则取第一个启用的源）
        #[arg(long)]
        source: Option<i32>,
        /// 覆盖 config.toml 的下载目录
        #[arg(long)]
        output: Option<String>,
        /// 覆盖 config.toml 的输出格式（epub / txt / html）
        #[arg(long)]
        format: Option<String>,
    },
    /// 列出当前激活书源
    Sources {
        /// 输出 JSON 到 stdout（机器可读，禁用人类可读格式）
        #[arg(long)]
        json: bool,
    },
}

/// CLI 入口。被 `main.rs` 在检测到子命令时调用。
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = ConfigPaths::discover();
    let cfg = load_config(&paths.config_file).context("加载 config.toml 失败")?;

    // 与 GUI 启动行为保持一致：首次运行时把默认 config 写出去，
    // 用户立刻能在项目根看到 config.toml 可编辑。失败仅警告，不阻塞 CLI。
    if !paths.config_file.exists() {
        if let Err(e) = crate::config::save_config(&paths.config_file, &cfg) {
            tracing::warn!("写入默认 config.toml 失败: {e:#}");
        } else {
            tracing::info!("首次运行：已生成 {}", paths.config_file.display());
        }
    }

    match cli.command {
        Cmd::Search {
            keyword,
            source,
            limit,
            json,
        } => run_search(&cfg, &paths, keyword, source, limit, json),
        Cmd::Download {
            url,
            source,
            output,
            format,
        } => run_download(&cfg, &paths, url, source, output, format),
        Cmd::Sources { json } => run_sources(&cfg, &paths, json),
    }
}

fn effective_cfg(cfg: AppConfig, output: Option<String>, format: Option<String>) -> AppConfig {
    let mut cfg = cfg;
    if let Some(o) = output {
        cfg.download_path = o;
    }
    if let Some(f) = format {
        cfg.ext_name = ExportFormat::parse(&f);
    }
    cfg
}

fn load_active_sources(cfg: &AppConfig, paths: &ConfigPaths) -> Result<Vec<Source>> {
    let mut db = Db::open(&paths.db_file)
        .with_context(|| format!("打开 sonovel.db 失败: {}", paths.db_file.display()))?;
    let rules = load_rules_from_db(db.conn_mut()).context("加载规则失败")?;
    Ok(rules
        .into_iter()
        .filter(|r| !r.disabled)
        .map(|r| Source::from(r, cfg))
        .collect())
}

fn run_search(
    cfg: &AppConfig,
    paths: &ConfigPaths,
    keyword: String,
    source: Option<i32>,
    limit: Option<usize>,
    json: bool,
) -> Result<()> {
    let sources = load_active_sources(cfg, paths)?;
    let target_sources: Vec<Source> = if let Some(id) = source {
        sources.into_iter().filter(|s| s.rule.id == id).collect()
    } else {
        sources
    };
    if target_sources.is_empty() {
        anyhow::bail!("没有可用的书源（检查 config.toml / sonovel.db）");
    }

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
    // 一次构造、走所有 source（search_aggregated 内部按 rule.ignore_ssl 选）。
    let http = std::sync::Arc::new(crate::http::HttpClients::new(cfg)?);

    let outcomes = rt.block_on(crawler::search::search_aggregated(
        cfg,
        http,
        target_sources,
        keyword.clone(),
        limit,
        cf_bypass,
    ));

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
        for (id, name, err) in &failed {
            eprintln!("✗ {name}#{id} 失败: {err}");
        }
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
    for (id, name, err) in &failed {
        eprintln!("✗ {name}#{id} 失败: {err}");
    }
    drop(rt);
    Ok(())
}

fn run_download(
    cfg: &AppConfig,
    paths: &ConfigPaths,
    url: String,
    source: Option<i32>,
    output: Option<String>,
    format: Option<String>,
) -> Result<()> {
    let cfg = effective_cfg(cfg.clone(), output, format);
    let sources = load_active_sources(&cfg, paths)?;
    if sources.is_empty() {
        anyhow::bail!("没有可用的书源");
    }
    let chosen = match source {
        Some(id) => sources
            .into_iter()
            .find(|s| s.rule.id == id)
            .with_context(|| format!("找不到 ID={id} 的书源"))?,
        None => {
            // 默认第一个启用的源
            sources.into_iter().next().context("没有可用的书源")?
        }
    };

    // cfg.cf_bypass 由 crawler::download_book 内部读取；这里不重复计算。

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Progress>();
    let cancel = CancelToken::new();
    let opts = crawler::DownloadOptions {
        progress: tx,
        cancel: cancel.clone(),
    };

    // 后台跑下载，主线程排空进度打印到 stderr。
    let cfg_for_task = cfg.clone();
    let url_for_task = url.clone();
    let source_for_task = chosen.clone();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("so-novel-cli")
        .build()
        .context("build tokio runtime")?;

    // CLI 临时构造共享 HTTP client 集合（与 search 子命令同款），由
    // download_book 内部按 source.rule.ignore_ssl 选 safe/unsafe_ssl 通道。
    let http_for_task = std::sync::Arc::new(crate::http::HttpClients::new(&cfg_for_task)?);

    let download_task = rt.spawn(async move {
        let client = http_for_task.for_rule(&source_for_task.rule);
        crawler::download_book(
            &cfg_for_task,
            &client,
            &source_for_task,
            &url_for_task,
            opts,
        )
        .await
    });

    let mut last_completed: u32 = 0;
    let mut rx = rx;
    while let Some(ev) = rt.block_on(async { rx.recv().await }) {
        match ev {
            Progress::BookResolved {
                book,
                total_chapters,
            } => {
                eprintln!(
                    "《{}》by {} — 共 {total_chapters} 章",
                    book.book_name, book.author
                );
            }
            Progress::ChapterDone { index, title } => {
                if index != last_completed {
                    eprintln!("  ✓ 第 {index} 章 《{title}》");
                    last_completed = index;
                }
            }
            Progress::ChapterFailed {
                index,
                title,
                reason,
            } => {
                eprintln!("  ✗ 第 {index} 章 《{title}》 — {reason}");
            }
            Progress::Finished { output_path } => {
                eprintln!("\n✅ 已生成: {}", output_path.display());
                let _ = open_path(&output_path);
                break;
            }
            Progress::Cancelled => {
                eprintln!("\n⚠ 已取消");
                break;
            }
            Progress::Failed { reason } => {
                eprintln!("\n❌ 下载失败: {reason}");
                break;
            }
        }
    }

    let result = rt.block_on(download_task).context("下载任务 join 失败")?;
    if let Err(e) = result {
        eprintln!("\n❌ 下载失败: {e:#}");
        std::process::exit(1);
    }
    drop(rt);
    Ok(())
}

fn run_sources(cfg: &AppConfig, paths: &ConfigPaths, json: bool) -> Result<()> {
    let _ = cfg; // 当前未用到 cfg 字段，保留参数为未来扩展（按 lang 过滤等）
    let mut db = Db::open(&paths.db_file)
        .with_context(|| format!("打开 sonovel.db 失败: {}", paths.db_file.display()))?;
    let rules: Vec<Rule> = load_rules_from_db(db.conn_mut()).context("加载规则失败")?;

    if json {
        // 机器可读：Rule 已 derive(Serialize)。
        println!("{}", serde_json::to_string(&rules)?);
        return Ok(());
    }

    let enabled = rules.iter().filter(|r| !r.disabled).count();
    let disabled = rules.iter().filter(|r| r.disabled).count();
    println!(
        "书源数据库: {}（启用 {} / 禁用 {}）",
        paths.db_file.display(),
        enabled,
        disabled
    );
    println!();
    for r in &rules {
        let mark = if r.disabled { "✗" } else { "✓" };
        let proxy = if r.need_proxy { " [proxy]" } else { "" };
        let lang = if !r.language.is_empty() {
            format!(" [{}]", r.language)
        } else {
            String::new()
        };
        let search = if r.search.as_ref().map(|s| !s.disabled).unwrap_or(false) {
            " [search]"
        } else {
            ""
        };
        println!(
            "  {mark} #{:>3} {}{}{}{}  {}",
            r.id, r.name, proxy, lang, search, r.url
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_rejects_version_subcommand() {
        // version 子命令已移除，改用 clap 自动注入的 -V / --version。
        let result = Cli::try_parse_from(["so-novel-rs", "version"]);
        assert!(result.is_err(), "version 应不再是子命令");
    }

    #[test]
    fn cli_accepts_version_flag() {
        // clap 自动注入的 --version flag：解析应在拿到 flag 时返回 Err
        // （DisplayVersion），而非正常子命令。
        let result = Cli::try_parse_from(["so-novel-rs", "--version"]);
        assert!(result.is_err(), "--version flag 应触发 DisplayVersion");
    }

    #[test]
    fn cli_parses_sources_subcommand() {
        let cli = Cli::try_parse_from(["so-novel-rs", "sources"]).unwrap();
        match cli.command {
            Cmd::Sources { json } => assert!(!json),
            _ => panic!("expected Sources"),
        }
    }

    #[test]
    fn cli_parses_search_with_keyword() {
        let cli = Cli::try_parse_from(["so-novel-rs", "search", "凡人修仙传"]).unwrap();
        match cli.command {
            Cmd::Search {
                keyword,
                source,
                limit,
                json,
            } => {
                assert_eq!(keyword, "凡人修仙传");
                assert_eq!(source, None);
                assert_eq!(limit, None);
                assert!(!json);
            }
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn cli_parses_search_with_source_and_limit() {
        let cli = Cli::try_parse_from([
            "so-novel-rs",
            "search",
            "斗破苍穹",
            "--source",
            "3",
            "--limit",
            "10",
        ])
        .unwrap();
        match cli.command {
            Cmd::Search {
                keyword,
                source,
                limit,
                json,
            } => {
                assert_eq!(keyword, "斗破苍穹");
                assert_eq!(source, Some(3));
                assert_eq!(limit, Some(10));
                assert!(!json);
            }
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn cli_parses_search_json_flag() {
        let cli = Cli::try_parse_from(["so-novel-rs", "search", "凡人修仙传", "--json"]).unwrap();
        match cli.command {
            Cmd::Search { json, .. } => assert!(json),
            _ => panic!("expected Search"),
        }
    }

    #[test]
    fn cli_parses_sources_json_flag() {
        let cli = Cli::try_parse_from(["so-novel-rs", "sources", "--json"]).unwrap();
        match cli.command {
            Cmd::Sources { json } => assert!(json),
            _ => panic!("expected Sources"),
        }
    }

    #[test]
    fn cli_parses_download_with_url_and_overrides() {
        let cli = Cli::try_parse_from([
            "so-novel-rs",
            "download",
            "https://example.com/book/123.html",
            "--source",
            "5",
            "--output",
            "D:\\novels",
            "--format",
            "epub",
        ])
        .unwrap();
        match cli.command {
            Cmd::Download {
                url,
                source,
                output,
                format,
            } => {
                assert_eq!(url, "https://example.com/book/123.html");
                assert_eq!(source, Some(5));
                assert_eq!(output.as_deref(), Some("D:\\novels"));
                assert_eq!(format.as_deref(), Some("epub"));
            }
            _ => panic!("expected Download"),
        }
    }

    #[test]
    fn cli_rejects_unknown_subcommand() {
        let result = Cli::try_parse_from(["so-novel-rs", "bogus"]);
        assert!(result.is_err(), "未知子命令应被 clap 拒绝");
    }

    #[test]
    fn effective_cfg_overrides_output_and_format() {
        let cfg = AppConfig::default();
        let new_cfg = effective_cfg(cfg, Some("D:/out".into()), Some("txt".into()));
        assert_eq!(new_cfg.download_path, "D:/out");
        assert_eq!(new_cfg.ext_name, ExportFormat::Txt);
    }

    #[test]
    fn effective_cfg_keeps_originals_when_no_overrides() {
        let cfg = AppConfig {
            download_path: "orig".into(),
            ext_name: ExportFormat::Html,
            ..AppConfig::default()
        };
        let new_cfg = effective_cfg(cfg, None, None);
        assert_eq!(new_cfg.download_path, "orig");
        assert_eq!(new_cfg.ext_name, ExportFormat::Html);
    }
}
