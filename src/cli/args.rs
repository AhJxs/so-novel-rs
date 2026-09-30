//! `Cli` / `Cmd` clap 定义。

use clap::{Arg, ArgAction, Command, Parser, Subcommand};

use crate::config::Language;

pub const PKG_NAME: &str = "so-novel-rs";

/// 顶层 `so-novel-rs` 描述（短）。clap 在 usage 行尾 / 简略模式用。
const ABOUT_SHORT: &str = "So Novel — 简繁小说批量下载（CLI / GUI / WEB 三模）";

/// 顶层 `so-novel-rs` 描述（长）。`--help` 全文模式用。
const ABOUT_LONG: &str = "\
So Novel — 简繁小说批量下载（CLI / GUI / WEB 三模）

不传任何子命令 → 启动 GPUI 桌面客户端；
带 --web / --host / --port → 启动 Web 服务（GUI 模式）；
带子命令 → 走 CLI 模式，复用同一份 parser / crawler / export。

全局 flag（-v / -q）对所有子命令生效：
  -v, --verbose  打开内部 tracing 日志（默认静默）
  -q, --quiet    抑制逐章进度与失败源 dump，脚本管道友好";

/// `--version` 输出格式：clap 会自动在版本号前加二进制名，故这里只传版本号。
pub const VERSION_STRING: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Parser)]
#[command(
    name = PKG_NAME,
    about = ABOUT_SHORT,
    long_about = ABOUT_LONG,
    version = VERSION_STRING,
    // 关闭 clap 自动生成的 -h / --help / -V / --version / help 子命令（默认文案是英文），
    // 改为在 `mod.rs::run` 用 SetTrue 手动分发 —— 避开 `ArgAction::Help`/`Version`
    // 在子命令解析时的 required assert，同时能写中文 help。
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true,
    // 让 `--help` / `--version` 不带子命令也能用，并让 `mod.rs::run` 能区分"没传子命令"。
    subcommand_required = false,
    // 放在 Options 区之后：新用户最需要的"先抄哪个"样例
    after_help = "Examples:\n  \
        启动 GUI（无子命令）:                                so-novel-rs\n  \
        搜索书源（聚合）:                                    so-novel-rs search 凡人修仙传\n  \
        单源搜索 + JSON 输出:                                so-novel-rs search 凡人修仙传 --source 5 --json\n  \
        下载整本书:                                          so-novel-rs download https://example.com/book/123.html\n  \
        下载指定章节范围:                                    so-novel-rs download https://example.com/book/123.html --from 100 --to 200\n  \
        列出书源:                                            so-novel-rs sources list\n  \
        禁用书源:                                            so-novel-rs sources disable 5\n  \
        启用书源:                                            so-novel-rs sources enable 5"
)]
pub struct Cli {
    /// 打开内部 tracing 日志（默认静默，避免污染 --json 等机器可读输出）
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    /// 抑制逐章进度 / 失败源 dump（脚本友好；纯 stdout 输出）
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,

    // 用 `SetTrue` + `mod.rs::run` 手动分发: clap 的 `ArgAction::Help` 会自动 exit,
    // 并把 `bool` 字段当成 required 在子命令解析时 assert 失败。
    #[arg(short = 'h', long = "help", action = clap::ArgAction::SetTrue, global = true)]
    pub help: bool,

    #[arg(short = 'V', long = "version", action = clap::ArgAction::SetTrue, global = true)]
    pub version_flag: bool,

    /// 子命令。`Option<Cmd>` 以支持 `--help` / `--version` 不带子命令使用
    /// （默认 `subcommand_required = true` 会卡住这两个 flag）。
    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// 搜索书源。默认聚合搜索；--source 指定单源。
    #[command(after_help = "Examples:\n  \
        聚合搜索（默认走所有启用书源）:                       so-novel-rs search 凡人修仙传\n  \
        单源搜索（只走 #5 书源）:                             so-novel-rs search 凡人修仙传 --source 5\n  \
        限制每源条数 + JSON 输出:                             so-novel-rs search 凡人修仙传 --limit 20 --json")]
    Search {
        /// 关键词（书名 / 作者）
        keyword: String,
        /// 指定书源 ID；省略则聚合所有启用书源
        #[arg(long, value_name = "ID")]
        source: Option<i32>,
        /// 每源最多返回条数（覆盖 config.toml 的 search-limit）
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
        #[arg(long)]
        json: bool,
    },
    /// 通过详情页 URL 下载整本书（默认全本；可用 --from / --to 指定章节范围）
    #[command(after_help = "Examples:\n  \
        全本下载:                                            so-novel-rs download https://example.com/book/123.html\n  \
        指定书源 + 自定义输出目录:                           so-novel-rs download https://example.com/book/123.html --source 5 --output D:\\novels\n  \
        下载章节 100-200（1-based）:                         so-novel-rs download https://example.com/book/123.html --from 100 --to 200\n  \
        抑制逐章进度，脚本模式:                              so-novel-rs download https://example.com/book/123.html --quiet")]
    Download {
        /// 详情页 URL
        url: String,
        /// 书源 ID（默认按 URL 自动匹配；未匹配则取第一个启用的源）
        #[arg(long, value_name = "ID")]
        source: Option<i32>,
        /// 覆盖 config.toml 的下载目录
        #[arg(long, value_name = "DIR")]
        output: Option<String>,
        /// 覆盖 config.toml 的输出格式（epub / txt / html / pdf / markdown）
        #[arg(long, value_name = "epub|txt|html|pdf|markdown")]
        format: Option<String>,
        /// 起始章节（1-based；省略 → 1）
        #[arg(long, value_name = "N")]
        from: Option<usize>,
        /// 结束章节（1-based；省略 → 最后一章；超出实际章数自动截断）
        #[arg(long, value_name = "N")]
        to: Option<usize>,
    },
    /// 书源管理：list / enable / disable
    ///
    #[command(after_help = "Examples:\n  \
        列出所有书源（人类可读）:                           so-novel-rs sources list\n  \
        列出所有书源（JSON）:                               so-novel-rs sources list --json\n  \
        禁用 #5 书源:                                       so-novel-rs sources disable 5\n  \
        启用 #5 书源:                                       so-novel-rs sources enable 5")]
    Sources {
        #[command(subcommand)]
        action: Option<SourcesAction>,
        /// 输出 JSON 到 stdout（仅 list 有效；与 `sources list --json` 等价；旧版兼容）
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum SourcesAction {
    List {
        #[arg(long)]
        json: bool,
    },
    /// 启用指定 ID 的书源（写回 `sources_config.json`）
    Enable {
        #[arg(value_name = "ID")]
        id: i32,
    },
    /// 禁用指定 ID 的书源（写回 `sources_config.json`）
    Disable {
        #[arg(value_name = "ID")]
        id: i32,
    },
}

/// 手搓一个**本地化的 `clap::Command`** 用于 help 打印。
///
/// 不能复用 derive 的 `Cli::command()`：clap 4 顶层的 `about` / `long_about` /
/// `after_help` 是构造期 builder 设的 `String`、**没有 public setter**，derive 之后改不动；
/// 而这些文案要按 `config.toml [global].language`（zh-CN / zh-TW / en）运行时切换，
/// 只能整棵 `Command` 手搓。
///
/// 结构与 derive 一一对应：顶层 4 个 global arg (`-v`/`-q`/`-h`/`-V`) + `search`/`download`/
/// `sources` 三个子命令 + `sources` 下 `list`/`enable`/`disable`。`sources --json` 与
/// `sources list --json` 各放一个同名 flag（derive 里是 `Cmd::Sources.json` 一个字段）。
/// 任何结构偏离都会被 `src/cli/tests.rs::localized_command_matches_derive_structure` 抓住。
pub fn build_localized_command(lang: Language) -> Command {
    // 切到目标 locale，并清空 `ts()` 缓存（缓存里的旧 locale 翻译要失效）。
    rust_i18n::set_locale(crate::i18n::locale_for(lang));
    crate::i18n::invalidate_cache();

    // clap 的 help / about / long_about 接 `&'static str` / `String`，而 TStr 在 gui 下是
    // SharedString、web-only 下是 String，故统一 `.to_string()`。
    let ts = |key: &'static str| crate::i18n::ts(key).to_string();

    Command::new(PKG_NAME)
        .about(ts("Cli.about_short"))
        .long_about(ts("Cli.about_long"))
        .after_help(ts("Cli.after_help"))
        .version(VERSION_STRING)
        // 关闭 clap 自动注入的 --help / -V / help 子命令, 与 derive `Cli` 的三个
        // `disable_*` 开关对齐 —— 我们手搓同名 arg 自己处理。
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        // 全局 flag —— 与 derive `Cli` 字段一一对应。
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .action(ArgAction::SetTrue)
                .global(true)
                .help(ts("Cli.verbose_help")),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .action(ArgAction::SetTrue)
                .global(true)
                .help(ts("Cli.quiet_help")),
        )
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::SetTrue)
                .global(true)
                .help(ts("Cli.help_help")),
        )
        .arg(
            Arg::new("version_flag")
                .short('V')
                .long("version")
                .action(ArgAction::SetTrue)
                .global(true)
                .help(ts("Cli.version_help")),
        )
        // 三个 subcommand —— about / after_help 走 `Cli.xxx` 翻译。
        .subcommand(
            Command::new("search")
                .about(ts("Cli.search_about"))
                .after_help(ts("Cli.search_after_help"))
                .arg(
                    Arg::new("keyword")
                        .help(ts("Cli.search_keyword_help"))
                        .required(true),
                )
                .arg(
                    Arg::new("source")
                        .long("source")
                        .value_name("ID")
                        .help(ts("Cli.search_source_help")),
                )
                .arg(
                    Arg::new("limit")
                        .long("limit")
                        .value_name("N")
                        .help(ts("Cli.search_limit_help")),
                )
                .arg(
                    Arg::new("json")
                        .long("json")
                        .action(ArgAction::SetTrue)
                        .help(ts("Cli.search_json_help")),
                ),
        )
        .subcommand(
            Command::new("download")
                .about(ts("Cli.download_about"))
                .after_help(ts("Cli.download_after_help"))
                .arg(
                    Arg::new("url")
                        .help(ts("Cli.download_url_help"))
                        .required(true),
                )
                .arg(
                    Arg::new("source")
                        .long("source")
                        .value_name("ID")
                        .help(ts("Cli.download_source_help")),
                )
                .arg(
                    Arg::new("output")
                        .long("output")
                        .value_name("DIR")
                        .help(ts("Cli.download_output_help")),
                )
                .arg(
                    Arg::new("format")
                        .long("format")
                        .value_name("epub|txt|html|pdf|markdown")
                        .help(ts("Cli.download_format_help")),
                )
                .arg(
                    Arg::new("from")
                        .long("from")
                        .value_name("N")
                        .help(ts("Cli.download_from_help")),
                )
                .arg(
                    Arg::new("to")
                        .long("to")
                        .value_name("N")
                        .help(ts("Cli.download_to_help")),
                ),
        )
        .subcommand(
            Command::new("sources")
                .about(ts("Cli.sources_about"))
                .after_help(ts("Cli.sources_after_help"))
                // 顶层 --json: 兼容旧版裸 `sources --json` 调用 (derive 里是 `Cmd::Sources.json`)。
                .arg(
                    Arg::new("json")
                        .long("json")
                        .action(ArgAction::SetTrue)
                        .help(ts("Cli.sources_json_help")),
                )
                .subcommand(
                    Command::new("list")
                        .about(ts("Cli.sources_list_about"))
                        .arg(
                            Arg::new("json")
                                .long("json")
                                .action(ArgAction::SetTrue)
                                .help(ts("Cli.sources_list_json_help")),
                        ),
                )
                .subcommand(
                    Command::new("enable")
                        .about(ts("Cli.sources_enable_about"))
                        .arg(
                            Arg::new("id")
                                .value_name("ID")
                                .required(true)
                                .help(ts("Cli.sources_enable_id_help")),
                        ),
                )
                .subcommand(
                    Command::new("disable")
                        .about(ts("Cli.sources_disable_about"))
                        .arg(
                            Arg::new("id")
                                .value_name("ID")
                                .required(true)
                                .help(ts("Cli.sources_disable_id_help")),
                        ),
                ),
        )
}

/// 把 `cli.command` 映射到子命令名 (供 `find_subcommand_mut(name).print_long_help()`),
/// 与 derive `Cmd` 的 variant 一一对应。
pub const fn subcommand_name(cmd: &Cmd) -> &'static str {
    match cmd {
        Cmd::Search { .. } => "search",
        Cmd::Download { .. } => "download",
        Cmd::Sources { .. } => "sources",
    }
}
