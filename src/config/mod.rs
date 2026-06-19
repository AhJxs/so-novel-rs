//! `config.toml` 读写。对应 Java `core.AppConfigLoader` + `model.AppConfig`。

mod loader;

pub use loader::{
    AppConfig, ConfigPaths, ExportFormat, LangType, Language, ThemeDynMode, ThemeKind, ThemePref,
    load_config, save_config,
};
