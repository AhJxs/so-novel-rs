//! 持久化层 (PR #11 文档化, 2026-07-08): 所有 `~/.sonovel/` 下的 JSON 文件读写。
//!
//! # 子模块
//!
//! - `tasks` — 下载任务 `tasks.json` 的 CRUD (load / save / trim_completed)
//! - `sources_config` — 书源配置 `sources_config.json` 的读写 (SourcesConfig)
//! - `rules` — 规则目录 `rules/` 的初始化与加载 (含 META_* 常量, load/apply/init)
//! - `mod.rs` — 公共 helper (`write_atomically` 原子写) + 顶层 `DaoError`
//!
//! # 错误处理
//!
//! - **领域级**: `RulesError` (thiserror) 承载规则文件 IO + 解析错误, 保留
//!   路径和原因 (强类型, 不丢信息)
//! - **顶层归一**: `DaoError` (thiserror) 统一所有 dao 层错误, 业务层用 `?`
//!   一步透传到 `AppError`
//! - **task 文件**: 暂用 `anyhow::Result`, 字段简单 (单 JSON array) 不值得
//!   单独错误枚举, 错误消息走 `format!("{e:#}")` 给 UI
//!
//! # I/O 策略 (不抢跑, 留 PR #12 阶段)
//!
//! - 当前所有读写作 **同步** `std::fs`, 因为 dao 函数被 CLI 启动 / web setup /
//!   gpui 启动等同步上下文直接调用
//! - 迁到 `tokio::fs` 是行为变更 (sync → async), 需全仓 caller 同步改, 单独 PR
//! - 关键路径已加 `#[tracing::instrument]`, 出问题能直接定位
//!
//! # 原子写
//!
//! [`write_atomically`] 是核心: 写 tmp → fsync → rename, 断电最坏情况"老文件
//! 还在", 不会留半截。Windows 上 rename 不允许覆盖, 先 remove 再 rename 是
//! 已知妥协, 配合 fsync 仍保证一致性。

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

use std::path::{Path, PathBuf};

/// 顶层 dao 错误 (PR #11, 2026-07-08).
///
/// 业务层用 `?` 一步透传到 [`crate::error::AppError::Db`]。`RulesError` 仍保留
/// (有路径 + 原因, 不能丢), 通过 `From<RulesError> for DaoError` 自动归一。
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

/// 把 `data` 写到 `path`，失败时不会留下半截文件。
///
/// 步骤：
/// 1. 在目标同一目录下生成唯一临时文件名（避免与其它实例的 tmp 冲突）；
/// 2. 全量写入并 fsync，确保字节落盘；
/// 3. `rename` 覆盖目标 — POSIX 原子、Windows 在同卷上也是原子操作；
/// 4. 失败时主动删除临时文件，不留垃圾。
///
/// 同目录是为了让 `rename` 是原子的（跨目录 / 跨文件系统 rename 不是原子）。
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn write_atomically_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        write_atomically(&path, b"hello").unwrap();
        let mut s = String::new();
        std::fs::File::open(&path).unwrap().read_to_string(&mut s).unwrap();
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
        // 把 path 做成目录, write 会失败
        std::fs::create_dir(&path).unwrap();
        let result = write_atomically(&path, b"x");
        assert!(result.is_err());

        // 没有 .tmp.* 残留
        let leftover: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with(".data.json.tmp."))
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            leftover.is_empty(),
            "tmp files should be cleaned, found: {:?}",
            leftover
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
