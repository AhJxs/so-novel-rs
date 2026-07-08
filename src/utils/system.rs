//! 跨平台"用系统默认程序打开文件 / 打开所在目录"。
//!
//! 不引入 `opener` crate：直接调系统命令足够。功能就这两个，封装成本远低于
//! 多拉一个依赖。
//!
//! 行为：
//! - `open_path`：Windows 用 `start ""`、macOS 用 `open`、Linux 用 `xdg-open`；
//! - `reveal_in_folder`：Windows 用 `explorer /select,<file>` 高亮选中；
//!   macOS 用 `open -R <file>`；Linux 没有标准命令，回退到打开父目录。

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, anyhow};

/// 用系统默认程序打开文件或目录。
pub fn open_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(anyhow!("path does not exist: {}", path.display()));
    }
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("path not valid UTF-8: {}", path.display()))?;

    #[cfg(target_os = "windows")]
    {
        // `cmd /C start "" "<path>"` —— 第一个 "" 是 start 的窗口标题，必须有，
        // 否则带空格的路径会被当作标题。
        Command::new("cmd")
            .args(["/C", "start", "", path_str])
            .spawn()
            .with_context(|| format!("start {}", path_str))?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path_str)
            .spawn()
            .with_context(|| format!("open {}", path_str))?;
        Ok(())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(path_str)
            .spawn()
            .with_context(|| format!("xdg-open {}", path_str))?;
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        let _ = path_str;
        Err(anyhow!("open_path: unsupported platform"))
    }
}

/// 在文件管理器中显示文件 — Windows / macOS 高亮选中文件，
/// Linux 退回为打开父目录。
pub fn reveal_in_folder(path: &Path) -> Result<()> {
    if !path.exists() {
        return Err(anyhow!("path does not exist: {}", path.display()));
    }

    #[cfg(target_os = "windows")]
    {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("path not valid UTF-8: {}", path.display()))?;
        // explorer 的特殊语法：/select, 后面接绝对路径。
        Command::new("explorer")
            .arg(format!("/select,{path_str}"))
            .spawn()
            .with_context(|| format!("explorer /select,{path_str}"))?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("path not valid UTF-8: {}", path.display()))?;
        Command::new("open")
            .args(["-R", path_str])
            .spawn()
            .with_context(|| format!("open -R {path_str}"))?;
        Ok(())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Linux 没有标准的"显示并选中文件"命令；退回打开父目录。
        let parent = path
            .parent()
            .ok_or_else(|| anyhow!("path has no parent: {}", path.display()))?;
        open_path(parent)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", unix)))]
    {
        Err(anyhow!("reveal_in_folder: unsupported platform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonexistent_path_returns_error() {
        let bogus = Path::new("/definitely/does/not/exist/at/all");
        assert!(open_path(bogus).is_err());
        assert!(reveal_in_folder(bogus).is_err());
    }
}
