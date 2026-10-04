// Windows release 下走 GUI subsystem，避免启动时弹出控制台黑窗。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;

fn main() -> Result<()> {
    so_novel_rs::logger::init();
    so_novel_rs::desktop::run()
}
