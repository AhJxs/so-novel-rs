//! 本地书库业务方法。

use std::path::{Path, PathBuf};

use super::super::library_state::{LibraryState, scan_library_dir};

/// 扫描 `download_path` 下所有已生成的电子书文件。
pub fn refresh_library(library: &mut LibraryState, download_path: &str) {
    let dir = PathBuf::from(download_path);
    let abs = if dir.is_absolute() {
        dir.clone()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&dir))
            .unwrap_or(dir)
    };
    library.scanned_dir = Some(abs.clone());
    library.entries.clear();
    library.last_error = None;
    library.pending_delete = None;

    if !abs.exists() {
        return;
    }

    match scan_library_dir(&abs) {
        Ok(mut entries) => {
            entries.sort_by_key(|b| std::cmp::Reverse(b.modified_unix_secs));
            library.entries = entries;
        }
        Err(e) => {
            library.last_error = Some(
                crate::i18n::ts_fmt("Toasts.library_scan_failed", &[("err", &e.to_string())])
                    .to_string(),
            );
        }
    }
}

/// 真正删除一个本地文件；**外科式**从 `entries` 中移除（不调用 `refresh_library`）。
/// 返回 Ok(成功 toast 文案) / Err(错误 toast 文案)。
///
/// **为什么不做 rescan**：`refresh_library` 会 `entries.clear()` 后再 fill，
/// 中间会被 watcher 后续 fs 事件再次触发 → 用户看到 "empty → 重新加载" 的闪一下。
/// 删除本身是可预测的（删哪个文件已知），直接 `retain` 掉对应 entry 即可：
/// 瞬间、零空态。watcher 在 1s 内看到的 fs 事件由 `watcher_skip_until_unix_ms` 抑制，
/// 避免 race。
pub fn delete_library_entry(
    library: &mut LibraryState,
    _download_path: &str,
    path: &Path,
) -> Result<String, String> {
    let result = match std::fs::remove_file(path) {
        Ok(_) => {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let display = if file_name.is_empty() {
                crate::i18n::ts("Toasts.library_delete_unknown").to_string()
            } else {
                crate::utils::formatting::truncate(&file_name, 50).to_string()
            };
            Ok(crate::i18n::ts_fmt("Toasts.library_delete_ok", &[("file", &display)]).to_string())
        }
        Err(e) => {
            let msg =
                crate::i18n::ts_fmt("Toasts.library_delete_failed", &[("err", &e.to_string())])
                    .to_string();
            library.last_error = Some(msg.clone());
            Err(msg)
        }
    };
    library.pending_delete = None;
    // 外科式移除：路径完全相等才删，避开 path 末尾不同但 basename 相同的边界情况。
    // 即便文件已经成功删除（result=Ok），entry 仍在内存里 → 显式过滤。
    library.entries.retain(|e| e.path != path);
    // 抑制 watcher 在接下来 1 秒内因 fs 事件触发的 rescan —— 删除瞬间文件系统
    // 一定会发 Modify/Remove 事件，watcher 300ms debounce 后会调 refresh_library_async
    // 把 entries.clear() 再 fill，制造"empty → 重新加载"闪一下。1s 覆盖 rescan 全程。
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    library.watcher_skip_until_unix_ms = Some(now_ms + 1000);
    result
}
