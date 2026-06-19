// Windows release 下走 GUI subsystem，避免 GUI 启动时弹出控制台黑窗。
// debug 仍保留 console subsystem，方便开发时直接看 tracing 输出 + panic backtrace。
//
// 注意：CLI 模式下 stdout/stderr 在 GUI subsystem 里是 invalid handle —
// 见下方 `attach_parent_console` 的处理。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

fn main() -> Result<()> {
    // 任意 argv（除程序名外）→ CLI 模式；不带任何参数 → GUI。
    // 与 Unix 惯例一致：so-novel-rs search <kw> / so-novel-rs sources / etc.
    let is_cli = std::env::args().len() > 1;

    // 解析日志目录 —— tracing 文件 layer 需要它（按天滚到 `log_dir/so-novel-rs.YYYY-MM-DD.log`）。
    // 这里不算完整 paths（gpui_app::run 内部自己 discover），只为 tracing 拿 log_dir。
    let log_dir = so_novel_rs::config::ConfigPaths::discover().log_dir;

    // GUI subsystem 的 exe 默认没有 stdio 句柄；从 cmd / PowerShell 跑 CLI 子命令
    // 时附加到父进程的控制台，这样 println! / tracing 都能正常输出。
    // 双击或从开发环境直接跑 GUI 时不调 — 调了反而会闪一个父 cmd 窗口出来。
    if is_cli {
        attach_parent_console();
    }

    init_tracing(&log_dir);

    if is_cli {
        return so_novel_rs::cli::run();
    }

    // 启动 GPUI + gpui-component GUI。
    so_novel_rs::gpui_app::run()
}

/// 把当前进程附加到父进程的控制台（仅 Windows）。
///
/// `AttachConsole(ATTACH_PARENT_PROCESS)`：
/// - 从 cmd/PowerShell 跑时成功，stdout/stderr 直通父终端；
/// - 双击 / GUI shell 启动时父进程没有控制台，调用失败 — 静默忽略，stdio 仍是空。
///
/// debug build 是 console subsystem，本来就有自己的窗口，调用此函数会失败但无害。
#[cfg(target_os = "windows")]
fn attach_parent_console() {
    // SAFETY: 单纯调 Win32 API；失败用返回值判断，不依赖 GetLastError 也能容错。
    unsafe {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_parent_console() {}

fn init_tracing(log_dir: &std::path::Path) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,so_novel_rs=debug"));

    // stdout layer（保持原行为）。
    let stdout_layer = fmt::layer().with_target(false);

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer);

    // 文件 layer：按天滚动到 `log_dir/so-novel-rs.YYYY-MM-DD.log`。
    // 用 `match` 内联构造让 Rust 推断 Layer<S> 的 S —— helper 函数的返回类型
    // 写不出嵌套的 Layered<...>。文件 appender 失败不 panic —— 静默退化为只有 stdout。
    match std::fs::create_dir_all(log_dir) {
        Ok(()) => {
            // 日志文件名 `<日期>.log`（如 `2026-06-18.log`）：传空 prefix 让 rolling 直接拼日期后缀。
            let appender = tracing_appender::rolling::daily(log_dir, "");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            // guard 进 leak 让文件 writer 后台线程存活到进程退出 —— tracing_appender 标准用法。
            Box::leak(Box::new(guard));
            // 文件 layer 关掉 ANSI 颜色码（\x1b[2m / \x1b[32m …）—— 文件不是终端，
            // 不解释转义码，带颜色码会显示成 [2m...[0m 乱码。stdout layer 保留 ANSI。
            let file_layer = fmt::layer()
                .with_writer(writer)
                .with_target(true)
                .with_ansi(false);
            registry.with(file_layer).init();
        }
        Err(e) => {
            eprintln!("init_tracing: file layer disabled ({e})");
            registry.init();
        }
    }
}
