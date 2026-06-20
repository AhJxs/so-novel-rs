//! 持久化层：SQLite 单文件数据库。
//!
//! 文件路径由 `config::ConfigPaths::db_file` 决定（项目根目录的 `sonovel.db`）。
//!
//! 当前装的内容：
//! - 下载任务记录（`download_tasks`，详见 `tasks.rs`）；
//! - 书源规则（`sources`，详见 `sources.rs`）—— 规则首次启动从 main.json seed；
//! - 用户对书源的启用/禁用覆写（`source_overrides`）。
//!
//! 设计原则：
//! - `rusqlite::Connection` 在 `Db` 里直接 owned（eframe 单线程，不需要 Mutex）；
//! - 业务表的 `data` 列存 JSON 字符串，过滤在 Rust 侧做 — 几百条规模够用，
//!   想加 SQL 索引时再扩 column；
//! - `schema_version` 表给将来 migration 用。

use std::path::Path;

use rusqlite::Connection;

/// 当前 schema 版本号。新加表 / 改字段时 bump，迁移逻辑在 `init_schema` 里递增地跑。
const CURRENT_SCHEMA_VERSION: i32 = 1;

pub mod sources;
pub mod tasks;

pub use tasks::{DownloadTaskRecord, FailureRecord, FinishedReason};

/// 包装一个 `rusqlite::Connection`，负责打开 + 初始化 schema + 提供表级 API。
pub struct Db {
    conn: Connection,
}

impl Db {
    /// 打开（或创建）数据库文件，跑 schema 初始化。
    ///
    /// 父目录不存在时自动创建。schema 已存在时 `init_schema` 是幂等的（`IF NOT EXISTS`）。
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        if let Some(parent) = path.parent() {
            // 父目录创建失败时静默 — sqlite 自己也会尝试，但早点建好能拿到更明确的错误。
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 可变借引；事务（`Connection::transaction`）要求 `&mut Connection`。
    /// 因为 GPUI / eframe 都是单线程，且 `Db` 在 `AppModel` 里以 owned 形式持有，
    /// 调用方拿 `&mut self.db` 就能拿到 `&mut Connection`。
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// 内存数据库（`sqlite::memory:`）—— `Db::open` 失败时回退用，不阻塞启动。
    /// 表 schema 跟磁盘版完全一致（init_schema 仍会跑）。
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }
}

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        );

        CREATE TABLE IF NOT EXISTS download_tasks (
            id   INTEGER PRIMARY KEY,
            data TEXT NOT NULL
        );

        -- 书源规则。`data` 是 JSON 序列化的 Rule（字段对应 main.json 里一条 entry）。
        -- ord 列保留稳定的展示顺序（首次 seed 时按文件顺序写）。
        CREATE TABLE IF NOT EXISTS sources (
            id   INTEGER PRIMARY KEY,
            ord  INTEGER NOT NULL,
            data TEXT NOT NULL
        );

        -- 用户对书源的启用/禁用覆写。
        -- 只记 disabled=true 的行；启用就直接删行。比一行一 bool 更省。
        CREATE TABLE IF NOT EXISTS source_overrides (
            source_id INTEGER PRIMARY KEY
        );
        ",
    )?;

    let has: bool = conn
        .query_row("SELECT EXISTS(SELECT 1 FROM schema_version)", [], |row| {
            row.get(0)
        })
        .unwrap_or(false);
    if !has {
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            rusqlite::params![CURRENT_SCHEMA_VERSION],
        )?;
    }

    Ok(())
}
