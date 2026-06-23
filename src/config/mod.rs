//! `config.toml` 读写。对应 Java `core.AppConfigLoader` + `model.AppConfig`。
//!
//! 注意：`SourcesConfig` 和 `write_atomically` 已迁移到 `crate::persistent` 模块。

mod loader;

pub use loader::{
    AppConfig, ConfigPaths, ExportFormat, LangType, Language, ThemeDynMode, ThemeKind, ThemePref,
    load_config, save_config,
};
