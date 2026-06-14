//! 本地书库：条目、状态、目录扫描。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct LibraryEntry {
    pub path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
    /// 文件修改时间。Unix 时间戳（秒）；获取失败时为 0。
    pub modified_unix_secs: u64,
    /// 扩展名（小写、不含点）：epub / txt / zip / html / pdf / 其它。
    pub ext: String,
}

#[derive(Default)]
pub struct LibraryState {
    /// 当前扫描结果（已按修改时间倒序）。
    pub entries: Vec<LibraryEntry>,
    /// 用户输入的搜索关键字（按文件名过滤）。
    pub filter_text: String,
    /// 用户选的格式过滤（None = 全部）。
    pub filter_ext: Option<String>,
    /// 上次扫描时的下载目录绝对路径（变化时自动重扫）。
    pub scanned_dir: Option<PathBuf>,
    /// 待删除确认中的条目路径；点删除后置位，再次点确认才真正删除。
    pub pending_delete: Option<PathBuf>,
    /// 上次扫描 / 操作失败提示。
    pub last_error: Option<String>,
}

/// 扫描下载目录得到 LibraryEntry 列表。
///
/// - 仅包含**直接子文件**（不递归子目录）。
/// - 仅保留 `.epub / .txt / .zip / .html / .pdf` 五种扩展名。
pub fn scan_library_dir(dir: &Path) -> std::io::Result<Vec<LibraryEntry>> {
    const KEEP_EXT: &[&str] = &["epub", "txt", "zip", "html", "pdf"];

    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext_raw) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        let ext = ext_raw.to_ascii_lowercase();
        if !KEEP_EXT.contains(&ext.as_str()) {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let meta = entry.metadata()?;
        let size_bytes = meta.len();
        let modified_unix_secs = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);

        out.push(LibraryEntry {
            path,
            file_name,
            size_bytes,
            modified_unix_secs,
            ext,
        });
    }
    Ok(out)
}
