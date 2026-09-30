//! 三端共享的下载文件元数据 + 扩展名常量 + 文件打开 helper。
//!
//! Web `handlers/library.rs` 与桌面 `model/library_state.rs::scan_library_dir` 都要"扫下载目录 /
//! 列条目 / 算 ext"，各自维护一份白名单字面量容易漏改一边，统一用这里的常量。
//!
//! `open_download_file` 不依赖 HTTP 类型（返回 `Result<_, OpenFileError>`，由 handler 映射成
//! `(StatusCode, String)`），所以放 `core`，桌面将来也能复用。
//! **`ext` 字段必须保留** —— web-ui `library.tsx` 靠它做按类型分页 + Badge count，砍了 4 个 tab 全失效。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::utils::fs::sanitize_filename;

/// GUI + Web 共用的下载文件扩展名白名单（**单一事实来源**）。
/// web `library_list` 与桌面 `scan_library_dir` 都引用这里，新增 / 删除格式只改这一处。
pub const SUPPORTED_LIBRARY_EXTS: &[&str] = &["epub", "txt", "html", "zip", "pdf", "md"];

/// 扩展名 → MIME type（Web 用作 `Content-Type`；CLI / GUI 忽略）。
/// 白名单外 / 解析不出返回 `None`，调用方通常回退到 `"application/octet-stream"`。
pub fn extension_to_content_type(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "epub" => Some("application/epub+zip"),
        "txt" => Some("text/plain; charset=utf-8"),
        "html" | "zip" => Some("application/zip"),
        "pdf" => Some("application/pdf"),
        "md" => Some("text/markdown; charset=utf-8"),
        _ => None,
    }
}

/// 单条 library 条目（GUI + Web 共用 DTO）。
///
/// `Serialize` 给 web 直接当 JSON 返回，字段名与 web-ui 的 `LibraryFile` interface 对齐；
/// `ext` 必须保留（见模块顶部）。
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

/// 列出目录下所有支持的 library 条目，按 mtime 倒序（最新在前）。
///
/// `read_dir` 失败返回空 Vec —— 这是 UI 入口，当空目录显示比 500 错误友好；
/// 调用方要区分"空目录 vs 目录不存在"可自己 `exists()` / `is_dir()`。
pub fn list_library_entries(dir: &Path) -> Vec<LibraryEntry> {
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            if let Some(e) = LibraryEntry::from_path(&entry.path()) {
                entries.push(e);
            }
        }
    }
    entries.sort_by_key(|b| std::cmp::Reverse(b.modified_unix));
    entries
}

/// `open_download_file` 的错误类型；Web 把它映射成 `(StatusCode, String)`，CLI / GUI 按需处理。
#[derive(Debug)]
pub enum OpenFileError {
    /// 文件不存在 / sanitize 后路径不存在。
    NotFound,
    /// 读字节失败（permission / IO 错误）。
    Io(String),
}

impl std::fmt::Display for OpenFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("文件未找到"),
            Self::Io(e) => f.write_str(e),
        }
    }
}

/// "在下载目录里安全定位一个文件" 共享 helper。
///
/// `sanitize_filename` 防 `../` 注入 → 拼出 `download_dir/safe` → 检查存在；删除 / 下载都先调它。
/// **不**做 mtime / ext 白名单检查 —— caller 拿到路径后自己做后续动作。
pub fn safe_file_path(download_dir: &Path, filename: &str) -> Result<PathBuf, OpenFileError> {
    let safe = sanitize_filename(filename);
    let path = download_dir.join(&safe);
    if !path.exists() {
        return Err(OpenFileError::NotFound);
    }
    Ok(path)
}

/// "打开下载文件" 高阶 helper：`safe_file_path` + 读字节 + 解析 content-type。
/// 调用方（web `file_download`）一行拿到 `(bytes, content_type)`，再自己把 [`OpenFileError`]
/// 映射成 `(StatusCode, String)`。
pub async fn open_download_file(
    download_dir: &Path,
    filename: &str,
) -> Result<(Vec<u8>, &'static str), OpenFileError> {
    let path = safe_file_path(download_dir, filename)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| OpenFileError::Io(e.to_string()))?;
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    // 来自 `list_library_entries` 的 filename 一定 ext 合法；但 `file_download` 是公开端点
    //（URL 里随便塞名字），所以仍走 `extension_to_content_type` 而不是 `unwrap_or_default`。
    let content_type = extension_to_content_type(ext).unwrap_or("application/octet-stream");
    Ok((bytes, content_type))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

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
        // 锁死长度 —— 防新加 / 删除扩展名时漏改 web-ui Badge count 或 i18n key。
        assert_eq!(SUPPORTED_LIBRARY_EXTS.len(), 6);
    }

    #[test]
    fn content_type_known_extensions() {
        assert_eq!(
            extension_to_content_type("epub"),
            Some("application/epub+zip")
        );
        assert_eq!(
            extension_to_content_type("txt"),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(extension_to_content_type("html"), Some("application/zip"));
        assert_eq!(extension_to_content_type("zip"), Some("application/zip"));
        assert_eq!(extension_to_content_type("pdf"), Some("application/pdf"));
        assert_eq!(
            extension_to_content_type("md"),
            Some("text/markdown; charset=utf-8")
        );
    }

    #[test]
    fn content_type_case_insensitive() {
        // 大写 / 混合大小写都映射到同一 MIME —— helper 自己也要 robust。
        assert_eq!(
            extension_to_content_type("EPUB"),
            Some("application/epub+zip")
        );
        assert_eq!(extension_to_content_type("Pdf"), Some("application/pdf"));
    }

    #[test]
    fn content_type_unknown_returns_none() {
        assert_eq!(extension_to_content_type("docx"), None);
        assert_eq!(extension_to_content_type(""), None);
        assert_eq!(extension_to_content_type("mobi"), None);
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

    #[test]
    fn list_returns_empty_for_missing_dir() {
        let p = std::path::PathBuf::from("/nonexistent/sonovel-core-library-list-missing");
        assert!(list_library_entries(&p).is_empty());
    }

    #[test]
    fn list_skips_unsupported_extensions() {
        let dir = temp_dir("list-mixed");
        touch(&dir.join("a.epub"), b"e");
        touch(&dir.join("b.docx"), b"d");
        touch(&dir.join("c.txt"), b"t");
        touch(&dir.join("d"), b"x"); // no ext
        let entries = list_library_entries(&dir);
        let names: Vec<_> = entries.iter().map(|e| e.filename.as_str()).collect();
        assert_eq!(names, vec!["a.epub", "c.txt"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_sorts_by_mtime_descending() {
        let dir = temp_dir("list-sort");
        // 隔一小段时间让 mtime 明显不同（Windows / FAT 精度可能是 2s，但"后写"仍排在前）。
        touch(&dir.join("older.epub"), b"e");
        std::thread::sleep(std::time::Duration::from_millis(50));
        touch(&dir.join("newer.epub"), b"e");
        let entries = list_library_entries(&dir);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].filename, "newer.epub");
        assert_eq!(entries[1].filename, "older.epub");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_skips_subdirectories() {
        let dir = temp_dir("list-subdir");
        std::fs::create_dir(dir.join("nested")).expect("mkdir");
        touch(&dir.join("nested/inside.epub"), b"e");
        touch(&dir.join("top.epub"), b"e");
        let entries = list_library_entries(&dir);
        let names: Vec<_> = entries.iter().map(|e| e.filename.as_str()).collect();
        assert_eq!(names, vec!["top.epub"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn safe_file_path_returns_path_for_existing_file() {
        let dir = temp_dir("safe-ok");
        touch(&dir.join("a.epub"), b"e");
        let path = safe_file_path(&dir, "a.epub").expect("path");
        assert_eq!(path, dir.join("a.epub"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn safe_file_path_rejects_traversal() {
        let dir = temp_dir("safe-traverse");
        // "../" 被 sanitize_filename 清掉，不会逃出 dir。
        let result = safe_file_path(&dir, "../../etc/passwd");
        assert!(matches!(result, Err(OpenFileError::NotFound)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn safe_file_path_not_found_for_missing() {
        let dir = temp_dir("safe-missing");
        let result = safe_file_path(&dir, "missing.epub");
        assert!(matches!(result, Err(OpenFileError::NotFound)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_file_error_display() {
        assert_eq!(OpenFileError::NotFound.to_string(), "文件未找到");
        assert_eq!(
            OpenFileError::Io("permission denied".into()).to_string(),
            "permission denied"
        );
    }
}
