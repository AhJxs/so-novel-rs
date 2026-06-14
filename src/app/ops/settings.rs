//! 配置持久化。

/// 把当前 config 写回 config.toml。
///
/// 写盘失败时返回错误信息（toast 由调用方在能访问 self.toast 的地方处理）。
pub fn persist_settings(
    config: &crate::config::AppConfig,
    config_file: &std::path::Path,
) -> Result<(), String> {
    crate::config::save_config(config_file, config).map_err(|e| format!("保存失败: {e}"))
}
