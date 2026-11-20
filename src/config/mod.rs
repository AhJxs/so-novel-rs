//! `config.toml` 读写门面。
//!
//! 子模块：`defaults`（默认下载路径 / TOML 模板）、`paths`（`ConfigPaths`）、
//! `toml_io`（`load_config` / `save_config`，含字号夹取与校验）、
//! `types`（enum / `AppConfig` / `ConfigError` / 全局字号范围常量）。
//! 本文件只做 re-export，是对外唯一入口（外部仍 `use crate::config::*`）。
//!
//! 配置由 `core::bootstrap::load_context` 加载一次后放进 `AppContext`，各模块从那里读，
//! 避免重复加载 / 解析 / 路径漂移。`SourcesConfig` / `write_atomically` 已迁到 `crate::db`。

mod defaults;
mod paths;
mod toml_io;
mod types;

pub use paths::ConfigPaths;
pub use toml_io::{load_config, save_config};
pub use types::{
    AppConfig, ConfigError, CookieConfig, CrawlConfig, DownloadConfig, ExportFormat, FONT_SIZE_MAX,
    FONT_SIZE_MIN, GlobalConfig, LangType, Language, ParseConfigValueError, ProxyConfig, ProxyMode,
    ReaderConfig, ReaderTheme, ReadingMode, SourceConfig, ThemeDynMode, ThemeKind, ThemePref,
};

#[cfg(test)]
mod tests;
