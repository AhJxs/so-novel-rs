//! 下载任务的 JSON 文件存储。
//!
//! 数据存放在 `~/.sonovel/tasks.json`。

use std::path::Path;

use crate::core::DownloadTask;
use crate::models::DownloadTaskRecord;

/// 已完成任务的最大保留数量, 超出后按完成时间从旧到新删除; 运行中的任务不受限制。
const MAX_COMPLETED_TASKS: usize = 1000;

/// 从 JSON 文件读取历史任务；文件缺失或损坏时返回空列表。
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

/// 保存所有任务到 JSON 文件（原子写入）。接收运行期 [`DownloadTask`] 切片 → 内部转
/// [`DownloadTaskRecord`] → 写盘; 调用方不再手动 `.map(to_record)`。
pub fn save(path: &Path, tasks: &[DownloadTask]) -> anyhow::Result<()> {
    let records: Vec<DownloadTaskRecord> = tasks.iter().map(DownloadTask::to_record).collect();
    save_records(path, &records)
}

fn save_records(path: &Path, records: &[DownloadTaskRecord]) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(records)?;
    write_json(path, &content)
}

/// 把任务切片序列化成待写盘的 JSON 文本，并顺带清理超额的已完成任务。
///
/// 拆出这一步是为了让 UI 线程只做序列化（`DownloadTask` 含通道，克隆整表代价大），
/// 真正的写盘交给 `spawn_blocking`。
///
/// # Errors
///
/// `serde_json` 序列化失败。
pub fn tasks_json_with_trim(tasks: &[DownloadTask]) -> serde_json::Result<String> {
    let mut records: Vec<DownloadTaskRecord> = tasks.iter().map(DownloadTask::to_record).collect();
    let trimmed = trim_completed(&mut records);
    if trimmed > 0 {
        tracing::info!("自动清理了 {trimmed} 条旧的已完成任务");
    }
    serde_json::to_string_pretty(&records)
}

/// 把已序列化的 JSON 文本原子写入 `path`（父目录缺失时补建）。
///
/// # Errors
///
/// 写文件失败；父目录创建失败只记日志、不中断写入。
pub fn write_json(path: &Path, json: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    super::write_atomically(path, json.as_bytes())?;
    Ok(())
}

/// 清理超额的已完成任务，保留最近的 `MAX_COMPLETED_TASKS` 条，返回被删除的条数。运行中的任务
/// （`finished.is_none()`）不受影响；按 `finished_at_unix` 从新到旧排序，缺该字段的视为最旧。
pub fn trim_completed(tasks: &mut Vec<DownloadTaskRecord>) -> usize {
    let completed_count = tasks.iter().filter(|t| t.finished.is_some()).count();
    if completed_count <= MAX_COMPLETED_TASKS {
        return 0;
    }

    let to_remove = completed_count - MAX_COMPLETED_TASKS;

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

/// 保存任务并自动清理超额的已完成任务（保存前调 `trim_completed`，确保文件大小有界）。
///
/// # Examples
///
/// ```ignore
/// let tasks = state.tasks.lock().expect("tasks lock not poisoned");
/// save_with_trim(&PathBuf::from(".tasks.json"), &tasks)?;
/// ```
///
/// # Errors
///
/// - `std::io::Error` — 序列化或写文件失败
#[tracing::instrument(name = "db::tasks::save_with_trim", skip_all, fields(path = %path.display(), count = tasks.len()))]
pub fn save_with_trim(path: &Path, tasks: &[DownloadTask]) -> anyhow::Result<()> {
    let json = tasks_json_with_trim(tasks)?;
    write_json(path, &json)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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

    fn upsert(tasks: &mut Vec<DownloadTaskRecord>, rec: DownloadTaskRecord) {
        if let Some(existing) = tasks.iter_mut().find(|t| t.id == rec.id) {
            *existing = rec;
        } else {
            tasks.push(rec);
        }
    }

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

        let tasks: Vec<DownloadTask> = vec![
            sample_rec(1, Some(Ok(std::path::PathBuf::from("/tmp/a.epub")))),
            sample_rec(2, None),
        ]
        .into_iter()
        .map(DownloadTask::from_record)
        .collect();
        save(&path, &tasks).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, 1);
        assert_eq!(loaded[1].id, 2);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let tasks = load(Path::new("/definitely/does/not/exist.json"));
        assert!(tasks.is_empty(), "缺失文件应返回空列表");
    }

    #[test]
    fn trim_completed_keeps_running_tasks() {
        let mut tasks = vec![
            sample_rec(1, Some(Ok(std::path::PathBuf::from("/tmp/a.epub")))),
            sample_rec(2, None), // running
            sample_rec(3, Some(Err(FinishedReason::UserCancelled))),
        ];
        let trimmed = trim_completed(&mut tasks);
        assert_eq!(trimmed, 0, "只有 2 条已完成，不需要清理");
        assert_eq!(tasks.len(), 3);
    }

    #[test]
    fn trim_completed_removes_old_when_over_limit() {
        let mut tasks: Vec<DownloadTaskRecord> = (1..=1200)
            .map(|i| {
                let mut rec = sample_rec(
                    i,
                    Some(Ok(std::path::PathBuf::from(format!("/tmp/{i}.epub")))),
                );
                rec.finished_at_unix = Some(1_700_000_000 + i64::try_from(i).unwrap_or(0));
                rec
            })
            .collect();
        tasks.push(sample_rec(1201, None));

        let before = tasks.len();
        let trimmed = trim_completed(&mut tasks);

        assert_eq!(trimmed, 200, "1200 条已完成 - 上限 1000 = 清理 200");
        assert_eq!(tasks.len(), before - 200, "清理后总数应同步减少");
        // 运行中的任务应该保留
        assert!(
            tasks.iter().any(|t| t.id == 1201),
            "运行中的任务不受清理影响"
        );
        // 最新的 1000 条已完成任务应该保留（id 201..=1200）
        assert!(tasks.iter().any(|t| t.id == 1200), "最新的应保留");
        assert!(tasks.iter().any(|t| t.id == 201), "第 1000 条应保留");
        // 最旧的 200 条应该被删除
        assert!(!tasks.iter().any(|t| t.id == 1), "最旧的 200 条应删除");
        assert!(!tasks.iter().any(|t| t.id == 200), "第 200 条应删除");
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
        assert_eq!(trimmed, 0, "未超上限时不应清理");
        assert_eq!(tasks.len(), 50);
    }

    #[test]
    fn save_with_trim_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");

        let tasks: Vec<DownloadTask> = vec![
            sample_rec(1, Some(Ok(std::path::PathBuf::from("/tmp/a.epub")))),
            sample_rec(2, None),
        ]
        .into_iter()
        .map(DownloadTask::from_record)
        .collect();
        save_with_trim(&path, &tasks).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 2);
    }

    /// 验证 [`save_with_trim`] 统一入口在混合状态 (running + finished) 下无损: 既有
    /// roundtrip 测试只校验 `id` / `len`, 这里补 finished payload + completed 字段的端到端检查。
    #[test]
    fn save_with_trim_roundtrips_running_and_finished() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.json");

        let running = sample_rec(1, None);
        let finished = sample_rec(2, Some(Ok(std::path::PathBuf::from("/tmp/done.epub"))));
        let tasks: Vec<DownloadTask> = [running, finished]
            .into_iter()
            .map(DownloadTask::from_record)
            .collect();

        save_with_trim(&path, &tasks).unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 2);
        // save 不重排 —— id 顺序与输入一致
        assert_eq!(loaded[0].id, 1);
        assert!(
            loaded[0].finished.is_none(),
            "running task should stay running after roundtrip"
        );
        assert_eq!(loaded[0].completed, 0);

        assert_eq!(loaded[1].id, 2);
        match &loaded[1].finished {
            Some(Ok(p)) => assert_eq!(p, &std::path::PathBuf::from("/tmp/done.epub")),
            other => panic!("finished task payload lost, got: {other:?}"),
        }
        assert_eq!(loaded[1].completed, 100);
    }
}
