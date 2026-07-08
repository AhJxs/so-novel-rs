//! 配置持久化。

use crate::config::{AppConfig, CookieCfg, CrawlCfg, DownloadCfg, GlobalCfg, ProxyCfg, SourceCfg};

/// 把当前 config 写回 config.toml。
///
/// 写盘失败时返回错误信息（由调用方决定是否推 notification）。
pub fn persist_settings(config: &AppConfig, config_file: &std::path::Path) -> Result<(), String> {
    crate::config::save_config(config_file, config).map_err(|e| format!("保存失败: {e}"))
}
