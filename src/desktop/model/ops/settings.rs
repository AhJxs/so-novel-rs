//! 配置持久化。

use crate::config::AppConfig;
use crate::error::{AppError, AppResult};

/// 把当前 config 写回 config.toml。
///
/// 写盘失败时原样透传底层错误（由调用方决定是否推 notification）。**不要**在这里拼
/// 用户可见的中文前缀：toast 文案归 UI 边界（`t!`），这里只负责把技术细节带上去。
pub fn persist_settings(config: &AppConfig, config_file: &std::path::Path) -> AppResult<()> {
    crate::config::save_config(config_file, config)
        .map_err(|e| AppError::internal(format!("{e:#}")))
}
