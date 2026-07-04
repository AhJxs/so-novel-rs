# Changelog

## [0.3.4] - 2026-07-04

### Added
- **CLI `--help` 多语言**：`so-novel-rs --help` / `-h` / 子命令 help
  现在按 `~/.sonovel/config.toml [global].language` 显示三种语言
  （zh-CN / zh-TW → zh-HK / en）。`locales/app.yml` 新增 `Cli:` 命名空间
  （~25 个 leaf key）；`src/cli/args.rs` 新增 `build_localized_command(lang)`
  手搓本地化的 `clap::Command` 树（因 clap 4 顶层 `about`/`long_about`/
  `after_help` 无 public setter，无法 derive 后 mutate）
- `search --help` / `sources list --help` 等必填 positional 子命令的
  help 也能正确路由到子命令 help（`src/cli/mod.rs::parse_or_help_fallback`
  剥掉 `--help` 后允许 required-arg-missing 走 help 分发）
- 顶层 `Examples:` 区按 locale 切语言；`zh-TW` 通过 `crate::i18n::locale_for`
  映射到 `zh-HK` 查 app.yml（`Language::as_str()` 仍返回 `zh-TW`，仅 i18n
  查表层用 `zh-HK`）

### Changed
- **`src/main.rs` 128 行 → 18 行 + 新建 `src/startup/` 模块**：把 mode
  判定 / Windows console attach / tracing init / cfg-gated 分发拆到
  `startup/{mod,web}.rs`；main.rs 只剩 crate attribute + 3 行 `main()`
- `LaunchMode` enum 取代原 `is_web` / `is_cli` 两个 boolean 的隐式
  precedence；`detect(args) -> LaunchMode` 是唯一的 mode 入口
- `run_web` 改名 `startup::web::run`；`parse_arg_value` 改为
  `pub(super)` 仅 startup 模块内可见（仍只被 `--host` / `--port` 两处调用）
- `attach_parent_console` Windows Win32 调用原样搬到 `startup::mod.rs`，
  cfg gate 同步迁移
- "feature not enabled" 的 bail 提示（如 `--no-default-features --features
  gui` 跑 `--web`）移到对应 `run` 函数内部，与 cfg gate 紧贴
- `init_tracing()` 仍在 `dispatch` 里调用、但仅对非 CLI 模式（CLI 内部
  仍按 `--verbose` 自决），保持"CLI 默认静默、GUI / Web 始终有日志"原行为
  避免 `tracing_subscriber::init()` 双 init panic
- 三个构建目标全部 clean：`cargo build`（默认 features）、
  `cargo build --no-default-features --features web`（Docker 路径）、
  `cargo build --no-default-features --features gui`
- 全部 370 个 lib 测试仍 pass，无新增 / 删除

### Fixed
- CLI `so-novel-rs --help` 在 `config.toml language = "en"` / `"zh-TW"`
  时仍输出简体中文（之前 help 文案硬编码中文，与 UI 语言不一致）
- release 打包的 `so-novel-rs.exe` 双击启动 GUI 时会**额外弹出一个黑色
  控制台窗口**（与 gpui 主窗口一起出现）。根因：`src/startup/mod.rs::dispatch`
  对 `LaunchMode::Gui` 调用了 `attach_parent_console()`，Explorer 双击
  场景下 `AttachConsole(ATTACH_PARENT_PROCESS)` 失败 → `AllocConsole()`
  fallback 在运行时分配新 console。修复：Gui arm 仅 `init_tracing()`，
  **不**调 attach_console（`windows_subsystem = "windows"` 已在 PE 层保证
  进程启动时无 console；Web arm 行为不变，仍 attach 以保证 Explorer 双击
  启动 Web 时日志可读）
- release 打包的 `so-novel-rs.exe` 跑 `--help` / `-V` / 任意 CLI 子命令
  时**完全没有 stdout 输出**。根因：同上 release build 是 GUI subsystem，
  Windows 默认把 stdio 关到 NUL（即使从 cmd 跑也无效），CLI arm 原本不调
  `attach_parent_console` → 写入直接被丢掉。修复：CLI arm 在 `cli::run()`
  之前**先**调 `attach_parent_console()`（cmd / bash 有父控制台时直通父
  终端；Explorer 双击无父控制台时 `AllocConsole()` 让用户看到帮助进
  console 窗口；行为与 Web arm 对齐）
