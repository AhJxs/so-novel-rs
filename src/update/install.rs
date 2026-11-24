//! 用暂存的新主程序替换正在运行的二进制，并提供重启与残留清理。
//!
//! Windows 不允许覆盖运行中的 exe：先把旧文件改名成 `<exe>.old`，再把新文件改名就位，
//! 失败时回滚。Unix 直接 `rename` 覆盖（同一文件系统内原子替换）。

use std::path::{Path, PathBuf};

use crate::config::ConfigPaths;
use crate::core::update::UpdateError;

/// 把 `staged` 安装为当前主程序，返回主程序路径（**替换前**解析，供重启使用）。
///
/// # Errors
///
/// 复制新文件、备份旧文件或改名就位失败时返回 [`UpdateError`]；任何一步失败都会
/// 尽量回滚到旧版本，调用方可继续运行当前进程。
pub fn install_binary(staged: &Path) -> Result<PathBuf, UpdateError> {
    let exe =
        std::env::current_exe().map_err(|e| UpdateError(format!("定位当前主程序失败: {e}")))?;
    replace_executable(staged, &exe)?;
    Ok(exe)
}

/// 启动新版本主程序，参数与当前进程一致。
///
/// # Errors
///
/// 无法创建子进程时返回 [`UpdateError`]。
pub fn spawn_restarted(exe: &Path) -> Result<(), UpdateError> {
    std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .spawn()
        .map(|_| ())
        .map_err(|e| UpdateError(format!("启动新版本失败: {e}")))
}

/// 清理上次更新留下的备份与暂存目录；启动成功后调用，失败一律忽略。
///
/// 只在确认本进程拿到运行数据目录后再调用，避免第二个实例误删正在下载的包。
pub fn cleanup_after_update() {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        remove_leftovers(dir, &exe);
    }
    let _ = std::fs::remove_dir_all(ConfigPaths::discover().updates_dir);
}

/// 用 `staged` 替换 `exe`。
pub(crate) fn replace_executable(staged: &Path, exe: &Path) -> Result<(), UpdateError> {
    let dir = exe
        .parent()
        .ok_or_else(|| UpdateError("主程序路径没有父目录".into()))?;
    let name = exe
        .file_name()
        .ok_or_else(|| UpdateError("主程序路径没有文件名".into()))?
        .to_string_lossy();
    let incoming = dir.join(format!(".{name}.new"));
    let backup = dir.join(format!("{name}.old"));

    std::fs::copy(staged, &incoming)
        .map_err(|e| UpdateError(format!("复制新版本失败（目录可能不可写）: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&incoming, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| UpdateError(format!("设置执行权限失败: {e}")))?;
    }

    swap(&incoming, &backup, exe)
}

#[cfg(windows)]
fn swap(incoming: &Path, backup: &Path, exe: &Path) -> Result<(), UpdateError> {
    let _ = std::fs::remove_file(backup);
    retry_rename(exe, backup).map_err(|e| UpdateError(format!("备份旧版本失败: {e}")))?;
    if let Err(e) = retry_rename(incoming, exe) {
        // 回滚：旧版本仍完整，删掉没就位的新文件。
        let _ = std::fs::rename(backup, exe);
        let _ = std::fs::remove_file(incoming);
        return Err(UpdateError(format!("替换主程序失败: {e}")));
    }
    Ok(())
}

#[cfg(not(windows))]
fn swap(incoming: &Path, _backup: &Path, exe: &Path) -> Result<(), UpdateError> {
    std::fs::rename(incoming, exe).map_err(|e| {
        let _ = std::fs::remove_file(incoming);
        UpdateError(format!("替换主程序失败: {e}"))
    })
}

/// Windows 上杀毒软件/索引器会短暂占用刚写的文件，重试几次再放弃。
#[cfg(windows)]
fn retry_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    const ATTEMPTS: u64 = 5;
    let mut last = None;
    for attempt in 0..ATTEMPTS {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(100 * (attempt + 1)));
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("重命名重试耗尽")))
}

fn remove_leftovers(dir: &Path, exe: &Path) {
    let Some(name) = exe.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return;
    };
    let _ = std::fs::remove_file(dir.join(format!("{name}.old")));
    let _ = std::fs::remove_file(dir.join(format!(".{name}.new")));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn fake_exe(dir: &Path) -> PathBuf {
        let exe = dir.join(if cfg!(windows) {
            "so-novel-rs.exe"
        } else {
            "so-novel-rs"
        });
        std::fs::write(&exe, b"old").unwrap();
        exe
    }

    #[test]
    fn replaces_executable_and_keeps_backup() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_exe(dir.path());
        let staged = dir.path().join("staged");
        std::fs::write(&staged, b"new").unwrap();

        replace_executable(&staged, &exe).unwrap();

        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        #[cfg(windows)]
        {
            let backup = dir.path().join(format!(
                "{}.old",
                exe.file_name().unwrap().to_string_lossy()
            ));
            assert_eq!(std::fs::read(backup).unwrap(), b"old");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&exe).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
        assert!(!dir.path().join(".so-novel-rs.new").exists());
    }

    #[test]
    fn replace_fails_when_staged_missing() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_exe(dir.path());

        let err = replace_executable(&dir.path().join("nope"), &exe).unwrap_err();

        assert!(err.to_string().contains("复制新版本失败"));
        assert_eq!(std::fs::read(&exe).unwrap(), b"old");
    }

    #[test]
    fn remove_leftovers_ignores_absent_files() {
        let dir = tempfile::tempdir().unwrap();
        let exe = fake_exe(dir.path());
        let old = dir.path().join(format!(
            "{}.old",
            exe.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(&old, b"old").unwrap();

        remove_leftovers(dir.path(), &exe);
        remove_leftovers(dir.path(), &exe);

        assert!(!old.exists());
        assert!(exe.exists());
    }
}
