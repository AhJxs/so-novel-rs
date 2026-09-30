//! `config.toml` 读写门面。
//!
//! 子模块：`defaults`（默认下载路径 / TOML 模板）、`paths`（`ConfigPaths`）、
//! `toml_io`（`load_config` / `save_config`）、`types`（enum / `AppConfig` / `ConfigError`）。
//! 本文件只做 re-export，是对外唯一入口（外部仍 `use crate::config::*`）。
//!
//! 启动时用 [`set_global`] 注入 `AppConfig`，之后所有模块通过 [`global`] 读，避免重复
//! 加载 / 解析 / 路径漂移。`SourcesConfig` / `write_atomically` 已迁到 `crate::db`。

mod defaults;
mod paths;
mod toml_io;
mod types;

pub use paths::ConfigPaths;
pub use toml_io::{load_config, save_config};
pub use types::{
    AppConfig, ConfigError, CookieCfg, CrawlCfg, DownloadCfg, ExportFormat, GlobalCfg, LangType,
    Language, ProxyCfg, SourceCfg, ThemeDynMode, ThemeKind, ThemePref,
};

use std::sync::{LazyLock, OnceLock};

static GLOBAL: OnceLock<AppConfig> = OnceLock::new();

/// 全局 lazy 读取视图：`set_global` 还没调过时用 `with_defaults()` 兜底，
/// 业务代码用法 `crate::config::global().global.font_size`。
///
/// **警告**：这只是个**读视图**，不要借它改 `AppConfig` —— 改全局配置是反模式，
/// 应该走 `save_config()` + 重启。
static GLOBAL_VIEW: LazyLock<&'static AppConfig> =
    LazyLock::new(|| GLOBAL.get_or_init(AppConfig::with_defaults));

/// 注入全局配置。仅在启动期（`main` / `startup` 模块）调一次；重复调用返回 `Err`，
/// 由调用方决定如何处理（panic / warn-and-ignore）。
pub fn set_global(cfg: AppConfig) -> Result<(), &'static str> {
    GLOBAL
        .set(cfg)
        .map_err(|_| "AppConfig 全局已初始化, 重复 set_global")
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
