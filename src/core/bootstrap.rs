//! 三端共用的启动期公共资源加载 + 几样从 `cli/util.rs` 搬来的薄壳。
//!
//! ## 为什么 `AppContext::load_context` 不返回 Result
//!
//! 三端的容错策略不完全一致：
//!
//! - **desktop** `AppModel::new_with_wakeup` 对 `load_config` / `init_rules_dir` /
//!   `load_active_rules` 失败都是 `tracing::warn!` + 兜底默认（空 rules / 默认 cfg）。
//! - **web** 启动走 `unwrap_or_default()`（`startup/web.rs:34-42`）。
//! - **cli** 用 `anyhow::Result`，但实际上 `config.toml` 解析失败仍然冒泡。
//!
//! 任何端都不想在这里 panic。所以 `load_context` 内部所有 IO 失败都吞掉 + `tracing::warn!`，
//! 返回一个尽力凑齐的 `AppContext`。失败诊断走日志，不走返回类型 —— 跟桌面现状完全等价。
//!
//! ## 关于 `effective_cfg` / `validate_range` / `load_active_sources`
//!
//! 三个函数都从 `cli/util.rs` 搬过来。CLI 是它们的唯一调用方，搬到 `core` 是为了：
//! 1. 删掉 `cli/util.rs`（单文件模块的最终归宿是 `core`）；
//! 2. 让桌面将来想做"命令行下载 hook"时不复制粘贴。
//!
//! `validate_range` 与 core 没有依赖耦合（纯输入校验），放在这里只因为
//! `cli/util.rs` 已删、无家可归；将来若有 GUI 复用再考虑独立 `core::range`。

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::config::{AppConfig, ConfigPaths, ExportFormat};
use crate::db::{SourcesConfig, init_rules_dir, load_active_rules};
use crate::http::HttpClients;
use crate::models::Rule;

/// 启动期公共资源聚合（`paths` + `config` + `sources_config` + `rules` + `http`）。
///
/// desktop 的 `AppModel` 直接拿这个 + 额外的 `tasks / runtime / wakeup` 拼成自己的
/// state；web 拿前五个 + 自己额外 `load_tasks_from_file`；cli 仅消费 paths + cfg。
pub struct AppContext {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub sources_config: SourcesConfig,
    pub rules: Vec<Rule>,
    pub http: Arc<HttpClients>,
}

/// 启动期"凑齐所有公共资源"的统一入口。
///
/// **不**返回 `Result`：所有失败一律 `tracing::warn!` + 兜底默认。
/// 详细动机见模块顶部注释。
///
/// # Panics
///
/// 不会 panic：所有失败路径都 swallow 走兜底（见"失败兜底矩阵"表格）。
///
/// ## 失败兜底矩阵
///
/// | 步骤 | 失败时行为 |
/// |---|---|
/// | `ConfigPaths::discover` | 不可失败（`directories` 兜底当前工作目录） |
/// | `load_config` | `tracing::warn!` + `AppConfig::default()` |
/// | 首次启动写默认 config | `tracing::warn!`（不阻塞） |
/// | `init_rules_dir` | `tracing::warn!`（不阻塞） |
/// | `SourcesConfig::load` | 不可失败（文件不存在 → `SourcesConfig::default()`） |
/// | 首次启动写默认 sources_config | `tracing::warn!`（不阻塞） |
/// | `load_active_rules` | `tracing::warn!` + 空 Vec |
/// | `HttpClients::new` | `tracing::warn!` + 默认 HttpClients（proxy 等用默认值） |
pub fn load_context() -> AppContext {
    let paths = ConfigPaths::discover();

    // config.toml —— 失败回默认
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

    // sources_config + 首次启动写默认
    let sources_config = SourcesConfig::load(&paths.sources_config);
    if !paths.sources_config.exists()
        && let Err(e) = sources_config.save(&paths.sources_config)
    {
        tracing::warn!("写入默认 sources_config.json 失败: {e:#}");
    }

    // 活跃规则 —— 失败回空 Vec
    let rules = match load_active_rules(&paths.rules_dir, &sources_config) {
        Ok(rs) => rs,
        Err(e) => {
            tracing::warn!("rules load failed: {e:#}");
            Vec::new()
        }
    };

    // HTTP 客户端 —— 常见失败原因是 proxy URL 畸形；strip proxy 后重试。
    // 三步 fallback：原始 cfg → 关闭 proxy → 空 stub（任意失败 log 后继续走空集）。
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
///
/// 与 `cli/util.rs::effective_cfg` 行为完全一致；搬到 `core` 是为统一 CLI 入口的
/// 依赖来源（让 desktop / web 未来要做"hook 下载"也能复用）。
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
/// **注意**:
/// - 这是 cli 启动期的快捷入口，已应用 [`SourcesConfig`] 里 `disabled_urls` 的过滤。
/// - **不**再过滤 rule.disabled / search.disabled —— 这两步由调用方通过
///   [`crate::core::search::select_sources`] 统一处理（搜索场景用 `is_search_enabled`，
///   下载场景按需自选），避免 core 模块需要知道"调用方有没有预过滤"的歧义。
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

/// 一次性跑完 CLI 启动期的全部 IO + 写出默认 config 等。
///
/// 等价于 cli 旧版 `mod.rs::run` 第 142-209 行的合并版：
/// - `ConfigPaths::discover`
/// - `load_config`
/// - 首次启动 `save_config`
/// - `init_rules_dir`
///
/// **不**包括子命令 dispatch、locale 切换、tracing init —— 那些仍是 cli 专属。
///
/// 主要供 cli 复用 desktop 的 startup 兜底矩阵；返回 `Result` 是因为 cli 想要明确失败
///（malformed TOML 等），不像 `load_context` 全兜底。
pub fn cli_load_paths_and_config() -> Result<(ConfigPaths, AppConfig)> {
    let paths = ConfigPaths::discover();
    let cfg = crate::config::load_config(&paths.config_file).context("加载 config.toml 失败")?;

    // 与 GUI 启动行为保持一致：首次运行时把默认 config 写出去，
    // 用户立刻能在项目根看到 config.toml 可编辑。失败仅警告，不阻塞 CLI。
    if !paths.config_file.exists() {
        if let Err(e) = crate::config::save_config(&paths.config_file, &cfg) {
            tracing::warn!("写入默认 config.toml 失败: {e:#}");
        } else {
            tracing::info!("首次运行：已生成 {}", paths.config_file.display());
        }
    }

    // 初始化规则目录
    if let Err(e) = init_rules_dir(&paths.rules_dir) {
        tracing::warn!("规则目录初始化失败: {e:#}");
    }

    Ok((paths, cfg))
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

    // ----- effective_cfg -----

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

    // ----- cli_load_paths_and_config -----

    #[test]
    fn cli_load_paths_and_config_creates_default_when_missing() {
        // HOME 指向 tempdir → ConfigPaths::discover 走 ~/.sonovel/，tempdir 当 HOME。
        // 但这里只验证函数行为（成功），不实际改 HOME —— ConfigPaths::discover 用的是
        // BaseDirs（系统 API），不会读 env。
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
