//! 用系统默认程序打开文件、在文件管理器中定位文件。三平台各走命令行，不引新依赖。

use std::path::Path;
use std::process::Command;

/// 在文件管理器中显示并选中文件。
///
/// # Errors
///
/// 平台命令启动失败时返回错误文案（可直接进 toast）。
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let (program, args) = reveal_command(path);
    Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 用系统默认程序打开文件。
///
/// # Errors
///
/// 平台命令启动失败时返回错误文案。
pub fn open_with_default_app(path: &Path) -> Result<(), String> {
    let (program, args) = open_command(path);
    Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 「在文件管理器中显示」的平台命令 `(程序, 参数)`。
#[must_use]
pub fn reveal_command(path: &Path) -> (String, Vec<String>) {
    let target = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        // `explorer /select,<路径>` 会打开父目录并选中该文件。
        ("explorer".to_string(), vec![format!("/select,{target}")])
    } else if cfg!(target_os = "macos") {
        ("open".to_string(), vec!["-R".to_string(), target])
    } else {
        // Linux 没有统一的「选中」，退回打开父目录。
        let parent = path
            .parent()
            .map_or(target, |dir| dir.to_string_lossy().into_owned());
        ("xdg-open".to_string(), vec![parent])
    }
}

/// 「用系统程序打开」的平台命令 `(程序, 参数)`。
#[must_use]
pub fn open_command(path: &Path) -> (String, Vec<String>) {
    let target = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        // 第一个空串是窗口标题占位：少了它，`start` 会把带引号的路径当成标题。
        (
            "cmd".to_string(),
            vec!["/C".to_string(), "start".to_string(), String::new(), target],
        )
    } else if cfg!(target_os = "macos") {
        ("open".to_string(), vec![target])
    } else {
        ("xdg-open".to_string(), vec![target])
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::assert_is_empty,
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic
    )]

    use std::path::Path;

    use super::{open_command, reveal_command};

    #[test]
    fn commands_carry_the_path() {
        let path = Path::new("/tmp/a b.txt");

        let (reveal_program, reveal_args) = reveal_command(path);
        assert!(!reveal_program.is_empty());
        assert!(reveal_args.iter().any(|arg| arg.contains("a b.txt")));

        let (open_program, open_args) = open_command(path);
        assert!(!open_program.is_empty());
        assert!(open_args.iter().any(|arg| arg.contains("a b.txt")));
    }
}
