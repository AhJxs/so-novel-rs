//! `config.toml` 读写。对应 Java `core.AppConfigLoader` + `model.AppConfig`。
//!
//! 注意：`SourcesConfig` 和 `write_atomically` 已迁移到 `crate::db` 模块。
//!
//! ## 内部模块拆分
//!
//! - `defaults` — 默认下载路径 / 默认 TOML 模板
//! - `paths`   — `ConfigPaths` + 路径发现
//! - `toml_io` — `load_config` / `save_config` / `t_*` helper
//! - `types`   — enum / `AppConfig` struct / sub-structs / serde derive / `ConfigError`
//!
//! 本文件作为 **re-export 门面**，是唯一对外暴露的入口（外部仍 `use crate::config::*`）。
//!
//! ## 全局单例 (PR #6, 2026-07-08)
//!
//! 启动时通过 [`set_global`] 注入 `AppConfig`, 之后所有模块通过 [`global`]
//! 读, 避免重复加载 / 重复解析 / 路径漂移。单例由 `std::sync::LazyLock` 保护,
//! 首次读时 `OnceLock` 检查 + `Result` 传播; 设值是 `OnceLock::set`, 只能调一次。

mod defaults;
mod paths;
mod toml_io;
mod types;

pub use paths::ConfigPaths;
pub use toml_io::{load_config, save_config};
pub use types::{
    AppConfig, ConfigError, CookieCfg, CrawlCfg, DownloadCfg, GlobalCfg, ProxyCfg, SourceCfg,
    ExportFormat, LangType, Language, ThemeDynMode, ThemeKind, ThemePref,
};

use std::sync::{LazyLock, OnceLock};

/// 全局配置单例容器。仅在 `OnceLock` 内保存, 第一次读时才初始化。
static GLOBAL: OnceLock<AppConfig> = OnceLock::new();

/// 全局 lazy 读取视图。如果 `set_global` 还没调过, 返回 `with_defaults()` 兜底。
///
/// 业务代码用法: `crate::config::global().global.font_size`。
///
/// **警告**: 这只是个 **读视图**, 不要尝试通过这里修改 `AppConfig` —
/// 改全局 mutable 配置是反模式, 应该走 `save_config()` + 重启模式。
static GLOBAL_VIEW: LazyLock<&'static AppConfig> = LazyLock::new(|| {
    GLOBAL.get_or_init(|| AppConfig::with_defaults())
});

/// 注入全局配置。仅在启动期 (`main` / `startup` 模块) 调一次。
///
/// 重复调用会返回 `Err`, 由调用方决定如何处理 (panic / warn-and-ignore)。
pub fn set_global(cfg: AppConfig) -> Result<(), &'static str> {
    GLOBAL.set(cfg).map_err(|_| "AppConfig 全局已初始化, 重复 set_global")
}

/// 获取全局配置。第一次读时若未显式 [`set_global`], 用 `with_defaults()` 兜底。
pub fn global() -> &'static AppConfig {
    &GLOBAL_VIEW
}

/// 校验全局配置。启动期 `set_global` 后调一次, 失败让用户改 config.toml 重启。
pub fn validate_global() -> Result<(), ConfigError> {
    global().validate()
}

#[cfg(test)]
mod tests;
