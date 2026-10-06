//! 下载文件元数据 + 扩展名常量。
//!
//! 桌面 `model/library_state.rs::scan_library_dir` 扫描下载目录时用这里的常量，
//! 白名单集中一处避免漏改。

use std::path::Path;
use std::time::SystemTime;

use serde::Serialize;

/// 下载文件扩展名白名单（**单一事实来源**）。
/// 桌面 `scan_library_dir` 引用这里，新增 / 删除格式只改这一处。
pub const SUPPORTED_LIBRARY_EXTS: &[&str] = &["epub", "txt", "html", "zip", "pdf", "md"];

/// 单条 library 条目。
#[derive(Debug, Clone, Serialize)]
pub struct LibraryEntry {
    pub filename: String,
    pub ext: String,
    pub modified_unix: i64,
    pub size_bytes: u64,
}

impl LibraryEntry {
    /// 从一条候选文件路径构造 entry；过滤一步到位，调用方拿到的 Vec 已经是"该展示的"。
    ///
    /// 返回 `None`：路径不是 regular file、扩展名不在 [`SUPPORTED_LIBRARY_EXTS`] 白名单（或解析不出）、
    /// 元数据读不出（permission / IO 错误）。
    pub fn from_path(path: &Path) -> Option<Self> {
        if !path.is_file() {
            return None;
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)?;
        if !SUPPORTED_LIBRARY_EXTS.contains(&ext.as_str()) {
            return None;
        }
        let filename = path.file_name().and_then(|s| s.to_str())?.to_string();
        let meta = path.metadata().ok()?;
        let size_bytes = meta.len();
        let modified_unix = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs().cast_signed());
        Some(Self {
            filename,
            ext,
            modified_unix,
            size_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn supported_exts_contains_expected() {
        for ext in ["epub", "txt", "html", "zip", "pdf", "md"] {
            assert!(
                SUPPORTED_LIBRARY_EXTS.contains(&ext),
                "{ext} should be in SUPPORTED_LIBRARY_EXTS"
            );
        }
    }

    #[test]
    fn supported_exts_length_is_six() {
        // 锁死长度 —— 防新加 / 删除扩展名时漏改下游消费方的分页 / 计数逻辑。
        assert_eq!(SUPPORTED_LIBRARY_EXTS.len(), 6);
    }

    fn touch(path: &Path, content: &[u8]) {
        std::fs::write(path, content).expect("write");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "sonovel-core-library-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).expect("mkdir");
        p
    }

    #[test]
    fn from_path_returns_none_for_directory() {
        let dir = temp_dir("dir");
        assert!(LibraryEntry::from_path(&dir).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_returns_none_for_unsupported_ext() {
        let dir = temp_dir("unsupported");
        let f = dir.join("book.docx");
        touch(&f, b"x");
        assert!(LibraryEntry::from_path(&f).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_returns_none_for_no_extension() {
        let dir = temp_dir("noext");
        let f = dir.join("README");
        touch(&f, b"x");
        assert!(LibraryEntry::from_path(&f).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_returns_entry_for_supported_file() {
        let dir = temp_dir("ok");
        let f = dir.join("book.epub");
        touch(&f, b"epub-body");
        let entry = LibraryEntry::from_path(&f).expect("entry");
        assert_eq!(entry.filename, "book.epub");
        assert_eq!(entry.ext, "epub");
        assert_eq!(entry.size_bytes, b"epub-body".len() as u64);
        assert!(entry.modified_unix > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_normalizes_extension_case() {
        let dir = temp_dir("case");
        let f = dir.join("book.EPUB");
        touch(&f, b"x");
        let entry = LibraryEntry::from_path(&f).expect("entry");
        assert_eq!(entry.ext, "epub", "ext must be lowercase in DTO");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_returns_none_for_missing_file() {
        let p = std::path::PathBuf::from("/nonexistent/path/never-exists.epub");
        assert!(LibraryEntry::from_path(&p).is_none());
    }
}
