// Windows release 下走 GUI subsystem，避免 GUI 启动时弹出控制台黑窗。
// debug 仍保留 console subsystem，方便开发时直接看 tracing 输出 + panic backtrace。
//
// 注意：CLI 模式下 stdout/stderr 在 GUI subsystem 里是 invalid handle —
// 见下方 `attach_parent_console` 的处理。
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use std::sync::Arc;

use anyhow::Result;
use so_novel_rs::app::SoNovelApp;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// 嵌入式 PNG logo（编译时 include_bytes!，无运行时 IO）。
const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

fn main() -> Result<()> {
    // 任意 argv（除程序名外）→ CLI 模式；不带任何参数 → GUI。
    // 与 Unix 惯例一致：so-novel-rs search <kw> / so-novel-rs sources / etc.
    let is_cli = std::env::args().len() > 1;

    // GUI subsystem 的 exe 默认没有 stdio 句柄；从 cmd / PowerShell 跑 CLI 子命令
    // 时附加到父进程的控制台，这样 println! / tracing 都能正常输出。
    // 双击或从开发环境直接跑 GUI 时不调 — 调了反而会闪一个父 cmd 窗口出来。
    if is_cli {
        attach_parent_console();
    }

    init_tracing();

    if is_cli {
        return so_novel_rs::cli::run();
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("So Novel")
        // 关键：关掉原生标题栏（去左上角图标 + 软件名）。
        // 关闭/最小化/最大化按钮改由自定义 nav 栏右侧的 svg 按钮承担。
        .with_decorations(false)
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([900.0, 600.0]);

    // 解码 logo.png → IconData（RGBA），失败时不带图标启动而非崩。
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(Arc::new(icon));
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "So Novel",
        native_options,
        Box::new(|cc| Ok(Box::new(SoNovelApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe run_native failed: {e}"))?;

    Ok(())
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
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(target_os = "windows"))]
fn attach_parent_console() {}

/// 把嵌入的 PNG 解码为 egui IconData。失败返回 None，让应用以无图标启动。
fn load_icon() -> Option<egui::IconData> {
    use std::io::Cursor;
    let img = image::ImageReader::new(Cursor::new(LOGO_PNG))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    })
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,so_novel_rs=debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(false))
        .init();
}
