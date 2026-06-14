//! 本地书库业务方法。

use std::path::{Path, PathBuf};

use super::super::library_state::{scan_library_dir, LibraryState};

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
            library.last_error = Some(format!("扫描下载目录失败: {e}"));
        }
    }
}

/// 真正删除一个本地文件；删完后立即重扫。
/// 返回 Ok(成功 toast 文案) / Err(错误 toast 文案)。
pub fn delete_library_entry(
    library: &mut LibraryState,
    download_path: &str,
    path: &Path,
) -> Result<String, String> {
    let result = match std::fs::remove_file(path) {
        Ok(_) => Ok(format!(
            "已删除: {}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("（未知）")
        )),
        Err(e) => {
            let msg = format!("删除失败: {e}");
            library.last_error = Some(msg.clone());
            Err(msg)
        }
    };
    // 即使删成功也清掉 pending 并重扫
    library.pending_delete = None;
    refresh_library(library, download_path);
    result
}
