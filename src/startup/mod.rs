//! 进程启动层：从 argv + `SO_NOVEL_WEB` env 判定走 GUI / Web / CLI 模式，各自转交给
//! `desktop::run` / `web::run` / `cli::run`。
//!
//! 两个顺序约束，错则行为可见地坏：
//! - CLI / Web 必须先 `attach_parent_console()` 再分发：release 是 GUI subsystem exe，不 attach 则
//!   stdio 关到 NUL，用户看不到任何输出。
//! - GUI **不**能 attach：Explorer 双击时 `AllocConsole` fallback 会弹黑窗。

pub mod web;

use anyhow::Result;

/// 三种启动模式。`Web` 携带已解析的 `host` / `port`：dispatch 阶段不再读
/// argv，所有 argv 解析都集中在 [`detect`] 里完成。
#[derive(Debug)]
pub enum LaunchMode {
    /// CLI 子命令模式（`so-novel-rs search ...` / `download ...` / `sources ...`）。
    Cli,
    /// Web 服务模式（`so-novel-rs --web [--host H] [--port P]` 或 `SO_NOVEL_WEB=1`）。
    Web { host: String, port: u16 },
    /// GPUI 桌面客户端模式（无任何参数时）。
    Gui,
}

/// 从 argv 判定启动模式，precedence：`--web` argv → `SO_NOVEL_WEB` env →
/// 除 binary name 外还有 arg → `Gui`。`--host` / `--port` 缺失时用默认值。
pub fn detect(args: &[String]) -> LaunchMode {
    if args.iter().any(|a| a == "--web") {
        return LaunchMode::Web {
            host: web::parse_arg_value_pub(args, "--host").unwrap_or_else(|| "127.0.0.1".into()),
            port: web::parse_arg_value_pub(args, "--port")
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(8080),
        };
    }
    let env_web = std::env::var("SO_NOVEL_WEB").is_ok_and(|v| v == "1" || v == "true");
    if env_web {
        return LaunchMode::Web {
            host: "127.0.0.1".into(),
            port: 8080,
        };
    }
    if args.len() > 1 {
        return LaunchMode::Cli;
    }
    LaunchMode::Gui
}

/// 把当前进程附加到父进程控制台（仅 Windows）；`AttachConsole` 失败时
/// （双击 / GUI shell 启动，父进程无控制台）回退 `AllocConsole()`，确保
/// CLI/Web 仍有 stdio。debug build 本身是 console subsystem，静默成功。
#[cfg(target_os = "windows")]
#[allow(unsafe_code)] // SAFETY: Windows 控制台附着是 OS 层 FFI, 唯一可行的接入点
pub fn attach_parent_console() {
    unsafe {
        use windows_sys::Win32::System::Console::{
            ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole,
        };
        // SAFETY: `AttachConsole` / `AllocConsole` 是 Win32 控制台管理 API,
        // 接受简单整数参数 (`ATTACH_PARENT_PROCESS` = `u32::MAX`), 无指针/句柄, 不会越界。
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
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

/// 当前构建不含 GUI 功能（用户没加 `--features gui`），提示改用 `--web` / `--host` / `--port`。
#[cfg(not(feature = "gui"))]
pub fn run_gui() -> Result<()> {
    anyhow::bail!("当前构建不含 GUI（需 --features gui），请使用 --web 或 --host/--port 模式")
}

/// 调度器：按 `LaunchMode` 分发到对应模式。
///
/// CLI / Web 先 `attach_parent_console()` 再分发（Web 之后还要 `init_tracing()`）。CLI **不**调
/// `init_tracing`（由 `cli::run` 自己在 `--verbose` 时决定）；全局
/// `tracing_subscriber::registry().init()` 二次调用会 panic。
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
