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
use clap::Parser;

use crate::config::{ConfigPaths, load_config};
use crate::db::init_rules_dir;

use self::args::{PKG_NAME, VERSION_STRING, build_localized_command, subcommand_name};

/// 解析 CLI 参数。`--help` / `-h` 场景下用 `try_parse_from` + 兜底空 Cli，
/// 让 `Cli::parse()` 必需的必填 positional（`search` 的 `keyword`、
/// `download` 的 `url`）缺失时不被 clap 拦截 —— help 路径应该总是可达的。
///
/// 具体：
/// 1. argv 不含 --help / -h → 直接 `try_parse_from`，错误照旧向上抛
///    （用户拼写错误 / 必填缺失照常报）。
/// 2. argv 含 --help / -h → 剥掉 help flag 再 try_parse，错误时给一个
///    `command: None` 的空 Cli + `help: true`，让 `run` 走 help 分发。
fn parse_or_help_fallback() -> Result<Cli> {
    let argv: Vec<String> = std::env::args().collect();
    let wants_help = argv.iter().any(|a| a == "--help" || a == "-h");
    if !wants_help {
        return Cli::try_parse_from(argv).map_err(Into::into);
    }
    // 剥掉 -h / --help 再 parse —— 让 required 必填项不参与校验
    let mut trimmed = argv;
    trimmed.retain(|a| a != "--help" && a != "-h");
    // 找子命令名（如果有）—— 在剥掉 --help 的 argv 里第一个非 flag 的子串。
    // 用于 fallback 时正确路由到子命令 help。
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
    // 手动把 sub 标到 cli.command 上（如果 try_parse 没成功解析出 command）。
    if cli.command.is_none() && sub.is_some() {
        // stub subcommand：run() 不用看它的具体内容，只看 subcommand_name
        // 决定走哪个子命令 help。给个空 `Search { keyword: "".into() }`。
        // 但 Search 的 keyword 是 String 必填 —— 解析不会成功（已被兜底成 None），
        // 走这个分支就是 sub 已知但 try_parse 失败。给一个 dummy 必填字段
        // 让 try_parse 不会过 —— 我们现在需要的只是让 find_subcommand 知道 name。
        // 用 `build_localized_command` 走子命令 help 时不读 subcommand 的具体值，
        // 只 `find_subcommand_mut(name)`，所以这里不构造 `cli.command` 也能工作：
        // run() 看到 cli.command.is_none() 会打印**顶层** help。
        //
        // 想让 `search --help` 真的进 search help，需要 cli.command 是 Some。
        // 重建 `Cmd::Search { keyword: "".into() }` —— 但用户没传 keyword，
        // 占位字符串足够用于 subcommand_name 路由。
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

/// CLI 入口。被 `main.rs` 在检测到子命令时调用。
///
/// ## 控制流（i18n 后）
///
/// 1. `Cli::parse()` —— parsing 不依赖 config / locale。
/// 2. `--version` → 立即打印（locale 无关）→ return。
/// 3. `load_config` —— 文件不存在时 `load_config` 返回 `AppConfig::default()`
///    （`language = SimplifiedChinese` → 默认中文 help）；无需特判。
/// 4. `rust_i18n::set_locale(crate::i18n::locale_for(cfg.global.language))` + 清缓存。
/// 5. `--help` / `-h` / 无子命令 → 用 `build_localized_command(lang)` 打印本地化 help。
/// 6. 否则：init tracing / first-run save / init rules / 派发子命令。
///
/// ## 为什么 help 分发要在 config 加载之后
///
/// 旧版直接 `Cli::command().print_help()` —— help 文本是 derive 写死的简体中文。
/// 现在需要按 `config.toml [global].language` 切语言，必须先知道 language 才能
/// `set_locale` + 拿正确翻译。控制流重排见 `docs/superpowers/plans/`。
pub fn run() -> Result<()> {
    let cli = parse_or_help_fallback()?;

    // 手动分发 --version（locale 无关，先于 config 加载）。
    if cli.version_flag {
        println!("{PKG_NAME} {VERSION_STRING}");
        return Ok(());
    }

    // 加载 config。文件不存在 → `load_config` 返回 `AppConfig::default()`
    // （含 `language = SimplifiedChinese`），无 panic / 无错误。malformed TOML
    // 才报错向上抛。
    let paths = ConfigPaths::discover();
    let cfg = load_config(&paths.config_file).context("加载 config.toml 失败")?;

    // 切到用户配置的语言。`locale_for` 是项目里 `Language → locale 字符串`
    // 的唯一权威映射（见 `crate::i18n::locale_for` 注释）。先 `invalidate_cache`
    // —— `ts()` 的缓存是按 `key` 维度存的，旧 locale 翻译要失效。
    rust_i18n::set_locale(crate::i18n::locale_for(cfg.global.language));
    crate::i18n::invalidate_cache();

    // 手动分发 --help / -h / 无子命令：调用 `build_localized_command(cfg.global.language)`
    // 手搓一个含本地化文本的 `Command` 树，按需 `find_subcommand_mut(name)` 定位。
    // 见 `args.rs::build_localized_command` 关于"为什么不用 derive 版本"的注释。
    let is_short_help = std::env::args().any(|a| a == "-h");
    if cli.help {
        let mut cmd = build_localized_command(cfg.global.language);
        // `find_subcommand_mut` 返回 `Option<&mut Command>` —— 用 if let 显式
        // 拿出子命令引用，避免 `.unwrap_or(&mut cmd)` 的双 mutable borrow。
        if let Some(sub) = &cli.command {
            if let Some(target) = cmd.find_subcommand_mut(subcommand_name(sub)) {
                if is_short_help {
                    target.print_help().ok();
                } else {
                    target.print_long_help().ok();
                }
                println!();
                return Ok(());
            }
            // 找不到子命令（不该发生 —— derive 和手搓结构应一致），fall through 顶层。
        }
        if is_short_help {
            cmd.print_help().ok();
        } else {
            cmd.print_long_help().ok();
        }
        println!();
        return Ok(());
    }

    // 没传子命令（main.rs 通常已经把"无参数 → GUI"拦了，但 `-v` / `-q`
    // 单跑这种边缘情况还是有可能走到这里）：打印顶层长帮助。
    let Some(cmd) = cli.command else {
        let mut cmd = build_localized_command(cfg.global.language);
        cmd.print_long_help().ok();
        println!();
        return Ok(());
    };

    // 默认静默 tracing：--verbose 才会把内部日志打到 stdout。
    // 注：此时还没有 subscriber，要等 --verbose 才 init；这样不开启时
    // tracing::info!/warn! 调用完全不会有任何输出。
    if cli.verbose {
        crate::logger::init_compat_legacy();
    }

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
