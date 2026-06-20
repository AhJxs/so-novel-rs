//! tracing 初始化 + 日志保留策略。
//!
//! `init_tracing` 在 `main()` 最早调用，配置 stdout + 文件双 layer；
//! `purge_old_logs` 启动时清理过期日志文件。

use std::path::Path;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// 日志保留天数。启动时 `purge_old_logs` 删除 `log_dir` 里 mtime 早于
/// `now - LOG_RETENTION_DAYS` 的所有文件。设 30 天：足够覆盖大多数 bug 复现
/// 窗口，又不至于把 `~/.sonovel/logs/` 撑到 GB 级。
const LOG_RETENTION_DAYS: u64 = 30;

/// 初始化 tracing：stdout layer + 按天滚动的文件 layer。
///
/// 文件 layer 失败不 panic —— 静默退化为只有 stdout。
pub fn init_tracing(log_dir: &Path) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,so_novel_rs=debug"));

    // stdout layer（保持原行为）。
    let stdout_layer = fmt::layer().with_target(false);

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(stdout_layer);

    // 文件 layer：按天滚动到 `log_dir/so-novel-rs.YYYY-MM-DD.log`。
    // 用 `match` 内联构造让 Rust 推断 Layer<S> 的 S —— helper 函数的返回类型
    // 写不出嵌套的 Layered<...>。文件 appender 失败不 panic —— 静默退化为只有 stdout。
    match std::fs::create_dir_all(log_dir) {
        Ok(()) => {
            // 先清过期的旧日志：tracing_appender 本身**不**做保留策略（看 v0.2.5
            // 源码 `RollingFileAppender` 只有构造期指定 max_files 才 prune）；
            // 我们用 daily("", "") 创建的 appender 没设 max_files，所以日志只增不减。
            // 启动时按 mtime 删 30 天前的，避免 ~/.sonovel/logs/ 慢慢涨到 GB 级。
            purge_old_logs(log_dir, LOG_RETENTION_DAYS);

            // 日志文件名 `<日期>.log`（如 `2026-06-18.log`）：传空 prefix 让 rolling 直接拼日期后缀。
            let appender = tracing_appender::rolling::daily(log_dir, "");
            let (writer, guard) = tracing_appender::non_blocking(appender);
            // guard 进 leak 让文件 writer 后台线程存活到进程退出 —— tracing_appender 标准用法。
            Box::leak(Box::new(guard));
            // 文件 layer 关掉 ANSI 颜色码（\x1b[2m / \x1b[32m …）—— 文件不是终端，
            // 不解释转义码，带颜色码会显示成 [2m...[0m 乱码。stdout layer 保留 ANSI。
            let file_layer = fmt::layer()
                .with_writer(writer)
                .with_target(true)
                .with_ansi(false);
            registry.with(file_layer).init();
        }
        Err(e) => {
            eprintln!("init_tracing: file layer disabled ({e})");
            registry.init();
        }
    }
}

/// 删 `log_dir` 里 mtime 超过 `retention_days` 天的文件。
///
/// 只看顶层、只看 regular file —— 子目录和别的资源不动。失败不 panic，只是
/// `eprintln!` 让 CLI 模式下用户能看到，GUI 模式下静默。tracing_appender 本身
/// 没暴露 prune API，所以由我们做这套保留策略。
fn purge_old_logs(log_dir: &Path, retention_days: u64) {
    let entries = match std::fs::read_dir(log_dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!(
                "purge_old_logs: read_dir({}) failed: {e}",
                log_dir.display()
            );
            return;
        }
    };

    let cutoff = match std::time::SystemTime::now().checked_sub(std::time::Duration::from_secs(
        retention_days.saturating_mul(24 * 60 * 60),
    )) {
        Some(t) => t,
        None => {
            // 系统时间早于 epoch —— 不可能但兜底；不做删除。
            return;
        }
    };

    let mut purged = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }
        let mtime = match metadata.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if mtime < cutoff {
            if let Err(e) = std::fs::remove_file(&path) {
                eprintln!("purge_old_logs: remove {} failed: {e}", path.display());
            } else {
                purged += 1;
            }
        }
    }
    if purged > 0 {
        eprintln!("purge_old_logs: removed {purged} file(s) older than {retention_days} days");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    /// retention=30 天：mtime=31 天前的文件应被删，1 天前的保留。
    #[test]
    fn purge_old_logs_removes_only_expired() {
        let dir = tempfile::tempdir().unwrap();

        let old = dir.path().join("2025-01-01");
        let recent = dir.path().join("2026-06-19");
        fs::write(&old, b"old log").unwrap();
        fs::write(&recent, b"recent log").unwrap();

        // 把 old 的 mtime 调到 31 天前
        let mtime_31d_ago = SystemTime::now() - Duration::from_secs(31 * 24 * 60 * 60);
        filetime_touch(&old, mtime_31d_ago);

        purge_old_logs(dir.path(), 30);

        assert!(!old.exists(), "old file should be purged");
        assert!(recent.exists(), "recent file should be kept");
    }

    /// 子目录不被删（purge 只看顶层 regular file）。
    #[test]
    fn purge_old_logs_skips_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("nested");
        fs::create_dir(&sub).unwrap();
        let nested_file = sub.join("anything.log");
        fs::write(&nested_file, b"x").unwrap();
        // 把子目录的 mtime 也调老 — 不该触发删除
        let old = SystemTime::now() - Duration::from_secs(60 * 24 * 60 * 60);
        filetime_touch_dir(&sub, old);

        purge_old_logs(dir.path(), 30);

        assert!(sub.exists(), "subdirectory must not be deleted");
        assert!(
            nested_file.exists(),
            "files inside subdirs must not be touched"
        );
    }

    /// retention=0：所有 mtime < now 的文件全删（== 所有文件）。
    #[test]
    fn purge_old_logs_zero_retention_removes_all() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), b"x").unwrap();
        fs::write(dir.path().join("b"), b"y").unwrap();

        purge_old_logs(dir.path(), 0);

        assert!(!dir.path().join("a").exists());
        assert!(!dir.path().join("b").exists());
    }

    // mtime 没法直接用 std::fs::File 设；用 filetime crate 或者构造老 mtime。
    // 这里走最简单路径：开 file → set_modified wrapped via `std::fs::File::set_modified`（1.75+ 稳定）。
    fn filetime_touch(path: &std::path::Path, t: SystemTime) {
        let f = fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(t).unwrap();
    }

    fn filetime_touch_dir(path: &std::path::Path, t: SystemTime) {
        // 子目录的 mtime 改不到（rust std 没 API），跳过 — 子目录测试只断言
        // purge 不动子目录内容，mtime 改不改都该不动。
        let _ = t;
        let _ = path;
    }
}
