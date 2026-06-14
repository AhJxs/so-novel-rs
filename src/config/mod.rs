//! `config.ini` 兼容读写。对应 Java `core.AppConfigLoader` + `model.AppConfig`。

mod loader;

pub use loader::{load_config, save_config, AppConfig, ConfigPaths, ExportFormat, LangType};
