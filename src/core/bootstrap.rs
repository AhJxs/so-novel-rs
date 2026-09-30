//! 三端共用的启动期公共资源加载 + 几样从 `cli/util.rs` 搬来的薄壳。
//!
//! `AppContext::load_context` **不**返回 `Result`：三端容错策略不一致（desktop 各处
//! `tracing::warn!` + 兜底默认、web 走 `unwrap_or_default()`、cli 用 `anyhow::Result`
//! 但也不想在这里 panic），所以内部所有 IO 失败都吞掉 + warn，返回尽力凑齐的 `AppContext`。
//!
//! `effective_cfg` / `validate_range` / `load_active_sources` 从已删的 `cli/util.rs` 搬来，
//! 目前 CLI 是唯一调用方，放这里供 desktop / web 将来复用。

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::config::{AppConfig, ConfigPaths, ExportFormat};
use crate::db::{SourcesConfig, init_rules_dir, load_active_rules};
use crate::http::HttpClients;
use crate::models::Rule;

/// 启动期公共资源聚合（`paths` + `config` + `sources_config` + `rules` + `http`）。
/// desktop 拿它加 `tasks / runtime / wakeup` 拼 state；web 另加 `load_tasks_from_file`；cli 只用 paths + cfg。
pub struct AppContext {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub sources_config: SourcesConfig,
    pub rules: Vec<Rule>,
    pub http: Arc<HttpClients>,
}

/// 启动期"凑齐所有公共资源"的统一入口。**不**返回 `Result`。
///
/// 所有失败一律 `tracing::warn!` + 兜底默认：`load_config` → 默认配置；首次写 config /
/// `init_rules_dir` / 首次写 `sources_config` → 仅 warn 不阻塞；`load_active_rules` → 空
/// Vec；`HttpClients::new` → 默认 client。
///
/// # Panics
///
/// 不会 panic：所有失败路径都 swallow 走兜底。
pub fn load_context() -> AppContext {
    let paths = ConfigPaths::discover();

    // config.toml
    let (config, config_err) = match crate::config::load_config(&paths.config_file) {
        Ok(c) => (c, None),
        Err(e) => {
            tracing::warn!("config load failed: {e:#}");
            (AppConfig::default(), Some(format!("{e:#}")))
        }
    };
    // 首次启动写出默认 config（让用户立刻能在项目根看到 config.toml 可编辑）
    if !paths.config_file.exists() {
        if let Err(e) = crate::config::save_config(&paths.config_file, &config) {
            tracing::warn!("写入默认 config.toml 失败: {e:#}");
        } else {
            tracing::info!("首次启动：已生成 {}", paths.config_file.display());
        }
    }
    // config_err 暂未暴露 —— desktop 之后如需在 UI 上提示，可加进 AppContext。
    let _ = config_err;

    // 规则目录（首次启动时复制默认规则文件）
    if let Err(e) = init_rules_dir(&paths.rules_dir) {
        tracing::warn!("规则目录初始化失败: {e:#}");
    }

    let sources_config = SourcesConfig::load(&paths.sources_config);
    if !paths.sources_config.exists()
        && let Err(e) = sources_config.save(&paths.sources_config)
    {
        tracing::warn!("写入默认 sources_config.json 失败: {e:#}");
    }

    let rules = match load_active_rules(&paths.rules_dir, &sources_config) {
        Ok(rs) => rs,
        Err(e) => {
            tracing::warn!("rules load failed: {e:#}");
            Vec::new()
        }
    };

    // 常见失败原因是 proxy URL 畸形：原始 cfg → 关闭 proxy → 空 stub 三步 fallback。
    let http = match HttpClients::new(&config) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::warn!("HttpClients init failed: {e:#}，尝试关闭 proxy 后重试");
            let mut cfg_no_proxy = config.clone();
            cfg_no_proxy.proxy.proxy_enabled = false;
            match HttpClients::new(&cfg_no_proxy) {
                Ok(c) => Arc::new(c),
                Err(e2) => {
                    tracing::error!("HttpClients init 重试仍失败: {e2:#}；fall back to empty stub");
                    Arc::new(HttpClients::empty())
                }
            }
        }
    };

    AppContext {
        paths,
        config,
        sources_config,
        rules,
        http,
    }
}

/// 把 `--output` / `--format` 覆盖合并进 `AppConfig`（仅 download 用）。
/// 搬到 `core` 是为了统一 CLI 入口的依赖来源，desktop / web 将来做"hook 下载"也能复用。
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

/// 读 `sources_config.json` + rules dir，返回所有"未被 `sources_config` 禁用"的 Rule 列表。
///
/// 已应用 `disabled_urls` 过滤，但**不**过滤 rule.disabled / search.disabled —— 那两步由调用方走
/// [`crate::core::search::select_sources`] 统一处理，避免 core 去猜"调用方有没有预过滤"。
pub fn load_active_sources(paths: &ConfigPaths) -> Result<Vec<Rule>> {
    let sources_config = SourcesConfig::load(&paths.sources_config);
    load_active_rules(&paths.rules_dir, &sources_config).context("加载规则失败")
}

/// 校验并规范化 download 的 `--from` / `--to` 范围（都是 1-based）：`from == 0` 或 `from > total` → 报错；
/// `to > total` → 静默截断到 `total`（友好兜底）；任一为 `None` → 默认 `from=1` / `to=total`。
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
    let to = to_requested.min(total);
    Ok((from, to))
}

/// 一次性跑完 CLI 启动期的全部 IO + 写出默认 config 等（`ConfigPaths::discover` →
/// `load_config` → 首次 `save_config` → `init_rules_dir`）。
///
/// 不含子命令 dispatch / locale 切换 / tracing init。返回 `Result` 是因为 CLI 想要
/// malformed TOML 明确失败，不像 `load_context` 全兜底。
pub fn cli_load_paths_and_config() -> Result<(ConfigPaths, AppConfig)> {
    let paths = ConfigPaths::discover();
    let cfg = crate::config::load_config(&paths.config_file).context("加载 config.toml 失败")?;

    // 与 GUI 启动行为一致：首次运行写出默认 config.toml，用户立刻能编辑；失败仅警告，不阻塞 CLI。
    if !paths.config_file.exists() {
        if let Err(e) = crate::config::save_config(&paths.config_file, &cfg) {
            tracing::warn!("写入默认 config.toml 失败: {e:#}");
        } else {
            tracing::info!("首次运行：已生成 {}", paths.config_file.display());
        }
    }

    if let Err(e) = init_rules_dir(&paths.rules_dir) {
        tracing::warn!("规则目录初始化失败: {e:#}");
    }

    Ok((paths, cfg))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

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
        assert_eq!(validate_range(Some(10), Some(999), 100).unwrap(), (10, 100));
    }

    #[test]
    fn validate_range_rejects_from_zero() {
        assert!(validate_range(Some(0), None, 100).is_err());
    }

    #[test]
    fn validate_range_rejects_from_beyond_total() {
        // from 越界明确报错（不像 to）—— from 是用户指定的起点。
        assert!(validate_range(Some(101), None, 100).is_err());
    }

    #[test]
    fn validate_range_accepts_boundary_values() {
        // from == total 合法（单章下载）。
        assert_eq!(validate_range(Some(1), Some(1), 1).unwrap(), (1, 1));
        assert_eq!(
            validate_range(Some(100), Some(100), 100).unwrap(),
            (100, 100)
        );
    }

    #[test]
    fn effective_cfg_overrides_output_and_format() {
        let cfg = AppConfig::default();
        let new_cfg = effective_cfg(cfg, Some("D:/out".into()), Some("txt".into()));
        assert_eq!(new_cfg.download.download_path, "D:/out");
        assert_eq!(new_cfg.download.ext_name, ExportFormat::Txt);
    }

    #[test]
    fn effective_cfg_keeps_originals_when_no_overrides() {
        let cfg = AppConfig {
            download: crate::config::DownloadCfg {
                download_path: "orig".into(),
                ext_name: ExportFormat::Html,
                ..crate::config::DownloadCfg::default()
            },
            ..AppConfig::default()
        };
        let new_cfg = effective_cfg(cfg, None, None);
        assert_eq!(new_cfg.download.download_path, "orig");
        assert_eq!(new_cfg.download.ext_name, ExportFormat::Html);
    }

    #[test]
    fn cli_load_paths_and_config_creates_default_when_missing() {
        // 只验证函数成功返回，不改 HOME（ConfigPaths::discover 走 BaseDirs，不读 env）；
        // 真实端到端测试留给 desktop integration test。
        let (paths, _cfg) = cli_load_paths_and_config().expect("load");
        assert!(
            paths.config_file.ends_with("config.toml"),
            "config_file should end with config.toml: {:?}",
            paths.config_file
        );
        assert!(paths.rules_dir.ends_with("rules"));
        assert!(paths.sources_config.ends_with("sources_config.json"));
    }
}
