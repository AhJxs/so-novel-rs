//! `config.toml` 读写。对应 Java `core.AppConfigLoader` + `model.AppConfig`。

mod loader;

pub use loader::{load_config, save_config, AppConfig, ConfigPaths, ExportFormat, LangType, ThemePref};
