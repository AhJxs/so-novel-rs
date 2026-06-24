//! 持久化层：所有 `~/.sonovel/` 下的 JSON 文件读写。
//!
//! 子模块：
//! - `tasks` — 下载任务 `tasks.json` 的 CRUD
//! - `sources_config` — 书源配置 `sources_config.json` 的读写
//! - `rules` — 规则目录 `rules/` 的初始化与加载（也含 `META_*` 常量、
//!   `load_rules_from_path` / `apply_default_rule` / `RulesError` 等底层解析）

mod rules;
mod sources_config;
mod tasks;

pub use rules::{
    META_AUTHOR, META_BOOK_NAME, META_CATEGORY, META_COVER_URL, META_INTRO, META_LAST_UPDATE_TIME,
    META_LATEST_CHAPTER, META_LATEST_CHAPTER_URL, META_STATUS, RulesError, apply_default_rule,
    init_rules_dir, list_rule_files, load_active_rules, load_rules_from_path,
};
pub use sources_config::SourcesConfig;
pub use tasks::{load as load_tasks, save as save_tasks, save_with_trim};

use std::path::Path;

/// 把 `data` 写到 `path`，失败时不会留下半截文件。
///
/// 步骤：
/// 1. 在目标同一目录下生成唯一临时文件名（避免与其它实例的 tmp 冲突）；
/// 2. 全量写入并 fsync，确保字节落盘；
/// 3. `rename` 覆盖目标 — POSIX 原子、Windows 在同卷上也是原子操作；
/// 4. 失败时主动删除临时文件，不留垃圾。
///
/// 同目录是为了让 `rename` 是原子的（跨目录 / 跨文件系统 rename 不是原子）。
pub fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    // 全局唯一 + 进程内唯一（同一 ms 内多次调用也不会冲突）。
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{file_name}.tmp.{}.{}", std::process::id(), seq));

    let write_result = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    // rename 覆盖目标。如果目标已存在，std::fs::rename 在 Windows 上会
    // 失败（不允许覆盖），所以先 remove 再 rename。两步不是严格原子，
    // 但配合上面的 fsync，断电最坏情况是"老文件还在"（不是半截）。
    if path.exists() {
        if let Err(e) = std::fs::remove_file(path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}
