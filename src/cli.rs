//! CLI 子命令（5c）。复用现有 parser/crawler，跑在 `#[tokio::main]` runtime。
//!
//! 用法：
//! - `so-novel-rs search <关键词> [--source ID]` — 聚合或单源搜索
//! - `so-novel-rs download <详情页 URL> [--output DIR] [--format epub|txt|...]`
//! - `so-novel-rs sources` — 列出当前激活书源
//! - `so-novel-rs version` — 版本
//!
//! 不带子命令 → 启动 egui GUI（见 `main.rs` 的分发逻辑）。

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use crate::config::{load_config, AppConfig, ConfigPaths, ExportFormat};
use crate::crawler::{self, CancelToken, Progress};
use crate::db::Db;
use crate::rules::{load_rules_from_db, Source};
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
        /// 每源最多返回条数
        #[arg(long)]
        limit: Option<usize>,
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
    Sources,
    /// 打印版本号后退出
    Version,
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
        } => run_search(&cfg, &paths, keyword, source, limit),
        Cmd::Download {
            url,
            source,
            output,
            format,
        } => run_download(&cfg, &paths, url, source, output, format),
        Cmd::Sources => run_sources(&cfg, &paths),
        Cmd::Version => {
            println!("so-novel-rs {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
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
    let db = Db::open(&paths.db_file)
        .with_context(|| format!("打开 sonovel.db 失败: {}", paths.db_file.display()))?;
    let rules = load_rules_from_db(db.conn()).context("加载规则失败")?;
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

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("so-novel-cli")
        .build()
        .context("build tokio runtime")?;
    let outcomes = rt.block_on(crawler::search::search_aggregated(
        cfg,
        target_sources,
        keyword.clone(),
        limit,
        cf_bypass,
    ));

    let mut total = 0usize;
    for o in &outcomes {
        match &o.result {
            Ok(list) => {
                if !list.is_empty() {
                    println!(
                        "\n=== {}#{} ({} 条) ===",
                        o.source_name,
                        o.source_id,
                        list.len()
                    );
                    for r in list {
                        println!(
                            "  • {}  作者:{}  最新:{}  URL:{}",
                            r.book_name,
                            r.author.as_deref().unwrap_or("-"),
                            r.latest_chapter.as_deref().unwrap_or("-"),
                            r.url
                        );
                    }
                    total += list.len();
                }
            }
            Err(e) => {
                eprintln!("✗ {}#{} 失败: {}", o.source_name, o.source_id, e);
            }
        }
    }
    println!("\n共 {total} 条结果（关键词：{keyword}）");
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
    let download_task = rt.spawn(async move {
        crawler::download_book(&cfg_for_task, &source_for_task, &url_for_task, opts).await
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

fn run_sources(cfg: &AppConfig, paths: &ConfigPaths) -> Result<()> {
    let _ = cfg; // 当前未用到 cfg 字段，保留参数为未来扩展（按 lang 过滤等）
    let db = Db::open(&paths.db_file)
        .with_context(|| format!("打开 sonovel.db 失败: {}", paths.db_file.display()))?;
    let rules = load_rules_from_db(db.conn()).context("加载规则失败")?;
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
    fn cli_parses_version_subcommand() {
        let cli = Cli::try_parse_from(["so-novel-rs", "version"]).unwrap();
        assert!(matches!(cli.command, Cmd::Version));
    }

    #[test]
    fn cli_parses_sources_subcommand() {
        let cli = Cli::try_parse_from(["so-novel-rs", "sources"]).unwrap();
        assert!(matches!(cli.command, Cmd::Sources));
    }

    #[test]
    fn cli_parses_search_with_keyword() {
        let cli = Cli::try_parse_from(["so-novel-rs", "search", "凡人修仙传"]).unwrap();
        match cli.command {
            Cmd::Search {
                keyword,
                source,
                limit,
            } => {
                assert_eq!(keyword, "凡人修仙传");
                assert_eq!(source, None);
                assert_eq!(limit, None);
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
            } => {
                assert_eq!(keyword, "斗破苍穹");
                assert_eq!(source, Some(3));
                assert_eq!(limit, Some(10));
            }
            _ => panic!("expected Search"),
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
