//! 下载任务表的仓库：list / upsert / delete_finished。
//!
//! 表 schema 只有两列 — 整条 `DownloadTaskRecord` 序列化成 JSON 存 `data`。
//! 业务字段（started_at_unix、finished_at_unix）也在 JSON 里，所以加字段不改 schema。

use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::models::{Book, SearchResult};

/// 任务结束原因 —— 替代原先用字符串字面量做语义 tag 的反模式。
///
/// 定义在持久化层（`db::tasks`）：JSON schema 跟着这里走，UI / 业务层复用。
///
/// `is_cancelled` / `is_failed` 直接 match enum，不再依赖字符串等值检测。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FinishedReason {
    /// 用户在 UI 点了"取消"，后台响应后通知。
    UserCancelled,
    /// 应用重启 / 进程被杀时正在跑的任务，重新打开 app 时识别为此状态。
    AppRestarted,
    /// 真正失败（网络 / parser / 后台异常）。消息展示给用户，UI 走 i18n。
    Failed { message: String },
}

impl FinishedReason {
    /// 给 UI 用的错误消息文案（仅 `Failed` 有内容）。
    pub fn user_message(&self) -> Option<&str> {
        match self {
            FinishedReason::Failed { message } => Some(message.as_str()),
            FinishedReason::UserCancelled | FinishedReason::AppRestarted => None,
        }
    }

    /// 用户取消 或 应用重启中断（"非真正的失败"）。
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self,
            FinishedReason::UserCancelled | FinishedReason::AppRestarted
        )
    }

    /// db 兼容：把旧 `String` 错误（`"用户已取消"` / `"应用重启时中断"` /
    /// `"后台任务异常退出（通道已断开）"` 等）映射成 enum。
    /// 新代码不应再调它 —— 派发方直接 `FinishedReason::UserCancelled` 等。
    pub fn from_legacy_reason(s: &str) -> Self {
        if s == "用户已取消" {
            FinishedReason::UserCancelled
        } else if s == "应用重启时中断" {
            FinishedReason::AppRestarted
        } else {
            // 旧版本的"后台异常退出"及其他任意失败消息都归类为 Failed。
            FinishedReason::Failed { message: s.to_string() }
        }
    }
}

/// 任务表持久化形态。所有字段对应 `DownloadTask` 里需要落盘的部分；
/// 运行时字段（rx / cancel / started_at Instant）不存。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadTaskRecord {
    pub id: u64,
    pub origin: SearchResult,
    /// 任务开始 unix 时间戳（秒）。比 `Instant` 多一个语义：可序列化 / 跨重启。
    pub started_at_unix: i64,
    /// 任务结束 unix 时间戳（秒）。None = 还在跑。
    pub finished_at_unix: Option<i64>,
    pub book_meta: Option<Book>,
    pub total_chapters: usize,
    pub completed: u32,
    pub failed: u32,
    pub last_chapter_title: String,
    /// `Some(Ok(path))` = 完成；`Some(Err(reason))` = 失败 / 取消（语义分类见 `FinishedReason`）；
    /// `None` = 还在跑。
    pub finished: Option<Result<PathBuf, FinishedReason>>,
    pub failures: Vec<FailureRecord>,
}

/// 失败章节明细。`DownloadTask` 里原本是 `Vec<(u32, String, String)>` 元组，
/// 这里改成 struct 让 serde 序列化为 `{"index":..., "title":..., "reason":...}`，
/// 人类可读且向后兼容性好（加字段不破坏老数据）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureRecord {
    pub index: u32,
    pub title: String,
    pub reason: String,
}

impl From<(u32, String, String)> for FailureRecord {
    fn from((index, title, reason): (u32, String, String)) -> Self {
        Self { index, title, reason }
    }
}

impl From<FailureRecord> for (u32, String, String) {
    fn from(f: FailureRecord) -> Self {
        (f.index, f.title, f.reason)
    }
}

/// 拉所有任务。调用方按需在 Rust 侧排序 / 过滤。
pub fn list(conn: &Connection) -> rusqlite::Result<Vec<DownloadTaskRecord>> {
    let mut stmt = conn.prepare("SELECT data FROM download_tasks")?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let data: String = row.get(0)?;
        match serde_json::from_str::<DownloadTaskRecord>(&data) {
            Ok(rec) => out.push(rec),
            Err(e) => {
                // 单条数据坏了不阻塞整体 — log + 跳过。生产环境可考虑加 dead_letter 表。
                tracing::warn!("download_tasks 行解析失败 ({e})：{data}");
            }
        }
    }
    Ok(out)
}

/// 写入 / 覆盖一条。`id` 是 PK，重复就覆盖。
pub fn upsert(conn: &Connection, rec: &DownloadTaskRecord) -> rusqlite::Result<()> {
    let data = serde_json::to_string(rec).map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e,
        )))
    })?;
    conn.execute(
        "INSERT INTO download_tasks (id, data) VALUES (?1, ?2)
         ON CONFLICT(id) DO UPDATE SET data = excluded.data",
        params![rec.id as i64, data],
    )?;
    Ok(())
}

/// 删所有 `finished IS NOT NULL` 的任务（即已结束 — 完成 / 失败 / 取消），
/// 返回受影响行数。运行中的任务（`finished` 字段为 None）不会动。
pub fn delete_finished(conn: &Connection) -> rusqlite::Result<usize> {
    // 简化实现：拉所有行 → 在 Rust 侧判 finished → 删非 running 的。
    // 几百条规模走全表扫描足够；如果以后上万条再考虑 SQL WHERE 过滤。
    let all = list(conn)?;
    let mut deleted = 0;
    for r in &all {
        if r.finished.is_some() {
            let n = conn.execute(
                "DELETE FROM download_tasks WHERE id = ?1",
                params![r.id as i64],
            )?;
            deleted += n;
        }
    }
    Ok(deleted)
}

/// 删单条（按 id）。返回是否真的删了。
pub fn delete_one(conn: &Connection, id: u64) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "DELETE FROM download_tasks WHERE id = ?1",
        params![id as i64],
    )?;
    Ok(n > 0)
}

/// 按 id 取一条。
pub fn get(conn: &Connection, id: u64) -> rusqlite::Result<Option<DownloadTaskRecord>> {
    let data: Option<String> = conn
        .query_row(
            "SELECT data FROM download_tasks WHERE id = ?1",
            params![id as i64],
            |row| row.get(0),
        )
        .optional()?;
    match data {
        Some(s) => Ok(Some(
            serde_json::from_str(&s).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        e,
                    )),
                )
            })?,
        )),
        None => Ok(None),
    }
}
