//! 书源管理相关业务方法。

use std::path::Path;
use std::sync::Arc;

use crate::db::Db;
use crate::models::Rule;

use super::super::sources_state::SourcesState;

/// 切换书源的禁用状态；立即持久化到 `sonovel.db` 的 `source_overrides` 表。
pub fn toggle_source_disabled(
    db: &Db,
    source_overrides: &mut crate::rules::SourceOverrides,
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
/// 返回 (导入数量, 错误信息 toast 文本)
/// - 成功：`Some(n)`
/// - 失败：`None` + 错误文案
pub fn add_sources_from_file(
    db: &mut Db,
    rules: &mut Vec<Rule>,
    rule_load_error: &mut Option<String>,
    path: &Path,
) -> Result<usize, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取文件失败: {e}"))?;
    let text = String::from_utf8_lossy(&bytes);

    let rules_vec: Vec<Rule> = match serde_json::from_str::<Vec<Rule>>(&text) {
        Ok(v) => v,
        Err(_) => match serde_json::from_str::<Rule>(&text) {
            Ok(one) => vec![one],
            Err(_) => match json5::from_str::<Vec<Rule>>(&text) {
                Ok(v) => v,
                Err(_) => match json5::from_str::<Rule>(&text) {
                    Ok(one) => vec![one],
                    Err(e) => return Err(format!("解析失败: {e}")),
                },
            },
        },
    };

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

    let n = crate::db::sources::insert_many(db.conn_mut(), &rules_vec)
        .map_err(|e| format!("导入失败: {e}"))?;

    match crate::rules::load_rules_from_db(db.conn()) {
        Ok(rs) => {
            *rules = rs;
            *rule_load_error = None;
        }
        Err(e) => {
            tracing::warn!("插入成功但重载规则失败: {e:#}");
        }
    }
    Ok(n)
}

/// 删除一条书源（DB + 内存中的 rules / overrides / health 都同步清掉）。
///
/// 返回 (是否真删了, 错误信息 toast 文本)
pub fn delete_source(
    db: &mut Db,
    rules: &mut Vec<Rule>,
    source_overrides: &mut crate::rules::SourceOverrides,
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
    sources_state.pending_delete = None;
    Ok(deleted)
}

/// 派一个连通性检测任务到后台。
pub fn spawn_health_check(
    rules: &[Rule],
    config: &crate::config::AppConfig,
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

    let cfg = Arc::new(config.clone());
    let rules = rules.to_vec();
    runtime.spawn(async move {
        crate::crawler::health::check_sources_health(cfg, rules, tx).await;
    });
}
