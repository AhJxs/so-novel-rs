//! desktop 启动期的公共资源加载。
//!
//! `AppContext::load_context` **不**返回 `Result`：内部所有 IO 失败都吞掉 + warn，返回
//! 尽力凑齐的 `AppContext`（desktop 各处自行 `tracing::warn!` + 兜底默认，不在这里 panic）。

use std::sync::Arc;

use crate::config::{AppConfig, ConfigPaths};
use crate::db::{SourcesConfig, init_rules_dir, load_active_rules};
use crate::http::HttpClients;
use crate::models::Rule;

/// 启动期公共资源聚合（`paths` + `config` + `sources_config` + `rules` + `http`）。
/// desktop 拿它加 `tasks / runtime / wakeup` 拼 state。
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
