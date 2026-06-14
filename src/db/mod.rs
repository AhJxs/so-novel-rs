//! 持久化层：SQLite 单文件数据库。
//!
//! 当前 schema 只装下载任务记录；后续书源覆写等也走这里。文件路径由
//! `config::ConfigPaths::download_db_file` 决定，跟 config.ini 同目录。
//!
//! 设计原则：
//! - `rusqlite::Connection` 在 `Db` 里直接 owned（eframe 单线程，不需要 Mutex）。
//! - 任务表只有两列（`id` + `data` 全文 JSON），过滤在 Rust 侧做。
//!   几百条任务在内存里 filter 完全够用；想加 SQL 索引时再扩 column。
//! - `schema_version` 表留给将来加表 / 加列时做 migration 用。

use std::path::Path;

use rusqlite::Connection;

/// 当前 schema 版本号。新加表 / 改字段时 bump 这个号，迁移逻辑在 `init_schema`
/// 里按 `current_version` 递增地跑。
const CURRENT_SCHEMA_VERSION: i32 = 1;

pub mod tasks;

pub use tasks::{DownloadTaskRecord, FailureRecord};

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

    /// 内存数据库（`sqlite::memory:`）—— `Db::open` 失败时回退用，不阻塞启动。
    /// 表 schema 跟磁盘版完全一致（init_schema 仍会跑）。
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }
}

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    // schema_version 表只用来标记"跑到第几版"，方便将来加 migration。
    // 当前每张业务表都用 `IF NOT EXISTS` 创建，没有 ALTER，所以 v1 直接写完。
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        );

        CREATE TABLE IF NOT EXISTS download_tasks (
            id   INTEGER PRIMARY KEY,
            data TEXT NOT NULL
        );
        ",
    )?;

    // 写入当前版本（如果还没记录）
    let has: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_version)",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);
    if !has {
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            rusqlite::params![CURRENT_SCHEMA_VERSION],
        )?;
    }

    Ok(())
}
