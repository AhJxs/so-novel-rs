//! CLI 子命令: `search` / `download` / `sources` / `--version`, 复用 parser + crawler,
//! 跑在 `#[tokio::main]` runtime。不带子命令 → 启动 GPUI GUI (见 `main.rs` 分发逻辑)。
//!
//! 本文件是 **re-export 门面**, 对外只暴露 `run()` (`Cli` / `Cmd` 仅供测试);
//! 子模块: `args` (clap 定义) / `search` / `download` / `sources`。
//!
//! CLI 启动期工具 (`effective_cfg` / `load_active_sources` / `validate_range`) 在
//! `crate::core::bootstrap`。

mod args;
mod download;
mod search;
mod sources;

pub use args::{Cli, Cmd, SourcesAction};

use anyhow::{Context, Result};
use clap::Parser;

use crate::core::bootstrap::cli_load_paths_and_config;

/// CLI 共享 tokio runtime 构造器 (`new_multi_thread` + `enable_all` + 线程名 `so-novel-cli`),
/// search / download 共用同一份配置。
pub(super) fn build_cli_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("so-novel-cli")
        .build()
        .context("build tokio runtime")
}

/// CLI 单行原地进度模板: 截断 → 拼 `《X》` 后缀 → 调底层 `print_in_place_line`,
/// search / download 只差 label 文案与截断长度。
pub(super) fn print_progress_line(label: &str, done: u32, total: usize, suffix: &str) {
    crate::utils::tty::print_in_place_line(label, u64::from(done), total, suffix);
}

use self::args::{PKG_NAME, VERSION_STRING, build_localized_command, subcommand_name};

/// 解析 CLI 参数。`--help` / `-h` 用 `try_parse_from` + 兜底空 Cli: help 路径必须总是可达,
/// 不能因缺少必填 positional (`search` 的 `keyword` / `download` 的 `url`) 被 clap 拦住。
///
/// 剥掉 help flag 再 parse, 失败时给一个 `command: None` + `help: true` 的空 Cli,
/// 由 `run` 走 help 分发。
fn parse_or_help_fallback() -> Result<Cli> {
    let argv: Vec<String> = std::env::args().collect();
    let wants_help = argv.iter().any(|a| a == "--help" || a == "-h");
    if !wants_help {
        return Cli::try_parse_from(argv).map_err(Into::into);
    }
    // 剥掉 -h / --help 再 parse —— 让 required 必填项不参与校验
    let mut trimmed = argv;
    trimmed.retain(|a| a != "--help" && a != "-h");
    // 找子命令名 (剥掉 --help 后第一个匹配的子串), 用于 fallback 时路由到子命令 help。
    let sub: Option<&'static str> = trimmed
        .iter()
        .skip(1) // 跳过 binary name
        .find_map(|a| match a.as_str() {
            "search" => Some("search"),
            "download" => Some("download"),
            "sources" => Some("sources"),
            _ => None,
        });
    let mut cli = Cli::try_parse_from(trimmed).unwrap_or(Cli {
        verbose: false,
        quiet: false,
        help: true,
        version_flag: false,
        command: None,
    });
    cli.help = true;
    // 兜底重建一个 dummy 子命令: `run()` 只用 `subcommand_name` 路由 help、不读字段值,
    // 故占位字符串 (空 keyword / 空 url) 足够。
    if cli.command.is_none() && sub.is_some() {
        cli.command = sub.and_then(|s| match s {
            "search" => Some(Cmd::Search {
                keyword: String::new(),
                source: None,
                limit: None,
                json: false,
            }),
            "download" => Some(Cmd::Download {
                url: String::new(),
                source: None,
                output: None,
                format: None,
                from: None,
                to: None,
            }),
            "sources" => Some(Cmd::Sources {
                action: None,
                json: false,
            }),
            _ => None,
        });
    }
    Ok(cli)
}

/// CLI 入口, 被 `main.rs` 在检测到子命令时调用。
///
/// 控制流: parse (不依赖 config / locale) → `--version` 立即打印 → 加载 config →
/// `set_locale` + 清缓存 → `--help` / `-h` / 无子命令打印本地化 help →
/// 否则 init tracing (仅 `--verbose`) 并派发子命令。
///
/// help 之所以排到 config 之后: 文案要按 `config.toml [global].language` 切语言,
/// 必须先知道 language 才能 `set_locale` 拿正确翻译 (旧版是 derive 写死的简体中文)。
pub fn run() -> Result<()> {
    let cli = parse_or_help_fallback()?;

    // 手动分发 --version（locale 无关，先于 config 加载）。
    if cli.version_flag {
        println!("{PKG_NAME} {VERSION_STRING}");
        return Ok(());
    }

    // 加载 config + 首次启动写默认 + 初始化规则目录 (三端 startup 兜底矩阵见 `core::bootstrap`)。
    let (paths, cfg) = cli_load_paths_and_config()?;

    // 切到用户配置的语言并清 `ts()` 缓存 (缓存按 key 维度存, 旧 locale 的翻译要失效)。
    rust_i18n::set_locale(crate::i18n::locale_for(cfg.global.language));
    crate::i18n::invalidate_cache();

    // --help / -h: 用 `build_localized_command` 手搓本地化 Command 树 (原因见 `args.rs`)。
    let is_short_help = std::env::args().any(|a| a == "-h");
    if cli.help {
        let mut cmd = build_localized_command(cfg.global.language);
        // 用 if let 拿子命令引用, 避免 `.unwrap_or(&mut cmd)` 造成的双 mutable borrow。
        if let Some(sub) = &cli.command
            && let Some(target) = cmd.find_subcommand_mut(subcommand_name(sub))
        {
            if is_short_help {
                target.print_help().ok();
            } else {
                target.print_long_help().ok();
            }
            println!();
            return Ok(());
        }
        // 找不到子命令 (不该发生 —— derive 与手搓结构应一致), fall through 顶层。
        if is_short_help {
            cmd.print_help().ok();
        } else {
            cmd.print_long_help().ok();
        }
        println!();
        return Ok(());
    }

    // 没传子命令 (main.rs 一般已把"无参数 → GUI"拦了): 打印顶层长帮助。
    let Some(cmd) = cli.command else {
        let mut cmd = build_localized_command(cfg.global.language);
        cmd.print_long_help().ok();
        println!();
        return Ok(());
    };

    // 默认静默 tracing: 只有 --verbose 才 init subscriber, 否则 info!/warn! 完全没有输出。
    if cli.verbose {
        crate::logger::init_compat_legacy();
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
            // 裸 `sources` / `--json` → 等价于 list
            None => sources::run_list(&paths, json),
            Some(SourcesAction::List { json: j }) => sources::run_list(&paths, j),
            Some(SourcesAction::Enable { id }) => sources::run_set_disabled(&paths, id, false),
            Some(SourcesAction::Disable { id }) => sources::run_set_disabled(&paths, id, true),
        },
    }
}

#[cfg(test)]
mod tests;
