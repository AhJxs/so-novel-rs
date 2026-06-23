//! 书源管理相关业务方法。

use std::path::Path;
use std::sync::Arc;

use crate::db::Db;
use crate::http::HttpClients;
use crate::models::Rule;
use crate::rules::SourceOverrides;

use super::super::sources_state::SourcesState;

/// 切换书源的禁用状态；立即持久化到 `sonovel.db` 的 `source_overrides` 表。
pub fn toggle_source_disabled(
    db: &Db,
    source_overrides: &mut SourceOverrides,
    rules: &mut [Rule],
    source_id: i32,
) {
    match crate::db::sources::toggle_disabled(db.conn(), source_id) {
        Ok(now_disabled) => {
            if now_disabled {
                source_overrides.disabled.insert(source_id);
            } else {
                source_overrides.disabled.remove(&source_id);
            }
            if let Some(r) = rules.iter_mut().find(|r| r.id == source_id) {
                r.disabled = now_disabled;
            }
        }
        Err(e) => {
            tracing::warn!("source_overrides toggle 失败: {e:#}");
        }
    }
}

/// 从用户选中的 JSON 文件导入书源到 sonovel.db。
///
/// **去重**：DB 中已存在的 `url`（忽略大小写、首尾空格）会被跳过，不重复插入。
/// 返回 `ImportResult { inserted, skipped }` —— `skipped` 即被去重跳过的条数。
/// 全部被跳过时返回 `Ok(ImportResult { inserted: 0, skipped: n })`（不视为错误，
/// 调用方据此给用户一个 info 文案）。
///
/// 失败：返回 `Err(msg)`，msg 走 toast notification。
pub fn add_sources_from_file(
    db: &mut Db,
    rules: &mut Vec<Rule>,
    rule_load_error: &mut Option<String>,
    path: &Path,
) -> Result<ImportResult, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取文件失败: {e}"))?;
    let text = String::from_utf8_lossy(&bytes);

    let rules_vec: Vec<Rule> = serde_json::from_str::<Vec<Rule>>(&text)
        .or_else(|_| serde_json::from_str::<Rule>(&text).map(|r| vec![r]))
        .or_else(|_| json5::from_str::<Vec<Rule>>(&text))
        .or_else(|_| json5::from_str::<Rule>(&text).map(|r| vec![r]))
        .map_err(|e| format!("解析失败: {e}"))?;

    if rules_vec.is_empty() {
        return Err("文件内容为空，未导入任何书源".to_string());
    }

    let valid_count = rules_vec
        .iter()
        .filter(|r| !r.url.trim().is_empty() || !r.name.trim().is_empty())
        .count();
    if valid_count == 0 {
        return Err("文件中未找到有效的书源条目".to_string());
    }

    // **去重**：先查 DB 已有的 url 集合，过滤掉文件中 url 重复的条目。
    //
    // 用 lowercase 比较：https://Foo.com 和 https://foo.com 视为同一源。trim 头尾
    // 空格后空字符串跳过（"占位规则"靠 name 唯一性区分，不靠 url）。
    let existing_urls = crate::db::sources::list_existing_urls(db.conn())
        .map_err(|e| format!("查询已有书源失败: {e}"))?;
    let mut to_insert: Vec<Rule> = Vec::with_capacity(rules_vec.len());
    let mut skipped: usize = 0;
    for r in rules_vec {
        let key = r.url.trim().to_lowercase();
        if !key.is_empty() && existing_urls.contains(&key) {
            skipped += 1;
        } else {
            to_insert.push(r);
        }
    }

    if to_insert.is_empty() {
        // 全部 url 都已存在 —— 不写库，提示"全部已存在"。
        return Ok(ImportResult {
            inserted: 0,
            skipped,
        });
    }

    let n = crate::db::sources::insert_many(db.conn_mut(), &to_insert)
        .map_err(|e| format!("导入失败: {e}"))?;

    match crate::rules::load_rules_from_db(db.conn_mut()) {
        Ok(rs) => {
            *rules = rs;
            *rule_load_error = None;
        }
        Err(e) => {
            tracing::warn!("插入成功但重载规则失败: {e:#}");
        }
    }
    Ok(ImportResult {
        inserted: n,
        skipped,
    })
}

/// 导入文件的结果统计。`inserted` 实际写入 DB 的条数，`skipped` 因 url 重复被
/// 去重跳过的条数。
#[derive(Debug, Clone, Copy, Default)]
pub struct ImportResult {
    pub inserted: usize,
    pub skipped: usize,
}

/// 删除一条书源（DB + 内存中的 rules / overrides / health 都同步清掉）。
///
/// 返回 (是否真删了, 错误信息 toast 文本)
pub fn delete_source(
    db: &mut Db,
    rules: &mut Vec<Rule>,
    source_overrides: &mut SourceOverrides,
    sources_state: &mut SourcesState,
    source_id: i32,
) -> Result<bool, String> {
    let deleted = crate::db::sources::delete_one(db.conn_mut(), source_id)
        .map_err(|e| format!("删除失败: {e}"))?;
    if deleted {
        rules.retain(|r| r.id != source_id);
        source_overrides.disabled.remove(&source_id);
        sources_state.health.remove(&source_id);
    } else {
        // id 已不在 DB；同步内存即可
        rules.retain(|r| r.id != source_id);
    }
    Ok(deleted)
}

/// 派一个连通性检测任务到后台。
pub fn spawn_health_check(
    rules: &[Rule],
    http: Arc<HttpClients>,
    runtime: &tokio::runtime::Runtime,
    sources_state: &mut SourcesState,
) {
    if sources_state.running {
        return;
    }
    if rules.is_empty() {
        return;
    }

    sources_state.health.clear();
    sources_state.received = 0;
    sources_state.expected = rules.len();
    sources_state.running = true;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    sources_state.rx = Some(rx);

    let http = Arc::clone(&http);
    let rules = rules.to_vec();
    runtime.spawn(async move {
        crate::crawler::health::check_sources_health(http, rules, tx).await;
    });
}
