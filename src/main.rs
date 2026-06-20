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

    so_novel_rs::logging::init_tracing(&log_dir);

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
