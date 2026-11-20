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

#[cfg(target_os = "windows")]
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x0000_0002;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 给缓存根目录设隐藏属性（Windows）。仓库禁 `unsafe`，所以调系统自带的 `attrib`。
///
/// 真实文件属性是唯一真相：不存在的路径直接返回（`attrib` 对缺失路径也返回 0），
/// 已隐藏则跳过。目录被删掉重建后属性随之丢失，下次调用会重新隐藏，不需要进程内状态。
#[cfg(target_os = "windows")]
pub fn hide_dir(dir: &Path) {
    use std::os::windows::fs::MetadataExt as _;
    use std::os::windows::process::CommandExt as _;
    use std::process::Command;

    let Ok(metadata) = std::fs::metadata(dir) else {
        return; // 不存在的路径不进任何状态（`attrib` 对缺失路径也返回 0）。
    };
    if metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0 {
        return;
    }
    let mut command = Command::new("attrib");
    command.arg("+h").arg(dir);
    command.creation_flags(CREATE_NO_WINDOW);
    let _ = command.status();
}

/// 非 Windows 上没有隐藏属性可设，点开头的目录名已经足够。
///
/// 空实现要写成 `const fn`：Linux 的 clippy 会建议
/// （`missing_const_for_fn`），Windows 分支因为要起 `attrib` 进程而做不到，两侧差异无妨。
#[cfg(not(target_os = "windows"))]
pub const fn hide_dir(_dir: &Path) {}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    /// 只给 Windows 测试用：读真实文件属性判断目录是否隐藏。
    #[cfg(target_os = "windows")]
    fn is_hidden(dir: &Path) -> bool {
        use std::os::windows::fs::MetadataExt as _;
        std::fs::metadata(dir).is_ok_and(|metadata| metadata.file_attributes() & 0x2 != 0)
    }

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

    #[test]
    fn hide_dir_on_missing_path_does_not_create_it() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        hide_dir(&missing);
        assert!(!missing.exists());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn missing_path_does_not_poison_later_calls() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        hide_dir(&missing);
        assert!(!missing.exists());

        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        hide_dir(&real);
        assert!(is_hidden(&real), "真实目录必须被隐藏");

        // 删掉重建：进程内没有残留状态，新建目录先不带隐藏属性。
        std::fs::remove_dir(&real).unwrap();
        std::fs::create_dir(&real).unwrap();
        assert!(!is_hidden(&real), "重建后不能残留隐藏状态");
        hide_dir(&real);
        assert!(is_hidden(&real), "重建后的目录必须再次被隐藏");
    }
}
