//! CLI 子命令（5c）。复用现有 parser/crawler，跑在 `#[tokio::main]` runtime。
//!
//! 用法：
//! - `so-novel-rs search <关键词> [--source ID] [--json]` — 聚合或单源搜索
//! - `so-novel-rs download <详情页 URL> [--output DIR] [--format epub|txt|...]`
//! - `so-novel-rs sources [--json]` — 列出当前激活书源
//! - `so-novel-rs --version` / `-V` — 版本（clap 自动注入）
//!
//! 不带子命令 → 启动 GPUI GUI（见 `main.rs` 的分发逻辑）。
//!
//! ## 内部模块拆分
//!
//! - `args`    — `Cli` / `Cmd` clap 定义
//! - `search`  — `run_search` 子命令
//! - `download` — `run_download` 子命令
//! - `sources` — `run_sources` 子命令
//! - `helpers` — 共享工具（`effective_cfg` / `load_active_sources`）
//!
//! 本文件作为 **re-export 门面**，对外只暴露 `run()`（`Cli` / `Cmd` 仅供测试用）。

mod args;
mod download;
mod search;
mod sources;
mod util;

pub use args::{Cli, Cmd, SourcesAction};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};

use crate::config::{ConfigPaths, load_config};
use crate::persistent::init_rules_dir;

use self::args::{PKG_NAME, VERSION_STRING};

/// CLI 入口。被 `main.rs` 在检测到子命令时调用。
pub fn run() -> Result<()> {
    let cli = Cli::parse();

    // 手动分发 --version / --help（避开 clap 默认的英文 help 文本）。
    // 见 `args.rs` 的 `disable_help_flag` / `disable_version_flag` 注释。
    if cli.version_flag {
        println!("{PKG_NAME} {VERSION_STRING}");
        return Ok(());
    }
    if cli.help {
        let mut cmd = Cli::command();
        // 顶层用 -h 时，clap 行为是"短帮助"（只列命令名 + 主选项），
        // --help 给全文。这里按命令行长度判定（h 单字符 = 短）。
        let is_short = std::env::args().any(|a| a == "-h");
        if is_short {
            cmd.print_help().ok();
        } else {
            cmd.print_long_help().ok();
        }
        println!();
        return Ok(());
    }

    // 没传子命令（main.rs 通常已经把"无参数 → GUI"拦了，但 `-v` / `-q`
    // 单跑这种边缘情况还是有可能走到这里）：打印帮助。
    let Some(cmd) = cli.command else {
        let mut cmd = Cli::command();
        cmd.print_long_help().ok();
        println!();
        return Ok(());
    };

    // 默认静默 tracing：--verbose 才会把内部日志打到 stdout。
    // 注：此时还没有 subscriber，要等 --verbose 才 init；这样不开启时
    // tracing::info!/warn! 调用完全不会有任何输出。
    if cli.verbose {
        crate::logging::init_tracing();
    }
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

    // 初始化规则目录
    if let Err(e) = init_rules_dir(&paths.rules_dir) {
        tracing::warn!("规则目录初始化失败: {e:#}");
    }

    match cmd {
        Cmd::Search {
            keyword,
            source,
            limit,
            json,
        } => search::run_search(&cfg, &paths, keyword, source, limit, json, cli.quiet),
        Cmd::Download {
            url,
            source,
            output,
            format,
            from,
            to,
        } => download::run_download(
            &cfg, &paths, url, source, output, format, from, to, cli.quiet,
        ),
        Cmd::Sources { action, json } => match action {
            // 裸 `sources` / `sources --json`（旧版兼容）→ 等价于 list
            None => sources::run_list(&paths, json),
            Some(SourcesAction::List { json: j }) => sources::run_list(&paths, j),
            Some(SourcesAction::Enable { id }) => sources::run_set_disabled(&paths, id, false),
            Some(SourcesAction::Disable { id }) => sources::run_set_disabled(&paths, id, true),
        },
    }
}

#[cfg(test)]
mod tests;
