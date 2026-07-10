//! 进程启动层：从 argv + `SO_NOVEL_WEB` env 判定走 GUI / Web / CLI 模式，
//! 各自转交给 `desktop::run` / `web::run` / `cli::run`。
//!
//! 主要拆 main.rs 的几个责任点：
//! - `LaunchMode` + `detect` —— 把原 `main.rs` 里 `is_web` / `is_cli` 两个
//!   boolean + 非显式 precedence 浓缩成一个 enum + 一个 pure-ish 函数。
//! - `attach_parent_console` —— Windows-only，把 GUI subsystem exe 的
//!   stdio 挂到父进程控制台（cmd / PowerShell），失败回退 `AllocConsole`。
//! - `run_gui` —— cfg gate 内联在 `desktop::run` 调用点；缺 feature
//!   时返回与原 main.rs 等价的 bail 信息。
//! - `dispatch` —— 唯一调度入口：CLI 路径**先 `attach_console`** 再 `cli::run()`
//!   （release GUI subsystem exe 默认 stdio 关到 NUL，不 attach 用户看不到任何
//!   输出；CLI 内部自己决定是否 `init_tracing，避免` `tracing_subscriber::init()`
//!   双 init panic）；Web 路径 `attach_console` 后再 `init_tracing` 再分发；Gui 路径
//!   **不** `attach_console（GUI` subsystem exe 启动时已无 console，避免 `AllocConsole`
//!   fallback 在 Explorer 双击时弹黑窗），仅 `init_tracing` 后分发。

pub mod web;

use anyhow::Result;

/// 三种启动模式。
///
/// `Web` 携带 `host` / `port`（已经从 argv + env 解析出来），dispatch 阶段
/// 不再读 argv —— 所有 argv 解析都在 [`detect`] 里集中完成。
#[derive(Debug)]
pub enum LaunchMode {
    /// CLI 子命令模式（`so-novel-rs search ...` / `download ...` / `sources ...`）。
    Cli,
    /// Web 服务模式（`so-novel-rs --web [--host H] [--port P]` 或 `SO_NOVEL_WEB=1`）。
    Web { host: String, port: u16 },
    /// GPUI 桌面客户端模式（无任何参数时）。
    Gui,
}

/// 从 argv 判定启动模式。
///
/// Precedence（与原 main.rs 一致）：
/// 1. `--web` argv flag → `Web`（`--host` / `--port` 缺失时走 `startup::web::run` 内的默认值）
/// 2. `SO_NOVEL_WEB=1` / `SO_NOVEL_WEB=true` env → `Web`（env 不携带 host/port，
///    与现状一致：env 模式只触发 Web 启动，host/port 走默认值）
/// 3. `args.len() > 1`（无 `--web` 也无 env） → `Cli`
/// 4. 其它 → `Gui`
pub fn detect(args: &[String]) -> LaunchMode {
    // 1. `--web` argv（最显式）
    if args.iter().any(|a| a == "--web") {
        return LaunchMode::Web {
            host: web::parse_arg_value_pub(args, "--host").unwrap_or_else(|| "127.0.0.1".into()),
            port: web::parse_arg_value_pub(args, "--port")
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(8080),
        };
    }
    // 2. `SO_NOVEL_WEB` env
    let env_web = std::env::var("SO_NOVEL_WEB").is_ok_and(|v| v == "1" || v == "true");
    if env_web {
        return LaunchMode::Web {
            host: "127.0.0.1".into(),
            port: 8080,
        };
    }
    // 3. CLI：除 binary name 外还有任何 arg
    if args.len() > 1 {
        return LaunchMode::Cli;
    }
    // 4. 默认 GUI
    LaunchMode::Gui
}

/// 把当前进程附加到父进程的控制台（仅 Windows），失败时自行分配。
///
/// `AttachConsole(ATTACH_PARENT_PROCESS)`：
/// - 从 cmd/PowerShell 跑时成功，stdout/stderr 直通父终端；
/// - 双击 / GUI shell 启动时父进程没有控制台，调用失败 →
///   回退 `AllocConsole()` 分配一个新控制台窗口，确保 CLI/Web 仍有 stdio。
///
/// debug build 是 console subsystem，本来就有自己的窗口，此函数静默成功。
#[cfg(target_os = "windows")]
#[allow(unsafe_code)] // SAFETY: Windows 控制台附着是 OS 层 FFI, 唯一可行的接入点
pub fn attach_parent_console() {
    unsafe {
        use windows_sys::Win32::System::Console::{
            ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole,
        };
        // SAFETY: `AttachConsole` / `AllocConsole` 是 Win32 控制台管理 API,
        // 接受简单整数参数 (`ATTACH_PARENT_PROCESS` = `u32::MAX`), 无指针/句柄,
        // 调用结果仅影响本进程 stdio 绑定, 不会越界。
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            // 没有父控制台（如从 Explorer 启动），自行分配一个
            AllocConsole();
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn attach_parent_console() {}

/// GUI 模式入口。`feature = "gui"` 启用时转交给 `desktop::run`；
/// 否则返回与原 main.rs 等价的 bail 信息。
#[cfg(feature = "gui")]
pub fn run_gui() -> Result<()> {
    crate::desktop::run()
}

/// 当前构建不含 GUI 功能（用户没加 `--features gui`），无法启动桌面客户端。
/// 提示改用 `--web` / `--host` / `--port` 走 Web 模式。
#[cfg(not(feature = "gui"))]
pub fn run_gui() -> Result<()> {
    anyhow::bail!("当前构建不含 GUI（需 --features gui），请使用 --web 或 --host/--port 模式")
}

/// 调度器：按 `LaunchMode` 分发到对应模式。
///
/// **CLI 路径**：先 `attach_parent_console()`，再 `cli::run()`。Release
/// build 是 GUI subsystem，Windows 默认把 stdio 关到 NUL（即使从 cmd 跑
/// 也无效）；不 attach 则用户看不到任何 stdout 输出（`--help` / `-V` / 子
/// 命令结果全丢）。`AttachConsole(ATTACH_PARENT_PROCESS)` 在 cmd / bash
/// 父控制台存在时成功、stdout 直通父终端；Explorer 双击无父控制台时
/// fallback `AllocConsole()` —— 用户能看到帮助进 console 窗口。不调
/// `init_tracing`：CLI 内部（`cli::run` → 仅 `--verbose` 时）自己决定；
/// 全局 `tracing_subscriber::registry().init()` 二次调用会 panic。
///
/// **Web 路径**：先 `attach_parent_console()`（Explorer 双击无父 console
/// 时 fallback `AllocConsole()`，让 axum 日志有地方输出），再 `init_tracing()`
/// （让 `tracing::info!` 打到 attach 后的 stdio），最后分发到 `web::run`。
///
/// **Gui 路径**：仅 `init_tracing()`，**不**调 `attach_parent_console`。
/// 原因：`main.rs` 的 `windows_subsystem = "windows"` 已经在 PE 层保证
/// 进程启动时无 console；若在这里 attach，Explorer 双击场景下
/// `AttachConsole(ATTACH_PARENT_PROCESS)` 失败 → `AllocConsole()` fallback
/// 会**运行时弹一个黑色 console 窗口**。tracing 输出无 stdout 时静默丢
/// 弃，行为与原生 GUI app 一致。
pub fn dispatch(mode: LaunchMode) -> Result<()> {
    match mode {
        LaunchMode::Cli => {
            attach_parent_console();
            crate::cli::run()
        }
        LaunchMode::Web { host, port } => {
            attach_parent_console();
            crate::logger::init();
            web::run(host, port)
        }
        LaunchMode::Gui => {
            crate::logger::init();
            run_gui()
        }
    }
}
