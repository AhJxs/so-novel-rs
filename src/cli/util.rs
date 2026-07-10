//! CLI 子命令共享工具：合并下载目录/格式覆盖、按书源规则加载启用列表、
//! 章节范围校验。TTY 原地进度行打印见 `crate::utils::tty`。

use anyhow::{Context, Result};

use crate::config::{AppConfig, ConfigPaths, ExportFormat};
use crate::db::{SourcesConfig, load_active_rules};
use crate::models::Rule;

/// 把 `--output` / `--format` 覆盖合并进 AppConfig（仅 download 用）。
pub fn effective_cfg(
    mut cfg: AppConfig,
    output: Option<String>,
    format: Option<String>,
) -> AppConfig {
    if let Some(o) = output {
        cfg.download.download_path = o;
    }
    if let Some(f) = format {
        cfg.download.ext_name = ExportFormat::parse(&f);
    }
    cfg
}

/// 读 `sources_config.json` + rules dir，返回所有"未被 sources_config 禁用"的 Rule 列表。
///
/// **注意**:
/// - 这是 cli 启动期的快捷入口，已应用 [`SourcesConfig`] 里 `disabled_urls` 的过滤。
/// - **不**再过滤 rule.disabled / search.disabled —— 这两步由调用方通过
///   [`crate::core::search::select_sources`] 统一处理（搜索场景用 `is_search_enabled`，
///   下载场景按需自选），避免 core 模块需要知道"调用方有没有预过滤"的歧义。
///
/// Phase 3.3 时本函数会搬到 `crate::core::bootstrap::load_active_sources`。
pub fn load_active_sources(paths: &ConfigPaths) -> Result<Vec<Rule>> {
    let sources_config = SourcesConfig::load(&paths.sources_config);
    load_active_rules(&paths.rules_dir, &sources_config).context("加载规则失败")
}

/// 校验并规范化 download 的 `--from` / `--to` 范围。
///
/// 规则：
/// - `from` / `to` 都是 1-based
/// - `from == 0` → 报错（1-based）
/// - `from > total` → 报错（明确越界，提示用户用 `sources list` 查章数）
/// - `to > total` → 静默截断到 `total`（用户可能没数对，友好兜底）
/// - `from` / `to` 任一为 `None` → 默认 `from=1` / `to=total`
///
/// 返回 `(from, to_clamped)`，可直接用于切片。
pub fn validate_range(
    from: Option<usize>,
    to: Option<usize>,
    total: usize,
) -> anyhow::Result<(usize, usize)> {
    let from = from.unwrap_or(1);
    let to_requested = to.unwrap_or(total);
    if from == 0 {
        anyhow::bail!("章节索引从 1 开始（--from 不能为 0）");
    }
    if from > total {
        anyhow::bail!("--from ({from}) 超出总章节数 ({total})");
    }
    // 到这里 from ≤ total，所以 to = min(to_requested, total) 也 ≤ from 不会发生。
    let to = to_requested.min(total);
    Ok((from, to))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    // ----- validate_range -----

    #[test]
    fn validate_range_both_none_uses_full_range() {
        assert_eq!(validate_range(None, None, 100).unwrap(), (1, 100));
    }

    #[test]
    fn validate_range_from_only_keeps_to_at_end() {
        assert_eq!(validate_range(Some(50), None, 100).unwrap(), (50, 100));
    }

    #[test]
    fn validate_range_to_only_keeps_from_at_one() {
        assert_eq!(validate_range(None, Some(30), 100).unwrap(), (1, 30));
    }

    #[test]
    fn validate_range_clamps_to_when_exceeds_total() {
        // 友好：to 超出时静默截断，不报错
        assert_eq!(validate_range(Some(10), Some(999), 100).unwrap(), (10, 100));
    }

    #[test]
    fn validate_range_rejects_from_zero() {
        // 1-based：0 不合法
        assert!(validate_range(Some(0), None, 100).is_err());
    }

    #[test]
    fn validate_range_rejects_from_beyond_total() {
        // from 越界：明确报错（不像 to，from 是用户意图起点）
        assert!(validate_range(Some(101), None, 100).is_err());
    }

    #[test]
    fn validate_range_accepts_boundary_values() {
        // from == total、to == total 都合法（单章下载）
        assert_eq!(validate_range(Some(1), Some(1), 1).unwrap(), (1, 1));
        assert_eq!(
            validate_range(Some(100), Some(100), 100).unwrap(),
            (100, 100)
        );
    }
}
