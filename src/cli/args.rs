//! `Cli` / `Cmd` clap 定义。

use clap::{Parser, Subcommand};

/// 主入口元信息：`name` / `version` 由 `--version` 自动注入。
pub(crate) const PKG_NAME: &str = "so-novel-rs";

/// 顶层 `so-novel-rs` 描述（短）。clap 在 usage 行尾 / 简略模式用。
const ABOUT_SHORT: &str = "So Novel — 简繁小说批量下载（CLI / GUI / WEB 三模）";

/// 顶层 `so-novel-rs` 描述（长）。`--help` 全文模式用。
///
/// 关键信息：
/// 1. 不传子命令 → 启动 GPUI GUI（`main.rs` 的分发逻辑）；
/// 2. 子命令走 CLI 模式，各自独立；
/// 3. 全局 flag（`-v` / `-q`）所有子命令生效。
const ABOUT_LONG: &str = "\
So Novel — 简繁小说批量下载（CLI / GUI / WEB 三模）

不传任何子命令 → 启动 GPUI 桌面客户端；
带 --web / --host / --port → 启动 Web 服务（GUI 模式）；
带子命令 → 走 CLI 模式，复用同一份 parser / crawler / export。

全局 flag（-v / -q）对所有子命令生效：
  -v, --verbose  打开内部 tracing 日志（默认静默）
  -q, --quiet    抑制逐章进度与失败源 dump，脚本管道友好";

/// `--version` 输出格式：clap 默认在版本号前自动加二进制名（"so-novel-rs 0.3.2"），
/// 这里只传版本号本身即可，不要重复写包名。
pub(crate) const VERSION_STRING: &str = env!("CARGO_PKG_VERSION");

/// so-novel-rs — 小说下载器（CLI）。
#[derive(Debug, Parser)]
#[command(
    name = PKG_NAME,
    about = ABOUT_SHORT,
    long_about = ABOUT_LONG,
    version = VERSION_STRING,
    // 关闭 clap 默认生成的 -h / --help / -V / --version / help 子命令 —— 默认都
    // 是英文 "Print help" / "Print version" / "Print this message..."。我们手动
    // 加回 -h / --help / -V / --version 并写中文 help 文本；help 子命令不需要
    // 单独开（-h / --help 已覆盖）。--help / --version 用 SetTrue 在 `mod.rs::run`
    // 里手动分发，避开 `ArgAction::Help`/`ArgAction::Version` 在子命令解析时的
    // required assert。
    disable_help_flag = true,
    disable_version_flag = true,
    disable_help_subcommand = true,
    // 让 `so-novel-rs --help` / `--version` 不带子命令也能用（手动分发需要
    // 走到 mod.rs::run 才能 print），同时让 mod.rs::run 能区分"没传子命令"
    // 和"传了子命令"。
    subcommand_required = false,
    // 放在 Options 区之后：常用调用样例，新用户最需要的"先抄哪个"
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

    // ponytail: 用 `SetTrue` + 在 `mod.rs::run` 里手动调 `Cli::command().print_help()`
    // —— clap 的 `ArgAction::Help` 会自动 exit，把 `bool` 字段当成 required 在子命令
    // 解析时 assert 失败。手动分发避免 assert，又能把帮助文本写成中文。
    /// 打印帮助信息
    #[arg(short = 'h', long = "help", action = clap::ArgAction::SetTrue, global = true)]
    pub help: bool,

    /// 打印版本号
    #[arg(short = 'V', long = "version", action = clap::ArgAction::SetTrue, global = true)]
    pub version_flag: bool,

    /// 子命令。`Option<Cmd>` 因为我们要让 `so-novel-rs --help` / `--version`
    /// 不带子命令也能用（默认 `subcommand_required = true` 会卡住这两个 flag）。
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
        /// 输出 JSON 到 stdout（机器可读，禁用人类可读格式）
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
        /// 覆盖 config.toml 的输出格式（epub / txt / html）
        #[arg(long, value_name = "epub|txt|html")]
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
    /// 不带子命令（裸 `sources`）等价于 `sources list`。
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
    /// 列出当前书源
    List {
        /// 输出 JSON 到 stdout（机器可读，禁用人类可读格式）
        #[arg(long)]
        json: bool,
    },
    /// 启用指定 ID 的书源（写回 sources_config.json）
    Enable {
        /// 书源 ID
        #[arg(value_name = "ID")]
        id: i32,
    },
    /// 禁用指定 ID 的书源（写回 sources_config.json）
    Disable {
        /// 书源 ID
        #[arg(value_name = "ID")]
        id: i32,
    },
}
