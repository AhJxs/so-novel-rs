//! `config.toml` 读写门面。
//!
//! 子模块：`defaults`（默认下载路径 / TOML 模板）、`global`（全局配置单例 / 读视图）、
//! `paths`（`ConfigPaths`）、`toml_io`（`load_config` / `save_config`）、
//! `types`（enum / `AppConfig` / `ConfigError`）。
//! 本文件只做 re-export，是对外唯一入口（外部仍 `use crate::config::*`）。
//!
//! 启动时用 [`set_global`] 注入 `AppConfig`，之后所有模块通过 [`global`] 读，避免重复
//! 加载 / 解析 / 路径漂移。`SourcesConfig` / `write_atomically` 已迁到 `crate::db`。

mod defaults;
mod global;
mod paths;
mod toml_io;
mod types;

pub use global::{global, set_global, validate_global};
pub use paths::ConfigPaths;
pub use toml_io::{load_config, save_config};
pub use types::{
    AppConfig, ConfigError, CookieConfig, CrawlConfig, DownloadConfig, ExportFormat, GlobalConfig,
    LangType, Language, ProxyConfig, ProxyMode, SourceConfig, ThemeDynMode, ThemeKind, ThemePref,
};

#[cfg(test)]
mod tests;
