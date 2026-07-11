//! 本地书库：条目、状态、目录扫描。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::AppResult;

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

/// 后台扫描完成事件。Ok 直接装 entries；Err 装 [`AppError`]，drain 时调
/// `e.message()` 拿 i18n 渲染后的错误文案。
pub type LibraryScanEvent = AppResult<Vec<LibraryEntry>>;

#[derive(Default)]
pub struct LibraryState {
    /// 当前扫描结果（已按修改时间倒序）。
    pub entries: Vec<LibraryEntry>,
    /// `entries` 改动的单调递增版本号。`drain_scan` 写入新结果时 +1，
    /// UI 渲染用 `(entries_version, filter_hash, page_index)` 缓存过滤/分页结果。
    pub entries_version: u64,
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
    /// 后台扫描进行中（避免重复触发；drain 期间清零）。
    pub scan_in_flight: bool,
    /// 后台扫描任务的 smol channel 接收端 —— 阻塞的 `read_dir` / `metadata` 跑在
    /// `background_executor`，结果通过这里回到主线程。
    pub scan_rx: Option<smol::channel::Receiver<LibraryScanEvent>>,
}

/// 扫描下载目录得到 `LibraryEntry` 列表。
///
/// - 仅包含**直接子文件**（不递归子目录）。
/// - 仅保留 [`crate::core::library::SUPPORTED_LIBRARY_EXTS`] 白名单内的扩展名。
///   白名单由 `core::library` 单一维护，桌面和 web 共用同一份（防止 web 加新格式
///   时漏改桌面）。
pub fn scan_library_dir(dir: &Path) -> std::io::Result<Vec<LibraryEntry>> {
    use crate::core::library::SUPPORTED_LIBRARY_EXTS;

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
        if !SUPPORTED_LIBRARY_EXTS.contains(&ext.as_str()) {
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
            .map_or(0, |d| d.as_secs());

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

impl LibraryState {
    /// 排空后台扫描完成事件。每次 `events::drain` 调一次。返回是否有进展。
    pub fn drain_scan(&mut self) -> bool {
        let mut any = false;
        // 只取一次；若调用方需要再排空，得自己循环（smol channel 一次性把全部
        // 已到的事件都收完）。
        let Some(rx) = self.scan_rx.as_mut() else {
            return false;
        };
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    any = true;
                    match event {
                        Ok(mut entries) => {
                            entries.sort_by_key(|b| std::cmp::Reverse(b.modified_unix_secs));
                            self.entries = entries;
                            self.entries_version = self.entries_version.wrapping_add(1);
                            self.last_error = None;
                        }
                        Err(e) => {
                            self.last_error = Some(e.message());
                        }
                    }
                }
                Err(smol::channel::TryRecvError::Empty) => break,
                Err(smol::channel::TryRecvError::Closed) => {
                    self.scan_rx = None;
                    self.scan_in_flight = false;
                    break;
                }
            }
        }
        any
    }
}
