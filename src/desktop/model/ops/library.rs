//! 本地书库业务方法。

use std::path::{Path, PathBuf};

use super::super::library_state::{LibraryState, scan_library_dir};

/// 扫描 `download_path` 下所有已生成的电子书文件。
pub fn refresh_library(library: &mut LibraryState, download_path: &str) {
    let dir = PathBuf::from(download_path);
    let abs = if dir.is_absolute() {
        dir
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

/// 真正删除一个本地文件；**外科式**从 `entries` 中移除（不调 `refresh_library`）。
/// 返回 Ok(成功 toast 文案) / Err(错误 toast 文案)。
///
/// 不做 rescan: `refresh_library` 的 `clear()` + fill 会让用户看到 "empty → 重新加载"
/// 闪一下, 而删除目标是确定的, 直接 `retain` 对应 entry 即可。
pub fn delete_library_entry(
    library: &mut LibraryState,
    _download_path: &str,
    path: &Path,
) -> crate::error::AppResult<String> {
    let result = match std::fs::remove_file(path) {
        Ok(()) => {
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            let display = if file_name.is_empty() {
                crate::i18n::ts("Toasts.library_delete_unknown").to_string()
            } else {
                crate::utils::formatting::truncate(&file_name, 50)
            };
            Ok(crate::i18n::ts_fmt("Toasts.library_delete_ok", &[("file", &display)]).to_string())
        }
        Err(e) => {
            let msg =
                crate::i18n::ts_fmt("Toasts.library_delete_failed", &[("err", &e.to_string())])
                    .to_string();
            library.last_error = Some(msg.clone());
            Err(crate::error::AppError::internal(msg))
        }
    };
    library.pending_delete = None;
    // 路径完全相等才删, 避开末尾不同但 basename 相同的边界情况。
    library.entries.retain(|e| e.path != path);
    // bump `entries_version` 让渲染端 ListCache 失效 —— 不 bump 的话 cache key 不变,
    // 下次 render 命中旧 Arc（仍含被删 entry), UI 不刷新。
    library.entries_version = library.entries_version.wrapping_add(1);
    result
}
