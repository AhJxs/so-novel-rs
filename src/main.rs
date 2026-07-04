// Windows release 下走 GUI subsystem，避免 GUI 启动时弹出控制台黑窗。
// CLI/Web 模式通过 `startup::attach_parent_console` 挂载到父进程控制台
// （详见 `startup::dispatch`）。
//
// 进程入口的所有职责（mode 判定 / console attach / tracing init / dispatch）
// 都委托给 `so_novel_rs::startup`，本文件只负责 argv 收集。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions), feature = "gui"),
    windows_subsystem = "windows"
)]

use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    so_novel_rs::startup::dispatch(so_novel_rs::startup::detect(&args))
}
