//! TTY 工具：原地进度行、stderr 写入。

use std::io::{Write, stderr};

/// TTY 模式下的原地单行进度：`\r` 回行首 + 覆盖上一行，不污染管道。
///
/// `\x1b[K`（擦到行尾）需要终端支持 ANSI：现代 Windows 10+ / macOS / Linux 都开箱即用；
/// Windows 终端若没开 `ENABLE_VIRTUAL_TERMINAL_PROCESSING` 会看到字面 `\x1b[K`（终端问题）。
pub fn print_in_place_line(label: &str, done: u64, total: usize, extra: &str) {
    let pct = if total == 0 {
        0
    } else {
        (done * 100 / total as u64).min(100)
    };
    eprint!("\r  {label} {done}/{total} ({pct}%)  {extra}\x1b[K");
    let _ = stderr().flush();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn pct_zero_when_total_is_zero() {
        // 不能断言输出（会污染 cargo test 报告），只验证 total=0 不除零、不 panic。
        print_in_place_line("⏳", 0, 0, "noop");
    }
}
