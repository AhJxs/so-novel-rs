//! TTY 工具：原地进度行、stderr 写入。

use std::io::{Write, stderr};

/// TTY 模式下的原地单行进度：覆盖上一行，不污染管道。
///
/// 调用方传入"状态前缀"（如"⏳ 已完成" / "🔍 搜索中…"）+ 计数 + 自定义尾部字段
/// （"最新:《X》" / "关键词:《kw》"）。统一处理 pct 算式、`\r` 回行首、`\x1b[K`
/// 擦到行尾、显式 flush。
///
/// ponytail: `\x1b[K`（ESC + `[K` = "erase to end of line"）需要终端支持 ANSI
/// —— 现代 Windows 10+ / macOS / Linux 终端都开箱即用。Windows 下若终端没启
/// `ENABLE_VIRTUAL_TERMINAL_PROCESSING`，会看到字面 `\x1b[K` 而不是清行；那是
/// 终端问题不是这里。
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
        // 防御 total=0 不除零；只通过返回值（无返回）间接验证 —— 不能 eprintln
        // 写进测试输出污染 cargo test 报告，所以只断 no-panic + 调用完成。
        print_in_place_line("⏳", 0, 0, "noop");
    }
}
