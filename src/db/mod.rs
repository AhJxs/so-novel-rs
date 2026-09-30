//! 持久化层: 所有 `~/.sonovel/` 下的 JSON 文件读写 (tasks / `sources_config` / rules)。
//!
//! **错误**: 领域级 `RulesError` 保留路径与原因; 顶层 `DaoError` 统一归一, 业务层 `?` 一步透传到
//! `AppError`; task 文件字段简单, 暂用 `anyhow::Result`。
//!
//! **同步 I/O**: 全用 `std::fs` —— dao 函数被 CLI 启动 / web setup / gpui 启动等同步上下文直接
//! 调用; 迁到 `tokio::fs` 是 sync → async 的行为变更, 需全仓 caller 同步改。
//!
//! **[`write_atomically`] 是原子写核心**: 写 tmp → fsync → rename, 断电最坏情况"老文件还在"。

mod rules;
mod sources_config;
mod tasks;

mod tasks_init;
pub use rules::{
    META_AUTHOR, META_BOOK_NAME, META_CATEGORY, META_COVER_URL, META_INTRO, META_LAST_UPDATE_TIME,
    META_LATEST_CHAPTER, META_LATEST_CHAPTER_URL, META_STATUS, RulesError, apply_default_rule,
    init_rules_dir, list_rule_files, load_active_rules, load_rules_from_path,
};
pub use sources_config::SourcesConfig;
use std::path::{Path, PathBuf};
pub use tasks::{load as load_tasks, save as save_tasks, save_with_trim};
pub use tasks_init::load_tasks_from_file;

/// 顶层 dao 错误: 业务层用 `?` 一步透传到 [`crate::error::AppError::Db`]；`RulesError` 仍保留
/// (有路径 + 原因, 不能丢), 通过 `From<RulesError> for DaoError` 归一。
#[derive(Debug, thiserror::Error)]
pub enum DaoError {
    /// IO 错误 (读 / 写 / 文件锁 / 权限)。
    #[error("dao IO 错误 {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// JSON 反序列化失败。
    #[error("dao JSON 错误 {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// 规则加载错误 (来自 `RulesError`)。
    #[error("dao 规则错误: {0}")]
    Rules(#[from] RulesError),
    /// 资源不存在 (文件 / 目录)。
    #[error("dao 资源不存在: {0}")]
    NotFound(PathBuf),
}

impl From<DaoError> for crate::error::AppError {
    fn from(e: DaoError) -> Self {
        Self::db(e.to_string())
    }
}

/// 把 `data` 写到 `path`, 失败时不会留下半截文件: 同目录生成唯一临时名 (避免多实例冲突) →
/// 全量写 + fsync → `rename` 覆盖 → 失败时主动删临时文件。临时文件放同目录是为了让 `rename`
/// 是原子的 (跨目录 / 跨文件系统 rename 不是原子)。
#[tracing::instrument(skip(data), fields(path = %path.display(), bytes = data.len()))]
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

    // rename 覆盖目标。Windows 上 `std::fs::rename` 不允许覆盖已存在的目标, 所以先
    // remove 再 rename。两步不是严格原子, 但配合上面的 fsync, 断电最坏是"老文件还在"。
    if path.exists()
        && let Err(e) = std::fs::remove_file(path)
    {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::io::Read;

    #[test]
    fn write_atomically_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        write_atomically(&path, b"hello").unwrap();
        let mut s = String::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn write_atomically_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        write_atomically(&path, b"v1").unwrap();
        write_atomically(&path, b"v2").unwrap();
        let s = std::fs::read_to_string(&path).unwrap();
        assert_eq!(s, "v2");
    }

    #[test]
    fn write_atomically_cleans_tmp_on_failure() {
        // 写一个不可写的目录触发失败, 验证没有 .tmp.* 残留
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        std::fs::create_dir(&path).unwrap();
        let result = write_atomically(&path, b"x");
        assert!(result.is_err());

        let leftover: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with(".data.json.tmp."))
            })
            .collect();
        assert!(
            leftover.is_empty(),
            "tmp files should be cleaned, found: {leftover:?}"
        );
    }

    #[test]
    fn dao_error_io_includes_path() {
        let e = DaoError::Io {
            path: PathBuf::from("/x/y"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        };
        let s = e.to_string();
        assert!(s.contains("/x/y"));
        assert!(s.contains("missing"));
    }

    #[test]
    fn dao_error_from_rules_error() {
        let r = RulesError::NotFound(PathBuf::from("/rules"));
        let d: DaoError = r.into();
        assert!(matches!(d, DaoError::Rules(_)));
    }

    #[test]
    fn dao_error_to_app_error() {
        let d = DaoError::NotFound(PathBuf::from("/missing"));
        let a: crate::error::AppError = d.into();
        assert!(matches!(a, crate::error::AppError::Db(_)));
    }
}
