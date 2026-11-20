//! 运行数据的嵌入式 KV 存储（SurrealKV）。
//!
//! 书架 / 下载任务 / 书源选择状态从三份 JSON 文件收进一个 `SurrealKV` 库（`~/.sonovel/state`）。
//! 每个域是一个整块 JSON blob，一次事务只写一个键，带 `meta:schema_version` 做升级闸门。
//!
//! **单实例**：库在应用启动时打开并保持到退出；第二个实例打不开目录锁，启动即提示
//! 「应用已在运行」。因此写入不需要反复 open/close —— 实测常驻打开后单次 commit
//! （`Durability::Immediate`，fsync）约 0.5ms，可以直接在 UI 线程同步写。

use std::path::{Path, PathBuf};
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

/// `read_only` 的成因：决定 [`StateStore::put`] 拒绝写入时给用户看的文案。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyReason {
    /// 磁盘 schema 高于本程序支持：正常只读，不是损坏。
    FutureSchema,
    /// schema 标记损坏（长度不是 4 字节）。
    CorruptSchema,
    /// 库打不开 / 读不出：本次运行完全不写。
    Unavailable,
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
    /// `read_only` 的成因；非只读时为 `None`。
    pub(crate) read_only_reason: Option<ReadOnlyReason>,
}

/// 运行数据存储句柄。持有打开到进程退出为止的 `Tree`。
///
/// `tree` 为 `None` 表示库打不开：本次运行按只读空状态启动，写入会失败并记日志。
pub struct StateStore {
    dir: PathBuf,
    tree: Option<Tree>,
    runtime: &'static Runtime,
    read_only: bool,
    /// 只读的原因：写入被拒时据此区分「版本过新」与「schema 损坏」。
    read_only_reason: Option<ReadOnlyReason>,
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
                        read_only_reason: Some(ReadOnlyReason::Unavailable),
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
            read_only_reason: None,
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
                store.read_only_reason = Some(ReadOnlyReason::Unavailable);
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
        store.read_only_reason = snapshot.read_only_reason;
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
    /// 只读（schema 过新 / schema 标记损坏 / 库不可用）、事务失败时返回错误。
    pub fn put(&self, key: &[u8], value: &[u8]) -> anyhow::Result<()> {
        if self.read_only {
            let message = match self.read_only_reason {
                Some(ReadOnlyReason::CorruptSchema) => {
                    "运行数据 schema 标记损坏（长度不是 4 字节），本程序只读，不会覆盖".to_string()
                }
                Some(ReadOnlyReason::Unavailable) => {
                    "运行数据存储不可用，本次运行不会保存".to_string()
                }
                Some(ReadOnlyReason::FutureSchema) | None => format!(
                    "运行数据由更新版本的程序写入（schema 高于 {STATE_SCHEMA_VERSION}），本程序只读"
                ),
            };
            anyhow::bail!("{message}");
        }
        let Some(tree) = self.tree.as_ref() else {
            anyhow::bail!("运行数据存储不可用，本次运行不会保存");
        };
        self.runtime.block_on(async {
            let mut tx = tree.begin()?;
            match tx.get(KEY_SCHEMA)? {
                Some(bytes) => {
                    let Some(version) = decode_schema(&bytes) else {
                        anyhow::bail!(
                            "运行数据 schema 值损坏（{} 字节，应为 4 字节），拒绝覆盖写入",
                            bytes.len()
                        );
                    };
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
    /// 备份名带时间戳；同秒内再次损坏（含崩溃后立刻重启）会追加 `-1`、`-2`… 后缀，
    /// 绝不覆盖上一份 —— 那可能是用户数据仅存的副本。
    #[must_use]
    pub fn backup_corrupt(&self, name: &str, bytes: &[u8]) -> Option<PathBuf> {
        std::fs::create_dir_all(&self.dir).ok()?;
        let secs = u64::try_from(crate::library::now_unix()).unwrap_or(0);
        let path = unique_backup_path(&self.dir, name, secs);
        std::fs::write(&path, bytes).ok()?;
        Some(path)
    }

    /// 优雅关闭（写 WAL 收尾标记）。退出 / 测试里显式调用；进程直接结束也不丢已提交数据。
    ///
    /// **幂等**：`SurrealKV` 的 `Tree::close` 内部有 `is_closed` 标记（第二次调用直接返回
    /// `Ok`），`Tree::drop` 也会先查该标记。因此显式关闭后实体析构不会重复关库。
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
        let schema = tx.get(KEY_SCHEMA)?;
        let library = tx.get(KEY_LIBRARY)?;
        let tasks = tx.get(KEY_TASKS)?;
        let sources = tx.get(KEY_SOURCES)?;
        drop(tx);
        let schema_version = schema.as_deref().and_then(decode_schema);
        // 键存在但长度不是 4 字节：不认识的 schema 标记，宁可只读也绝不按 0 覆盖。
        let schema_corrupt = schema.is_some() && schema_version.is_none();
        if schema_corrupt {
            tracing::warn!(
                "运行数据 schema 值损坏（{} 字节，应为 4 字节），本次只读",
                schema.as_ref().map_or(0, Vec::len)
            );
        }
        let future_version = schema_version.is_some_and(|version| version > STATE_SCHEMA_VERSION);
        if future_version {
            tracing::warn!(
                "运行数据 schema {:?} 高于当前程序支持的 {STATE_SCHEMA_VERSION}，本次只读",
                schema_version
            );
        }
        let read_only_reason = if schema_corrupt {
            Some(ReadOnlyReason::CorruptSchema)
        } else if future_version {
            Some(ReadOnlyReason::FutureSchema)
        } else {
            None
        };
        Ok(StateSnapshot {
            schema_version,
            library,
            tasks,
            sources,
            read_only: read_only_reason.is_some(),
            error: schema_corrupt
                .then(|| "运行数据 schema 标记损坏，本次运行按只读处理，不会覆盖".to_string()),
            read_only_reason,
        })
    })
}

/// schema 值按 4 字节大端读（与 `put` 的 `to_be_bytes` 对应）。长度不是 4 字节视为
/// 损坏（`None`），后续写入会拒绝而不是覆盖。
fn decode_schema(value: &[u8]) -> Option<u32> {
    value.try_into().ok().map(u32::from_be_bytes)
}

/// 损坏 blob 的备份路径：`<stem>.corrupt-<secs>.json`，已被占用就追加 `-1`、`-2`…
///
/// 同一秒内两次损坏（崩溃后立刻重启再损坏）不能覆盖上一份备份 —— 那可能是
/// 用户数据仅存的副本。时间戳始终保留在文件名里，便于用户辨认。
fn unique_backup_path(dir: &Path, stem: &str, secs: u64) -> PathBuf {
    let base = format!("{stem}.corrupt-{secs}");
    for suffix in 0..1000 {
        let name = if suffix == 0 {
            format!("{base}.json")
        } else {
            format!("{base}-{suffix}.json")
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    // 1000 个候选都被占用（现实中不会发生）：换一个名字，绝不覆盖已有备份。
    dir.join(format!("{base}-{secs}.json"))
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
    fn close_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(dir.path());
        store.put(KEY_LIBRARY, b"{\"books\":[]}").unwrap();
        // 退出路径会显式 close，随后实体析构时 `Tree::drop` 再检查一次：
        // 第二次 close 必须是空操作，不能报错或把已落盘数据弄丢。
        store.close();
        assert!(
            store.put(KEY_LIBRARY, b"x").is_err(),
            "关闭后的写入必须干净失败，而不是 panic 或静默成功"
        );
        store.close();
        drop(store);

        let (_store, snapshot) = open(dir.path());
        assert_eq!(
            snapshot.library.as_deref(),
            Some(&b"{\"books\":[]}"[..]),
            "重复 close 不应破坏已提交的数据"
        );
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
        let error = store.put(KEY_LIBRARY, b"new").unwrap_err();
        assert!(
            error.to_string().contains("更新版本"),
            "版本过新的错误文案应点明原因，实际: {error}"
        );
        store.close();

        let (_store, snapshot) = open(dir.path());
        assert_eq!(
            snapshot.library.as_deref(),
            Some(&b"old"[..]),
            "只读时不能覆盖别人的数据"
        );
    }

    /// 直接往库里写一组原始字节（绕开 `put` 的 schema 校验），模拟历史 / 损坏数据。
    fn seed_raw(dir: &Path, schema: Option<&[u8]>, library: &[u8]) {
        let path = dir.to_path_buf();
        runtime().block_on(async {
            let tree = TreeBuilder::new().with_path(path).build().unwrap();
            let mut tx = tree.begin().unwrap();
            if let Some(schema) = schema {
                tx.set(KEY_SCHEMA, schema.to_vec()).unwrap();
            }
            tx.set(KEY_LIBRARY, library.to_vec()).unwrap();
            tx.commit().await.unwrap();
            drop(tx);
            tree.close().await.unwrap();
            drop(tree);
        });
    }

    #[test]
    fn malformed_schema_is_read_only_and_never_overwritten() {
        // 3 字节（截断）与 8 字节（多写）都应视为损坏，而不是当成版本 0 继续覆盖。
        for schema in [&[0_u8, 0, 1][..], &[0_u8; 8][..]] {
            let dir = tempfile::tempdir().unwrap();
            seed_raw(dir.path(), Some(schema), b"old");

            let (store, snapshot) = open(dir.path());
            assert!(
                snapshot.read_only,
                "schema {} 字节：长度不为 4 应只读",
                schema.len()
            );
            assert!(
                snapshot.schema_version.is_none(),
                "损坏的 schema 不给出伪造版本"
            );
            assert!(snapshot.error.is_some(), "损坏时应给用户明确提示");
            assert!(store.is_read_only());
            let error = store.put(KEY_LIBRARY, b"new").unwrap_err();
            assert!(
                error.to_string().contains("损坏"),
                "损坏 schema 的错误文案不能误报成「版本过新」，实际: {error}"
            );
            store.close();

            let (_store, snapshot) = open(dir.path());
            assert_eq!(
                snapshot.library.as_deref(),
                Some(&b"old"[..]),
                "schema 损坏时绝不能覆盖原数据"
            );
            assert!(snapshot.read_only);
        }
    }

    #[test]
    fn four_byte_schema_is_writable() {
        let dir = tempfile::tempdir().unwrap();
        seed_raw(
            dir.path(),
            Some(&STATE_SCHEMA_VERSION.to_be_bytes()),
            b"old",
        );

        let (store, snapshot) = open(dir.path());
        assert!(!snapshot.read_only, "4 字节 schema 是正常数据");
        assert_eq!(snapshot.schema_version, Some(STATE_SCHEMA_VERSION));
        assert!(snapshot.error.is_none());
        store.put(KEY_LIBRARY, b"new").unwrap();
        store.close();

        let (_store, snapshot) = open(dir.path());
        assert_eq!(snapshot.library.as_deref(), Some(&b"new"[..]));
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

    /// 同一秒内两次备份（含崩溃后重启）必须落到不同文件，且第一次的原文不被覆盖。
    #[test]
    fn unique_backup_path_never_reuses_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let secs = 1_700_000_000_u64;

        let first = unique_backup_path(dir.path(), "library", secs);
        assert!(
            first
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!("library.corrupt-{secs}"))
        );
        std::fs::write(&first, b"first").unwrap();

        let second = unique_backup_path(dir.path(), "library", secs);
        assert_ne!(first, second, "同名候选被占用时必须换一个路径");
        assert!(
            second
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&format!("library.corrupt-{secs}")),
            "时间戳要保留在备份名里"
        );
        assert_eq!(
            std::fs::read(&first).unwrap(),
            b"first",
            "上一份备份不能被新备份覆盖"
        );
    }

    #[test]
    fn backup_corrupt_never_overwrites_previous_backup() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(dir.path());
        let first = store.backup_corrupt("tasks", b"first").unwrap();
        let second = store.backup_corrupt("tasks", b"second").unwrap();

        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"first");
        assert_eq!(std::fs::read(&second).unwrap(), b"second");
        store.close();
    }
}
