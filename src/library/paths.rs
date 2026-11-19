//! 下载目录下的隐藏缓存根：封面缓存与阅读解析缓存共用同一套路径规则。
//!
//! 结构：`<下载目录>/.sonovel/covers/`、`<下载目录>/.sonovel/books/`。
//! 下载目录为空时不缓存任何东西（返回 `None`），与阅读缓存的既有行为一致。

use std::path::{Path, PathBuf};

use super::book::absolute_dir;

/// 缓存根目录名：放在下载目录下，Windows 上额外设隐藏属性。
pub const CACHE_DIR_NAME: &str = ".sonovel";
/// 封面缓存子目录。
pub const COVERS_DIR_NAME: &str = "covers";
/// 阅读解析缓存子目录。
pub const BOOKS_DIR_NAME: &str = "books";

/// `<下载目录>/.sonovel`；下载目录为空时返回 `None`。
#[must_use]
pub fn cache_root(download_path: &str) -> Option<PathBuf> {
    let trimmed = download_path.trim();
    (!trimmed.is_empty()).then(|| absolute_dir(Path::new(trimmed)).join(CACHE_DIR_NAME))
}

/// `<下载目录>/.sonovel/covers`；下载目录为空时返回 `None`。
#[must_use]
pub fn covers_dir_for(download_path: &str) -> Option<PathBuf> {
    cache_root(download_path).map(|root| root.join(COVERS_DIR_NAME))
}

/// 给缓存根目录设隐藏属性（Windows）。仓库禁 `unsafe`，所以调系统自带的 `attrib`，
/// 每次进程运行最多成功设一次；点开头的名字在其它平台已经足够。
#[cfg(target_os = "windows")]
pub fn hide_dir(dir: &Path) {
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};

    static HIDDEN: AtomicBool = AtomicBool::new(false);
    if HIDDEN.load(Ordering::Relaxed) {
        return;
    }
    // `attrib` 只接受已存在的路径；调用点都在 `create_dir_all` 之后。
    let status = Command::new("attrib").arg("+h").arg(dir).status();
    if matches!(status, Ok(status) if status.success()) {
        HIDDEN.store(true, Ordering::Relaxed);
    }
}

/// 非 Windows 上没有隐藏属性可设，点开头的目录名已经足够。
#[cfg(not(target_os = "windows"))]
pub fn hide_dir(_dir: &Path) {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn blank_download_path_has_no_cache() {
        assert!(cache_root("").is_none());
        assert!(cache_root("   ").is_none());
        assert!(covers_dir_for("").is_none());
    }

    #[test]
    fn covers_live_under_hidden_root() {
        let root = cache_root("C:/books").unwrap();
        assert!(root.is_absolute());
        assert!(root.ends_with(CACHE_DIR_NAME));
        let covers = covers_dir_for("C:/books").unwrap();
        assert!(covers.ends_with(Path::new(CACHE_DIR_NAME).join(COVERS_DIR_NAME)));
    }
}
