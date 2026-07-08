//! 配置持久化。

use crate::config::{AppConfig, CookieCfg, CrawlCfg, DownloadCfg, GlobalCfg, ProxyCfg, SourceCfg};
use crate::error::AppResult;

/// 把当前 config 写回 config.toml。
///
/// 写盘失败时返回 [`AppError`](crate::error::AppError)（由调用方决定是否推 notification）。
pub fn persist_settings(config: &AppConfig, config_file: &std::path::Path) -> AppResult<()> {
    crate::config::save_config(config_file, config)
        .map_err(|e| crate::error::AppError::internal(format!("保存失败: {e:#}")))
}
