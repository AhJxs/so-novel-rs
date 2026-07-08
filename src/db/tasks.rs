//! 下载任务的 JSON 文件存储。
//!
//! 数据存放在 `~/.sonovel/tasks.json`。

use std::path::Path;

use super::write_atomically;
use crate::models::DownloadTaskRecord;

/// 已完成任务的最大保留数量。
/// 超出此数量的已完成任务（按完成时间从旧到新）会被自动删除。
/// 运行中的任务不受此限制。
const MAX_COMPLETED_TASKS: usize = 1000;

/// 从 JSON 文件加载所有任务记录。
pub fn load(path: &Path) -> Vec<DownloadTaskRecord> {
    if !path.exists() {
        return Vec::new();
    }
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!("tasks.json 解析失败: {e}");
            Vec::new()
        }),
        Err(e) => {
            tracing::warn!("tasks.json 读取失败: {e}");
            Vec::new()
        }
    }
}

/// 保存所有任务到 JSON 文件（原子写入）。
pub fn save(path: &Path, tasks: &[DownloadTaskRecord]) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(tasks)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    write_atomically(path, content.as_bytes())?;
    Ok(())
}

/// 清理超额的已完成任务，保留最近的 `MAX_COMPLETED_TASKS` 条。
///
/// - 运行中的任务（`finished.is_none()`）不受影响；
/// - 按 `finished_at_unix` 从新到旧排序，保留最新的 N 条；
/// - 没有 `finished_at_unix` 的已完成任务（异常情况）视为最旧；
/// - 返回被删除的条数。
pub fn trim_completed(tasks: &mut Vec<DownloadTaskRecord>) -> usize {
    let completed_count = tasks.iter().filter(|t| t.finished.is_some()).count();
    if completed_count <= MAX_COMPLETED_TASKS {
        return 0;
    }

    let to_remove = completed_count - MAX_COMPLETED_TASKS;

    // 收集已完成任务的 id + 时间戳，按时间从旧到新排序
    let mut completed: Vec<(u64, i64)> = tasks
        .iter()
        .filter(|t| t.finished.is_some())
        .map(|t| (t.id, t.finished_at_unix.unwrap_or(0)))
        .collect();
    completed.sort_by_key(|(_, ts)| *ts);

    // 要删除的 id 集合（最旧的 to_remove 条）
    let remove_ids: std::collections::HashSet<u64> = completed
        .iter()
        .take(to_remove)
        .map(|(id, _)| *id)
        .collect();

    let before = tasks.len();
    tasks.retain(|t| !remove_ids.contains(&t.id));
    before - tasks.len()
}

/// 保存任务并自动清理超额的已完成任务。
///
/// 在保存前调用 `trim_completed`，确保文件大小有界。
///
/// # Examples
///
/// ```ignore
/// let mut tasks = state.tasks.lock().unwrap();
/// save_with_trim(&PathBuf::from(".tasks.json"), &mut tasks)?;
/// ```
///
/// # Errors
///
/// - `std::io::Error` — 序列化或写文件失败
#[tracing::instrument(name = "db::tasks::save_with_trim", skip_all, fields(path = %path.display(), count = tasks.len()))]
pub fn save_with_trim(path: &Path, tasks: &mut Vec<DownloadTaskRecord>) -> anyhow::Result<()> {
    let trimmed = trim_completed(tasks);
    if trimmed > 0 {
        tracing::info!("自动清理了 {trimmed} 条旧的已完成任务");
    }
    save(path, tasks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{FinishedReason, SearchResult};

    fn sample_rec(
        id: u64,
        finished: Option<Result<std::path::PathBuf, FinishedReason>>,
    ) -> DownloadTaskRecord {
        DownloadTaskRecord {
            id,
            origin: SearchResult::default(),
            started_at_unix: 1_700_000_000,
            finished_at_unix: finished.as_ref().map(|_| 1_700_000_100),
            book_meta: None,
            total_chapters: 100,
            completed: if finished.is_some() { 100 } else { 0 },
            failed: 0,
            last_chapter_title: String::new(),
            finished,
            failures: Vec::new(),
        }
    }

    /// 测试辅助：插入或更新一条任务记录。
    fn upsert(tasks: &mut Vec<DownloadTaskRecord>, rec: DownloadTaskRecord) {
        if let Some(existing) = tasks.iter_mut().find(|t| t.id == rec.id) {
            *existing = rec;
        } else {
            tasks.push(rec);
        }
    }

    /// 测试辅助：获取下一个可用的任务 ID。
    fn next_task_id(tasks: &[DownloadTaskRecord]) -> u64 {
        tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1
    }

    #[test]
    fn upsert_inserts_new() {
        let mut tasks = Vec::new();
        let rec = sample_rec(1, None);
        upsert(&mut tasks, rec);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, 1);
    }

    #[test]
    fn upsert_updates_existing() {
        let mut tasks = vec![sample_rec(1, None)];
        let mut updated = sample_rec(1, None);
        updated.total_chapters = 200;
        upsert(&mut tasks, updated);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].total_chapters, 200);
    }

    #[test]
    fn next_task_id_returns_max_plus_one() {
        let tasks = vec![
            sample_rec(1, None),
            sample_rec(5, None),
            sample_rec(3, None),
        ];
        assert_eq!(next_task_id(&tasks), 6);
    }

    #[test]
    fn next_task_id_empty_returns_one() {
        let tasks: Vec<DownloadTaskRecord> = Vec::new();
        assert_eq!(next_task_id(&tasks), 1);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");

        let tasks = vec![
            sample_rec(1, Some(Ok(std::path::PathBuf::from("/tmp/a.epub")))),
            sample_rec(2, None),
        ];
        save(&path, &tasks).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, 1);
        assert_eq!(loaded[1].id, 2);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let tasks = load(Path::new("/definitely/does/not/exist.json"));
        assert!(tasks.is_empty());
    }

    #[test]
    fn trim_completed_keeps_running_tasks() {
        let mut tasks = vec![
            sample_rec(1, Some(Ok(std::path::PathBuf::from("/tmp/a.epub")))),
            sample_rec(2, None), // running
            sample_rec(3, Some(Err(FinishedReason::UserCancelled))),
        ];
        let trimmed = trim_completed(&mut tasks);
        assert_eq!(trimmed, 0); // 只有 2 条已完成，不需要清理
        assert_eq!(tasks.len(), 3);
    }

    #[test]
    fn trim_completed_removes_old_when_over_limit() {
        // 构造超过 MAX_COMPLETED_TASKS 条已完成任务
        let mut tasks: Vec<DownloadTaskRecord> = (1..=1200)
            .map(|i| {
                let mut rec = sample_rec(
                    i,
                    Some(Ok(std::path::PathBuf::from(format!("/tmp/{i}.epub")))),
                );
                rec.finished_at_unix = Some(1_700_000_000 + i as i64);
                rec
            })
            .collect();
        // 加一条运行中的任务
        tasks.push(sample_rec(1201, None));

        let before = tasks.len();
        let trimmed = trim_completed(&mut tasks);

        assert_eq!(trimmed, 200); // 1200 - 1000 = 200
        assert_eq!(tasks.len(), before - 200);
        // 运行中的任务应该保留
        assert!(tasks.iter().any(|t| t.id == 1201));
        // 最新的 1000 条已完成任务应该保留（id 201..=1200）
        assert!(tasks.iter().any(|t| t.id == 1200));
        assert!(tasks.iter().any(|t| t.id == 201));
        // 最旧的 200 条应该被删除
        assert!(!tasks.iter().any(|t| t.id == 1));
        assert!(!tasks.iter().any(|t| t.id == 200));
    }

    #[test]
    fn trim_completed_noop_when_under_limit() {
        let mut tasks: Vec<DownloadTaskRecord> = (1..=50)
            .map(|i| {
                sample_rec(
                    i,
                    Some(Ok(std::path::PathBuf::from(format!("/tmp/{i}.epub")))),
                )
            })
            .collect();

        let trimmed = trim_completed(&mut tasks);
        assert_eq!(trimmed, 0);
        assert_eq!(tasks.len(), 50);
    }

    #[test]
    fn save_with_trim_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");

        let mut tasks = vec![
            sample_rec(1, Some(Ok(std::path::PathBuf::from("/tmp/a.epub")))),
            sample_rec(2, None),
        ];
        save_with_trim(&path, &mut tasks).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 2);
    }
}
