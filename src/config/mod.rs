//! `config.toml` 读写。对应 Java `core.AppConfigLoader` + `model.AppConfig`。
//!
//! 注意：`SourcesConfig` 和 `write_atomically` 已迁移到 `crate::persistent` 模块。
//!
//! ## 内部模块拆分
//!
//! - `defaults` — 默认下载路径 / 默认 TOML 模板
//! - `paths`   — `ConfigPaths` + 路径发现
//! - `toml_io` — `load_config` / `save_config` / `t_*` helper
//! - `types`   — enum / `AppConfig` struct / serde derive
//!
//! 本文件作为 **re-export 门面**，是唯一对外暴露的入口（外部仍 `use crate::config::*`）。

mod defaults;
mod paths;
mod toml_io;
mod types;

pub use paths::ConfigPaths;
pub use toml_io::{load_config, save_config};
pub use types::{AppConfig, ExportFormat, LangType, Language, ThemeDynMode, ThemeKind, ThemePref};

#[cfg(test)]
mod tests;
