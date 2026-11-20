//! 下载任务的 JSON 序列化 / 反序列化。
//!
//! blob 本身由 [`crate::db::StateStore`] 存在 `~/.sonovel/state`，这里只管字节。

use crate::core::DownloadTask;
use crate::models::DownloadTaskRecord;

/// 已完成任务的最大保留数量, 超出后按完成时间从旧到新删除; 运行中的任务不受限制。
///
/// 落盘裁剪（[`trim_completed`]）与运行时裁剪（[`trim_completed_runtime`]）共用这一阈值。
pub const MAX_COMPLETED_TASKS: usize = 1000;

/// 从 JSON 字节解析历史任务；损坏时返回空列表。
#[must_use]
pub fn parse(bytes: &[u8]) -> Vec<DownloadTaskRecord> {
    serde_json::from_slice(bytes).unwrap_or_else(|e| {
        tracing::warn!("下载任务数据解析失败: {e}");
        Vec::new()
    })
}

/// 把任务切片序列化成待写入的 JSON 文本，并顺带清理超额的已完成任务。
///
/// 拆出这一步是为了让 UI 线程只做序列化（`DownloadTask` 含通道，克隆整表代价大）。
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

/// 运行时的任务表裁剪：直接在 `Vec<DownloadTask>` 上删掉最旧的超额已完成任务。
///
/// 阈值与落盘时的 [`trim_completed`] 相同（最近 [`MAX_COMPLETED_TASKS`] 条已完成），
/// 保留原有顺序，运行中的任务永不删除。返回是否有任务被删（调用方据此决定落盘）。
pub fn trim_completed_runtime(tasks: &mut Vec<DownloadTask>) -> bool {
    let completed_count = tasks.iter().filter(|t| t.finished.is_some()).count();
    if completed_count <= MAX_COMPLETED_TASKS {
        return false;
    }
    let to_remove = completed_count - MAX_COMPLETED_TASKS;

    // 先记下索引，再按 `finished_at_unix` 从旧到新排出要删的位置；缺该字段的视为最旧。
    let mut completed: Vec<(usize, i64)> = tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.finished.is_some())
        .map(|(index, t)| (index, t.finished_at_unix.unwrap_or(0)))
        .collect();
    completed.sort_by_key(|(_, ts)| *ts);

    let mut remove = vec![false; tasks.len()];
    for (index, _) in completed.into_iter().take(to_remove) {
        remove[index] = true;
    }
    // `retain` 按原顺序访问元素，用同步推进的下标决定去留，顺序因此保持不变。
    let mut index = 0;
    tasks.retain(|_| {
        let keep = !remove[index];
        index += 1;
        keep
    });
    tracing::info!("运行时自动清理了 {to_remove} 条旧的已完成任务");
    true
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
    fn json_round_trip() {
        let tasks: Vec<DownloadTask> = vec![
            sample_rec(1, Some(Ok(std::path::PathBuf::from("/tmp/a.epub")))),
            sample_rec(2, None),
        ]
        .into_iter()
        .map(DownloadTask::from_record)
        .collect();
        let json = tasks_json_with_trim(&tasks).unwrap();

        let loaded = parse(json.as_bytes());
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, 1);
        assert_eq!(loaded[1].id, 2);
    }

    #[test]
    fn malformed_json_returns_empty() {
        assert!(parse(b"{ not json").is_empty());
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

    fn completed_task(id: u64, finished_at: i64) -> DownloadTask {
        let mut rec = sample_rec(
            id,
            Some(Ok(std::path::PathBuf::from(format!("/tmp/{id}.epub")))),
        );
        rec.finished_at_unix = Some(finished_at);
        DownloadTask::from_record(rec)
    }

    fn running_task(id: u64) -> DownloadTask {
        DownloadTask::from_record(sample_rec(id, None))
    }

    #[test]
    fn trim_completed_runtime_keeps_newest_in_order() {
        let mut tasks: Vec<DownloadTask> = Vec::new();
        for id in 1..=600 {
            tasks.push(completed_task(
                id,
                1_700_000_000 + i64::try_from(id).unwrap_or(0),
            ));
        }
        // 运行中的任务夹在中间：裁剪后位置也不能变。
        tasks.push(running_task(1300));
        for id in 601..=1205 {
            tasks.push(completed_task(
                id,
                1_700_000_000 + i64::try_from(id).unwrap_or(0),
            ));
        }

        assert!(
            trim_completed_runtime(&mut tasks),
            "1205 条已完成超过上限，应返回 true"
        );

        let ids: Vec<u64> = tasks.iter().map(|t| t.id).collect();
        let mut expected: Vec<u64> = (206..=600).collect();
        expected.push(1300);
        expected.extend(601..=1205);
        assert_eq!(ids, expected, "只删最旧的 205 条；顺序与运行中任务都不变");
    }

    #[test]
    fn trim_completed_runtime_noop_under_limit() {
        let mut tasks: Vec<DownloadTask> = (1..=999)
            .map(|id| completed_task(id, 1_000 + i64::try_from(id).unwrap_or(0)))
            .collect();
        tasks.push(running_task(5000));

        assert!(
            !trim_completed_runtime(&mut tasks),
            "未超上限时应返回 false"
        );
        assert_eq!(tasks.len(), 1000, "未超上限时不应改动表");
    }

    #[test]
    fn trim_completed_runtime_treats_missing_finished_at_as_oldest() {
        let mut tasks: Vec<DownloadTask> = Vec::new();
        let mut no_ts = sample_rec(1, Some(Ok(std::path::PathBuf::from("/tmp/1.epub"))));
        no_ts.finished_at_unix = None;
        tasks.push(DownloadTask::from_record(no_ts));
        for id in 2..=1001 {
            tasks.push(completed_task(
                id,
                1_700_000_000 + i64::try_from(id).unwrap_or(0),
            ));
        }

        assert!(trim_completed_runtime(&mut tasks));
        assert_eq!(tasks.len(), 1000);
        assert!(
            !tasks.iter().any(|t| t.id == 1),
            "缺完成时间视为最旧，应最先被删"
        );
        assert!(tasks.iter().any(|t| t.id == 1001), "最新的应保留");
    }

    /// 混合状态 (running + finished) 下序列化无损：finished payload + completed 字段都要在。
    #[test]
    fn json_roundtrips_running_and_finished() {
        let running = sample_rec(1, None);
        let finished = sample_rec(2, Some(Ok(std::path::PathBuf::from("/tmp/done.epub"))));
        let tasks: Vec<DownloadTask> = [running, finished]
            .into_iter()
            .map(DownloadTask::from_record)
            .collect();

        let json = tasks_json_with_trim(&tasks).unwrap();
        let loaded = parse(json.as_bytes());
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
