//! 文件系统工具。对应 Java `util.FileUtils` 中与平台相关的部分。

use std::path::Path;

/// 替换文件名中的非法字符，仅适用于"文件名"，不要传入路径分隔符。
/// Windows 与 Java 端的处理保持一致（详见 `util.FileUtils#sanitizeFileName`）。
pub fn sanitize_filename(name: &str) -> String {
    if cfg!(target_os = "windows") {
        let mut out = String::with_capacity(name.len());
        for c in name.chars() {
            let replaced = match c {
                ':' => '：',
                '*' => '＊',
                '?' => '？',
                '"' => '\'',
                '<' => '＜',
                '>' => '＞',
                '/' | '\\' | '|' => '_',
                _ => c,
            };
            out.push(replaced);
        }
        out
    } else if cfg!(any(target_os = "linux", target_os = "macos")) {
        let mut out = String::with_capacity(name.len());
        for c in name.chars() {
            // Unix 只禁 `/`（路径分隔符）和 `\0`；`\` 不是分隔符但与 Windows
            // 跨平台一致地清掉，避免同名文件在不同平台行为不一。
            // **不替换 `.`** —— 扩展名点（如 `.pdf`/`.epub`）必须保留，否则
            // `书名(作者).pdf` 会被洗成 `书名(作者)。pdf` 导致找不到文件。
            let replaced = match c {
                '/' | '\\' | '\0' => '_',
                _ => c,
            };
            out.push(replaced);
        }
        out
    } else {
        name.replace(['/', '\\'], "")
    }
}

/// 日志字段脱敏：超过 `max_chars` 的字符串截断并加 `***` 后缀。
///
/// 用于搜索关键词等用户输入字段，避免完整查询词写入日志文件。
/// 短于阈值的字符串原样返回，零开销。
pub fn truncate_log(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        return s.to_string();
    }
    let prefix: String = chars[..max_chars].iter().collect();
    format!("{prefix}***")
}

/// 把相对路径转为绝对路径（基于当前工作目录）；输入已是绝对路径则原样返回。
pub fn to_absolute(p: impl AsRef<Path>) -> std::path::PathBuf {
    let p = p.as_ref();
    if p.is_absolute() {
        return p.to_path_buf();
    }
    std::env::current_dir().map_or_else(|_| p.to_path_buf(), |cwd| cwd.join(p))
}

/// 把字节数格式化为人类可读的文件大小（"1.5 MB" / "0 B"）。
///
/// 复用旧 `src/ui/pages/library.rs` 的实现 — Stage 5 把它移到 `util::fs`，
/// 旧 UI / 新 GPUI 都能直接调。旧 UI 的本地副本 Stage 11 一起删。
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.2} GB", u64_to_f64(bytes) / u64_to_f64(GB))
    } else if bytes >= MB {
        format!("{:.2} MB", u64_to_f64(bytes) / u64_to_f64(MB))
    } else if bytes >= KB {
        format!("{:.1} KB", u64_to_f64(bytes) / u64_to_f64(KB))
    } else {
        format!("{bytes} B")
    }
}

/// `u64` → `f64` 的转换（分解为两个 u32 避免 `cast_precision_loss`）。
/// 仅用于 UI 显示这一类"结果给人看、精度损失可接受"的场景。
/// u32 → f64 是精确的（53 bits mantissa 能覆盖全部 32 bits），
/// 先分解再合并不会损失精度；不要在算术管线里复用。
#[inline]
const fn u64_to_f64(v: u64) -> f64 {
    let lo = (v as u32) as f64;
    let hi = ((v >> 32) as u32) as f64;
    hi * 4_294_967_296.0 + lo
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn sanitize_filename_strips_path_separators() {
        let s = sanitize_filename("foo/bar\\baz");
        assert!(!s.contains('/'));
        assert!(!s.contains('\\'));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn sanitize_filename_windows_special() {
        let s = sanitize_filename("a:b*c?d\"e<f>g|h");
        assert!(!s.contains(':'));
        assert!(!s.contains('*'));
        assert!(!s.contains('?'));
        assert!(!s.contains('"'));
        assert!(!s.contains('<'));
        assert!(!s.contains('>'));
        assert!(!s.contains('|'));
    }

    #[test]
    fn to_absolute_idempotent_on_abs() {
        let abs = if cfg!(windows) {
            std::path::PathBuf::from("C:\\tmp\\x")
        } else {
            std::path::PathBuf::from("/tmp/x")
        };
        assert_eq!(to_absolute(&abs), abs);
    }

    #[test]
    fn format_size_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.00 MB");
        assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn truncate_log_short_string_unchanged() {
        assert_eq!(truncate_log("abc", 10), "abc");
        assert_eq!(truncate_log("12345", 5), "12345");
    }

    #[test]
    fn truncate_log_long_string_truncated() {
        assert_eq!(truncate_log("abcdefghijk", 5), "abcde***");
        assert_eq!(truncate_log("很长的中文关键词测试", 4), "很长的中***");
    }

    #[test]
    fn truncate_log_empty_string() {
        assert_eq!(truncate_log("", 10), "");
    }
}
