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
            let replaced = match c {
                '.' => '。',
                ':' => '：',
                '/' => '／',
                '\0' => '_',
                _ => c,
            };
            out.push(replaced);
        }
        out
    } else {
        name.replace('/', "")
    }
}

/// 把相对路径转为绝对路径（基于当前工作目录）；输入已是绝对路径则原样返回。
pub fn to_absolute(p: impl AsRef<Path>) -> std::path::PathBuf {
    let p = p.as_ref();
    if p.is_absolute() {
        return p.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(p))
        .unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
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
}
