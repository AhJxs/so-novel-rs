//! 书源管理相关业务方法。

use std::path::Path;
use std::sync::Arc;

use crate::core::sources as core_sources;
use crate::db::SourcesConfig;
use crate::error::AppError;
use crate::error::AppResult;
use crate::http::HttpClients;
use crate::models::Rule;

use super::super::sources_state::SourcesState;

/// 切换书源的禁用状态；立即持久化到 `sources_config.json`。
pub fn toggle_source_disabled(
    sources_config: &mut SourcesConfig,
    rules: &mut [Rule],
    source_url: &str,
) {
    let now_disabled = sources_config.toggle_disabled(source_url);
    // 同步内存里的规则状态（URL 键规范化统一在 core::sources）。
    if let Some(r) = rules
        .iter_mut()
        .find(|r| core_sources::rule_key(r) == core_sources::disabled_url_key(source_url))
    {
        r.disabled = now_disabled;
    }
}

/// 从用户选中的 JSON 文件导入书源到 `~/.sonovel/rules/`（同名文件覆盖）。
///
/// 失败返回 `Err(AppError)`, 调用方用 `e.message()` 渲染 toast。
pub fn add_sources_from_file(
    rules_dir: &Path,
    sources_config: &SourcesConfig,
    rules: &mut Vec<Rule>,
    rule_load_error: &mut Option<String>,
    source_path: &Path,
) -> AppResult<ImportResult> {
    let filename = source_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::invalid("无法获取文件名"))?
        .to_string();

    // 验证文件内容有效（解析链统一在 core::sources::parse_rules_bytes）。
    let bytes = std::fs::read(source_path).map_err(|e| AppError::io_msg(&e, "读取文件失败"))?;
    let _: Vec<Rule> = core_sources::parse_rules_bytes(&bytes, source_path)
        .map_err(|e| AppError::internal(format!("{e:#}")))?;

    // 复制文件到 rules 目录（重名覆盖）。
    let dest = rules_dir.join(&filename);
    std::fs::copy(source_path, &dest).map_err(|e| AppError::io_msg(&e, "复制文件失败"))?;

    tracing::info!("已导入书源文件: {}", dest.display());

    // 导入的是当前活跃文件 → 重新加载规则。
    let mut reloaded_active = false;
    if filename == sources_config.active_file {
        match core_sources::load_active(rules_dir, sources_config) {
            Ok(rs) => {
                *rules = rs;
                *rule_load_error = None;
                reloaded_active = true;
            }
            Err(e) => tracing::warn!("导入成功但重载规则失败: {e:#}"),
        }
    }

    Ok(ImportResult {
        filename,
        reloaded_active,
    })
}

/// 导入文件的结果统计。
#[derive(Debug, Clone)]
pub struct ImportResult {
    pub filename: String,
    /// true = 这次导入触发了 `active_file` 重载（rule 集合变了, 旧 results 的
    /// `source_id` 可能指向错源, 调用方需清空搜索状态）。
    pub reloaded_active: bool,
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
) -> AppResult<bool> {
    // URL 键规范化统一在 core::sources。
    let url_key = core_sources::disabled_url_key(source_url);

    // 必须在 retain 之前捕获规则 ID（retain 后就找不到了）。
    let doomed_id = rules
        .iter()
        .find(|r| core_sources::rule_key(r) == url_key)
        .map(|r| r.id);

    let before = rules.len();
    rules.retain(|r| core_sources::rule_key(r) != url_key);
    let deleted = rules.len() < before;

    if deleted {
        // 重新保存活跃文件（原子写入, 防崩溃损坏）。
        let file_path = rules_dir.join(&sources_config.active_file);
        if file_path.exists() {
            let content = serde_json::to_string_pretty(rules)
                .map_err(|e| AppError::internal(format!("序列化失败: {e}")))?;
            crate::db::write_atomically(&file_path, content.as_bytes())
                .map_err(|e| AppError::internal(format!("写入文件失败: {e}")))?;
        }

        // 清掉这条源的健康检查结果。
        if let Some(id) = doomed_id {
            sources_state.health.remove(&id);
        }
    }

    Ok(deleted)
}

/// 切换活跃书源文件, 返回切换后的规则列表。
pub fn switch_active_file(
    rules_dir: &Path,
    sources_config: &mut SourcesConfig,
    rules: &mut Vec<Rule>,
    rule_load_error: &mut Option<String>,
    filename: &str,
) -> AppResult<()> {
    let file_path = rules_dir.join(filename);
    if !file_path.exists() {
        return Err(AppError::not_found(format!("文件不存在: {filename}")));
    }

    sources_config.active_file = filename.to_string();

    match core_sources::load_active(rules_dir, sources_config) {
        Ok(rs) => {
            *rules = rs;
            *rule_load_error = None;
            Ok(())
        }
        Err(e) => {
            *rule_load_error = Some(format!("{e:#}"));
            Err(AppError::internal(format!("加载规则失败: {e:#}")))
        }
    }
}

/// 派一个连通性检测任务到后台。
#[allow(clippy::needless_pass_by_value)]
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
