//! 三端共享的下载文件元数据 + 扩展名常量 + 文件打开 helper。
//!
//! ## 为什么在 `core`?
//!
//! Web `handlers/library.rs` 和桌面 `model/library_state.rs::scan_library_dir` 都
//! 要做"扫下载目录 / 列条目 / 算 ext"。两边都维护一份 `["epub", "txt", "html", "zip",
//! "pdf", "md"]` 白名单字面量，新增格式时容易漏改一边 —— 抽到 `core::library` 用同一份常量。
//!
//! `open_download_file` 是 web `file_download` 的核心：`sanitize_filename` + path check +
//! 读字节 + 解析 content-type。放在 `core`（而不是 web）是因为它**不**依赖 HTTP
//! 类型 —— 返回 `Result<(_, _), OpenFileError>`，由 web handler 决定怎么映射成
//! `(StatusCode, String)`。这样未来桌面想做"打开本地下载文件"也能直接复用。
//!
//! ## 关于 `ext` 字段
//!
//! **`ext` 字段必须保留** —— web client (`web-ui/src/routes/library.tsx`) 拿这个字段
//! 做 filter (按类型分页 + Badge count)。如果砍了，前端 4 个 tab 全部失效。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::utils::fs::sanitize_filename;

/// GUI + Web 共用的下载文件扩展名白名单。
///
/// **单一事实来源** —— web `library_list` 和桌面 `scan_library_dir` 都引用这里，
/// 新增 / 删除扩展名只需改这一处。
pub const SUPPORTED_LIBRARY_EXTS: &[&str] = &["epub", "txt", "html", "zip", "pdf", "md"];

/// 扩展名 → MIME type。Web 用作 `Content-Type`；CLI / GUI 忽略。
///
/// 返回 `None` 当扩展名不在 [`SUPPORTED_LIBRARY_EXTS`] 白名单 / 解析不出。
/// 调用方拿到 `None` 时通常回退到 `"application/octet-stream"`。
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
/// `Serialize` 派生给 web 直接当 JSON 返回。`filename` / `ext` 都用 `snake_case`，
/// 跟 web-ui 的 `LibraryFile` interface 对齐（`{ filename, ext, size, modified }`）。
///
/// **`ext` 字段必须保留** —— 见模块顶部注释。
#[derive(Debug, Clone, Serialize)]
pub struct LibraryEntry {
    pub filename: String,
    pub ext: String,
    pub modified_unix: i64,
    pub size_bytes: u64,
}

impl LibraryEntry {
    /// 从一条候选文件路径构造 entry。
    ///
    /// 返回 `None` 当：
    /// - 路径不是 regular file（目录 / symlink-broken / 不存在）
    /// - 扩展名不在 [`SUPPORTED_LIBRARY_EXTS`] 白名单（**或**解析不出）
    /// - 元数据读不出（permission / IO 错误）
    ///
    /// 设计：所有过滤一步完成，调用方拿到的 Vec 已经是"该展示的"列表。
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
/// `read_dir` 失败时返回空 Vec —— `library_list` 是 UI 入口，扫不到目录就当空目录
/// 显示，比 500 错误友好。Web 调用方如果需要区分"空目录 vs 目录不存在"，可以
/// 自己 `dir.exists()` + `dir.is_dir()` 判一下。
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

/// `open_download_file` 的错误类型。Web 把它映射成 `(StatusCode, String)`；
/// CLI / GUI 拿到后按需处理。
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
/// 流程：`sanitize_filename(filename)` 防 `../` 注入 → 拼出 `download_dir/safe` →
/// 检查存在 → 返回 `PathBuf`。删除 / 下载都先调这个拿到合法路径。
///
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
///
/// 调用方（web `file_download`）一行拿到 `(bytes, content_type)`：
/// ```ignore
/// let (bytes, content_type) = open_download_file(&state.download_path, &filename)
///     .await
///     .map_err(|e| match e {
///         OpenFileError::NotFound => (StatusCode::NOT_FOUND, e.to_string()),
///         OpenFileError::Io(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
///     })?;
/// ```
pub async fn open_download_file(
    download_dir: &Path,
    filename: &str,
) -> Result<(Vec<u8>, &'static str), OpenFileError> {
    let path = safe_file_path(download_dir, filename)?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| OpenFileError::Io(e.to_string()))?;
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    // 来自 `list_library_entries` 的 filename 一定 ext 合法；但 `file_download` 是
    // 公开端点（URL 里随便塞名字），所以仍走 `extension_to_content_type` 而不是
    // 直接 `unwrap_or_default`。
    let content_type = extension_to_content_type(ext).unwrap_or("application/octet-stream");
    Ok((bytes, content_type))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    // ---- SUPPORTED_LIBRARY_EXTS ----

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

    // ---- extension_to_content_type ----

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
        // 大写 / 混合大小写都映射到同一 MIME —— ext 在 LibraryEntry 里已 lowercase，
        // 但 helper 自己也要 robust（公开 API 直接用没问题）。
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

    // ---- LibraryEntry::from_path ----

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
        // 目录不通过（is_file() = false）
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

    // ---- list_library_entries ----

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
        // 写两个文件，然后 sleep 一小段时间让 mtime 明显不同 —— Windows 上 mtime 精度可能
        // 是 2s / FAT 上是 2s，但同目录里多文件相对顺序仍由"后写"在前。
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

    // ---- safe_file_path ----

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
