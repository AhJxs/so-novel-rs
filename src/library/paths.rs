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

/// 本进程内已经处理过的目录集合。
///
/// 之前这里是「进程级 `AtomicBool`」，但 `attrib +h` 对不存在的路径也会退出 0（本机实测），
/// 只要任何渲染路径在目录创建前调过一次 `hide_dir`，布尔就被置上，之后真正的目录永远不再被隐藏。
/// 改成按路径记录：每个目录各自只尝试一次，不存在的路径根本不进入集合。
#[cfg(target_os = "windows")]
fn hidden_set() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    static HIDDEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> =
        std::sync::OnceLock::new();
    HIDDEN.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// 某个路径是否已登记为「尝试过隐藏」。只给 Windows 测试用，不对外暴露。
///
/// `cfg(test)` 一起卡上：非测试构建里它没有被调用点，否则 `dead_code` 会让
/// `clippy --all-targets -- -D warnings` 失败。
#[cfg(all(target_os = "windows", test))]
fn is_marked_hidden(dir: &Path) -> bool {
    hidden_set()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(dir)
}

/// 给缓存根目录设隐藏属性（Windows）。仓库禁 `unsafe`，所以调系统自带的 `attrib`；
/// 每个目录每次进程运行最多成功发起一次，点开头的名字在其它平台已经足够。
#[cfg(target_os = "windows")]
pub fn hide_dir(dir: &Path) {
    use std::os::windows::process::CommandExt as _;
    use std::process::Command;

    // `attrib` 对不存在的路径也返回 0（本机实测），不能靠退出码判断；
    // 目录不存在时直接返回，避免把路径错误地登记成「已隐藏」。
    if !dir.exists() {
        return;
    }
    {
        let mut hidden = hidden_set()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !hidden.insert(dir.to_path_buf()) {
            return;
        }
    }
    // GUI 子系统里不弹控制台窗口。
    let mut command = Command::new("attrib");
    command.arg("+h").arg(dir);
    command.creation_flags(0x0800_0000);
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
        assert!(!is_marked_hidden(&missing), "不存在的路径不能进已隐藏集合");

        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        hide_dir(&real);
        assert!(is_marked_hidden(&real), "真实目录必须被登记");
    }
}
