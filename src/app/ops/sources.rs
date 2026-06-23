//! 书源管理相关业务方法。

use std::path::Path;
use std::sync::Arc;

use crate::http::HttpClients;
use crate::models::Rule;
use crate::persistent::SourcesConfig;

use super::super::sources_state::SourcesState;

/// 切换书源的禁用状态；立即持久化到 `sources_config.json`。
pub fn toggle_source_disabled(
    sources_config: &mut SourcesConfig,
    rules: &mut [Rule],
    source_url: &str,
) {
    let now_disabled = sources_config.toggle_disabled(source_url);
    // 更新内存中的规则状态
    let url_key = source_url.trim().to_lowercase();
    if let Some(r) = rules
        .iter_mut()
        .find(|r| r.url.trim().to_lowercase() == url_key)
    {
        r.disabled = now_disabled;
    }
}

/// 从用户选中的 JSON 文件导入书源到 `~/.sonovel/rules/`。
///
/// **去重**：文件名相同时 replace（覆盖）。
/// 返回 `ImportResult { filename }`。
///
/// 失败：返回 `Err(msg)`，msg 走 toast notification。
pub fn add_sources_from_file(
    rules_dir: &Path,
    sources_config: &mut SourcesConfig,
    rules: &mut Vec<Rule>,
    rule_load_error: &mut Option<String>,
    source_path: &Path,
) -> Result<ImportResult, String> {
    let filename = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "无法获取文件名".to_string())?
        .to_string();

    // 验证文件内容是否有效
    let bytes = std::fs::read(source_path).map_err(|e| format!("读取文件失败: {e}"))?;
    let text = String::from_utf8_lossy(&bytes);

    let _: Vec<Rule> = serde_json::from_str::<Vec<Rule>>(&text)
        .or_else(|_| serde_json::from_str::<Rule>(&text).map(|r| vec![r]))
        .or_else(|_| json5::from_str::<Vec<Rule>>(&text))
        .or_else(|_| json5::from_str::<Rule>(&text).map(|r| vec![r]))
        .map_err(|e| format!("解析失败: {e}"))?;

    // 复制文件到 rules 目录（重名则覆盖）
    let dest = rules_dir.join(&filename);
    std::fs::copy(source_path, &dest).map_err(|e| format!("复制文件失败: {e}"))?;

    tracing::info!("已导入书源文件: {}", dest.display());

    // 如果导入的是当前活跃文件，重新加载规则
    if filename == sources_config.active_file {
        match crate::persistent::load_active_rules(rules_dir, sources_config) {
            Ok(rs) => {
                *rules = rs;
                *rule_load_error = None;
            }
            Err(e) => {
                tracing::warn!("导入成功但重载规则失败: {e:#}");
            }
        }
    }

    Ok(ImportResult { filename })
}

/// 导入文件的结果统计。
#[derive(Debug, Clone)]
pub struct ImportResult {
    pub filename: String,
}

/// 删除一条书源（从当前活跃文件中移除）。
///
/// 返回 (是否真删了, 错误信息 toast 文本)
pub fn delete_source(
    rules_dir: &Path,
    sources_config: &SourcesConfig,
    rules: &mut Vec<Rule>,
    sources_state: &mut SourcesState,
    source_url: &str,
) -> Result<bool, String> {
    let url_key = source_url.trim().to_lowercase();

    // 在 retain 之前捕获要删除的规则 ID（retain 后就找不到了）
    let doomed_id = rules
        .iter()
        .find(|r| r.url.trim().to_lowercase() == url_key)
        .map(|r| r.id);

    // 从内存中移除
    let before = rules.len();
    rules.retain(|r| r.url.trim().to_lowercase() != url_key);
    let deleted = rules.len() < before;

    if deleted {
        // 从活跃文件中重新保存（原子写入，防止崩溃时损坏文件）
        let file_path = rules_dir.join(&sources_config.active_file);
        if file_path.exists() {
            let content =
                serde_json::to_string_pretty(rules).map_err(|e| format!("序列化失败: {e}"))?;
            crate::persistent::write_atomically(&file_path, content.as_bytes())
                .map_err(|e| format!("写入文件失败: {e}"))?;
        }

        // 清理健康检查状态
        if let Some(id) = doomed_id {
            sources_state.health.remove(&id);
        }
    }

    Ok(deleted)
}

/// 切换活跃书源文件。
///
/// 返回切换后的规则列表。
pub fn switch_active_file(
    rules_dir: &Path,
    sources_config: &mut SourcesConfig,
    rules: &mut Vec<Rule>,
    rule_load_error: &mut Option<String>,
    filename: &str,
) -> Result<(), String> {
    let file_path = rules_dir.join(filename);
    if !file_path.exists() {
        return Err(format!("文件不存在: {filename}"));
    }

    sources_config.active_file = filename.to_string();

    match crate::persistent::load_active_rules(rules_dir, sources_config) {
        Ok(rs) => {
            *rules = rs;
            *rule_load_error = None;
            Ok(())
        }
        Err(e) => {
            *rule_load_error = Some(format!("{e:#}"));
            Err(format!("加载规则失败: {e:#}"))
        }
    }
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
