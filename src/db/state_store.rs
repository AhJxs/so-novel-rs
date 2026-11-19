//! 运行数据的嵌入式 KV 存储（SurrealKV）。
//!
//! 书架 / 下载任务 / 书源选择状态从三份 JSON 文件收进一个 `SurrealKV` 库（`~/.sonovel/state`）。
//! 每个域是一个整块 JSON blob，一次事务只写一个键，带 `meta:schema_version` 做升级闸门。
//!
//! **单实例**：库在应用启动时打开并保持到退出；第二个实例打不开目录锁，启动即提示
//! 「应用已在运行」。因此写入不需要反复 open/close —— 实测常驻打开后单次 commit
//! （`Durability::Immediate`，fsync）约 0.5ms，可以直接在 UI 线程同步写。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use surrealkv::{Durability, Tree, TreeBuilder};
use tokio::runtime::Runtime;

/// 当前程序认识的运行数据 schema 版本。
pub const STATE_SCHEMA_VERSION: u32 = 1;
/// 键：schema 版本（u32 大端字节）。
pub const KEY_SCHEMA: &[u8] = b"meta:schema_version";
/// 键：书架 JSON blob。
pub const KEY_LIBRARY: &[u8] = b"state:library";
/// 键：下载任务 JSON blob。
pub const KEY_TASKS: &[u8] = b"state:tasks";
/// 键：书源选择 JSON blob。
pub const KEY_SOURCES: &[u8] = b"state:sources";

/// 打开时目录被别的实例占着：调用方应提示「应用已在运行」。
#[derive(Debug, thiserror::Error)]
pub enum StateOpenError {
    /// 目录锁被另一个进程持有（通常是已经开了一个窗口）。
    #[error("应用已在运行，请先关闭另一个窗口")]
    Locked,
    /// 其它打开 / 读取失败。
    #[error("打开运行数据失败: {0}")]
    Other(anyhow::Error),
}

/// 一次读取的全部运行数据（缺失的域为 `None`）。
#[derive(Debug, Default)]
pub struct StateSnapshot {
    /// 磁盘上记录的 schema 版本。
    pub schema_version: Option<u32>,
    /// 书架 JSON。
    pub library: Option<Vec<u8>>,
    /// 下载任务 JSON。
    pub tasks: Option<Vec<u8>>,
    /// 书源选择 JSON。
    pub sources: Option<Vec<u8>>,
    /// 磁盘上的 schema 比本程序新、或库打不开：只读，拒绝写入。
    pub read_only: bool,
    /// 非致命的打开 / 读取错误（打不开库时以空状态启动）。
    pub error: Option<String>,
}

/// 运行数据存储句柄。持有打开到进程退出为止的 `Tree`。
///
/// `tree` 为 `None` 表示库打不开：本次运行按只读空状态启动，写入会失败并记日志。
pub struct StateStore {
    dir: PathBuf,
    tree: Option<Tree>,
    runtime: &'static Runtime,
    read_only: bool,
}

impl StateStore {
    /// 打开（或创建）运行数据库并读出全部运行数据。
    ///
    /// 目录被占用时重试约 1 秒再判定为「另一个实例在跑」：上一个实例正在退出时
    /// 锁会晚几十毫秒才释放，直接报错会误伤正常重启。
    ///
    /// # Errors
    ///
    /// 目录被另一个实例占用返回 [`StateOpenError::Locked`]；其它打不开 / 读不出的情况
    /// **不**是致命错误：以空状态 + 只读启动，错误写进 [`StateSnapshot::error`] 供 UI 提示。
    /// 数据损坏不值得把整个应用挡在门外，用户删掉 `~/.sonovel/state` 就能恢复。
    pub fn open(
        dir: PathBuf,
        runtime: &'static Runtime,
    ) -> Result<(Self, StateSnapshot), StateOpenError> {
        let tree = match runtime.block_on(open_with_retry(&dir)) {
            Ok(tree) => tree,
            // 另一个实例占着目录：单实例语义下这是启动失败。
            Err(StateOpenError::Locked) => return Err(StateOpenError::Locked),
            Err(StateOpenError::Other(error)) => {
                tracing::error!("运行数据打不开，本次运行不会保存: {error:#}");
                let message = format!("运行数据打不开，本次运行不会保存：{error:#}");
                return Ok((
                    Self {
                        dir,
                        tree: None,
                        runtime,
                        read_only: true,
                    },
                    StateSnapshot {
                        read_only: true,
                        error: Some(message),
                        ..StateSnapshot::default()
                    },
                ));
            }
        };
        let mut store = Self {
            dir,
            tree: Some(tree),
            runtime,
            read_only: false,
        };
        let snapshot = store.tree.as_ref().map_or_else(
            || Ok(StateSnapshot::default()),
            |tree| read_snapshot(runtime, tree),
        );
        let snapshot = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::error!("运行数据读取失败，本次运行不会保存: {error:#}");
                let message = format!("运行数据读取失败，本次运行不会保存：{error:#}");
                store.read_only = true;
                return Ok((
                    store,
                    StateSnapshot {
                        read_only: true,
                        error: Some(message),
                        ..StateSnapshot::default()
                    },
                ));
            }
        };
        store.read_only = snapshot.read_only;
        Ok((store, snapshot))
    }

    /// 磁盘上的 schema 比本程序新，所有写入都会被拒绝。
    #[must_use]
    pub const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// 写一个键。事务提交前已 fsync，返回即已落盘。
    ///
    /// # Errors
    ///
    /// schema 版本过新、事务失败时返回错误。
    pub fn put(&self, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        if self.read_only {
            anyhow::bail!(
                "运行数据由更新版本的程序写入（schema 高于 {STATE_SCHEMA_VERSION}），本程序只读"
            );
        }
        let Some(tree) = self.tree.as_ref() else {
            anyhow::bail!("运行数据存储不可用，本次运行不会保存");
        };
        self.runtime.block_on(async {
            let mut tx = tree.begin()?;
            match tx.get(KEY_SCHEMA)? {
                Some(bytes) => {
                    let version = decode_schema(&bytes);
                    if version > STATE_SCHEMA_VERSION {
                        anyhow::bail!(
                            "运行数据 schema 版本 {version} 高于当前程序支持的 {STATE_SCHEMA_VERSION}"
                        );
                    }
                }
                None => tx.set(KEY_SCHEMA, STATE_SCHEMA_VERSION.to_be_bytes().to_vec())?,
            }
            tx.set(key, value.to_vec())?;
            tx.set_durability(Durability::Immediate);
            tx.commit().await?;
            drop(tx);
            Ok(())
        })
    }

    /// 把无法解析的 blob 原文另存一份，返回备份路径。
    ///
    /// 文件版 `library.json` 是改名备份，KV 里没有「文件」可改名，只能把值落成文件。
    #[must_use]
    pub fn backup_corrupt(&self, name: &str, bytes: &[u8]) -> Option<PathBuf> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        std::fs::create_dir_all(&self.dir).ok()?;
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = self.dir.join(format!(
            "{name}.corrupt-{}-{seq}.json",
            crate::library::now_unix()
        ));
        std::fs::write(&path, bytes).ok()?;
        Some(path)
    }

    /// 优雅关闭（写 WAL 收尾标记）。退出 / 测试里显式调用；进程直接结束也不丢已提交数据。
    pub fn close(&self) {
        let Some(tree) = self.tree.as_ref() else {
            return;
        };
        if let Err(error) = self.runtime.block_on(tree.close()) {
            tracing::warn!("关闭运行数据存储失败: {error}");
        }
    }
}

/// 读出 schema 与三个域的 blob。
fn read_snapshot(runtime: &Runtime, tree: &Tree) -> anyhow::Result<StateSnapshot> {
    runtime.block_on(async {
        let tx = tree.begin()?;
        let schema_version = tx.get(KEY_SCHEMA)?.map(|value| decode_schema(&value));
        let library = tx.get(KEY_LIBRARY)?;
        let tasks = tx.get(KEY_TASKS)?;
        let sources = tx.get(KEY_SOURCES)?;
        drop(tx);
        let read_only = schema_version.is_some_and(|version| version > STATE_SCHEMA_VERSION);
        if read_only {
            tracing::warn!(
                "运行数据 schema {:?} 高于当前程序支持的 {STATE_SCHEMA_VERSION}，本次只读",
                schema_version
            );
        }
        Ok(StateSnapshot {
            schema_version,
            library,
            tasks,
            sources,
            read_only,
            error: None,
        })
    })
}

/// schema 值按 4 字节大端读；长度不对时当成 0（未知），后续写入会拒绝而不是覆盖。
fn decode_schema(value: &[u8]) -> u32 {
    value
        .get(..4)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, u32::from_be_bytes)
}

async fn open_with_retry(dir: &Path) -> Result<Tree, StateOpenError> {
    std::fs::create_dir_all(dir).map_err(|error| StateOpenError::Other(error.into()))?;
    let dir = dir.to_path_buf();
    let mut last = None;
    for _ in 0..OPEN_RETRY_ATTEMPTS {
        match TreeBuilder::new().with_path(dir.clone()).build() {
            Ok(tree) => return Ok(tree),
            Err(error) if is_lock_error(&error) => {
                last = Some(error);
                tokio::time::sleep(OPEN_RETRY_DELAY).await;
            }
            Err(error) => return Err(StateOpenError::Other(error.into())),
        }
    }
    tracing::warn!(
        "运行数据目录被占用（最后一次: {}）",
        last.map(|error| error.to_string()).unwrap_or_default()
    );
    Err(StateOpenError::Locked)
}

/// 打开时的锁冲突重试次数 × 间隔 ≈ 1 秒。
const OPEN_RETRY_ATTEMPTS: u32 = 20;
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(50);

/// 是否是「目录正被另一个实例占着」的锁冲突。
///
/// 平台差异实测：Unix 上 `SurrealKV` 返回 `Error::Other("... already locked ...")`；
/// Windows 上 `flock` 不可用，`LockFileEx` 的 `ERROR_LOCK_VIOLATION(33)` 被包成
/// `Error::Io`，`kind()` 是 `Uncategorized` 而不是 `WouldBlock`，只能按原始错误码认。
fn is_lock_error(error: &surrealkv::Error) -> bool {
    match error {
        surrealkv::Error::Other(message) => message.contains("already locked"),
        surrealkv::Error::Io(io) => {
            io.raw_os_error() == Some(33) || io.kind() == std::io::ErrorKind::WouldBlock
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use surrealkv::TreeBuilder;

    use super::*;

    fn runtime() -> &'static Runtime {
        Box::leak(Box::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap(),
        ))
    }

    fn open(dir: &Path) -> (StateStore, StateSnapshot) {
        StateStore::open(dir.to_path_buf(), runtime()).unwrap()
    }

    #[test]
    fn empty_dir_opens_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let (store, snapshot) = open(dir.path());
        assert!(snapshot.library.is_none());
        assert!(snapshot.tasks.is_none());
        assert!(snapshot.sources.is_none());
        assert!(snapshot.schema_version.is_none());
        assert!(!snapshot.read_only);

        store.put(KEY_LIBRARY, b"{\"books\":[]}").unwrap();
        store.close();

        let (_store, snapshot) = open(dir.path());
        assert_eq!(snapshot.library.as_deref(), Some(&b"{\"books\":[]}"[..]));
        assert_eq!(snapshot.schema_version, Some(STATE_SCHEMA_VERSION));
    }

    #[test]
    fn first_put_writes_schema() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(dir.path());
        store.put(KEY_SOURCES, b"{}").unwrap();
        store.close();

        let (_store, snapshot) = open(dir.path());
        assert_eq!(snapshot.schema_version, Some(STATE_SCHEMA_VERSION));
    }

    #[test]
    fn future_schema_is_read_only() {
        let dir = tempfile::tempdir().unwrap();
        {
            let path = dir.path().to_path_buf();
            runtime().block_on(async {
                let tree = TreeBuilder::new().with_path(path).build().unwrap();
                let mut tx = tree.begin().unwrap();
                tx.set(KEY_SCHEMA, 999_u32.to_be_bytes().to_vec()).unwrap();
                tx.set(KEY_LIBRARY, b"old".to_vec()).unwrap();
                tx.commit().await.unwrap();
                drop(tx);
                tree.close().await.unwrap();
                drop(tree);
            });
        }

        let (store, snapshot) = open(dir.path());
        assert!(snapshot.read_only);
        assert_eq!(snapshot.schema_version, Some(999));
        assert!(store.is_read_only());
        assert!(store.put(KEY_LIBRARY, b"new").is_err());
        store.close();

        let (_store, snapshot) = open(dir.path());
        assert_eq!(
            snapshot.library.as_deref(),
            Some(&b"old"[..]),
            "只读时不能覆盖别人的数据"
        );
    }

    #[test]
    fn second_instance_is_locked_out() {
        let dir = tempfile::tempdir().unwrap();
        let (first, _) = open(dir.path());

        let second = StateStore::open(dir.path().to_path_buf(), runtime());
        assert!(
            matches!(second, Err(StateOpenError::Locked)),
            "单实例：第二个实例必须打不开"
        );

        first.close();
        let (third, _) = open(dir.path());
        third.close();
    }

    #[test]
    fn corrupt_blob_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(dir.path());
        let path = store.backup_corrupt("library", b"not json").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read(&path).unwrap(), b"not json");
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("library.corrupt-")
        );
        store.close();
    }
}
