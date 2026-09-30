//! 应用关心的几条文件路径（`config.toml` / themes / rules / `sources_config` / tasks）。

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ConfigPaths {
    /// `config.toml` 路径。
    pub config_file: PathBuf,
    /// 主题目录 `~/.sonovel/themes/`：首次启动写入 21 个 embed 主题，之后 watcher 监听
    /// 该目录并热加载用户手动放入的自定义 *.json。
    pub themes_dir: PathBuf,
    /// 书源规则目录 `~/.sonovel/rules/`：存放书源 JSON 文件。
    pub rules_dir: PathBuf,
    /// 书源配置文件 `~/.sonovel/sources_config.json`：管理活跃书源文件和禁用列表。
    pub sources_config: PathBuf,
    /// 下载任务文件 `~/.sonovel/tasks.json`：替代 `SQLite` 管理下载任务。
    pub tasks_file: PathBuf,
}

impl ConfigPaths {
    /// 路径约定：所有文件统一在 `~/.sonovel/` 下，首次启动自动创建；
    /// 取不到主目录（极端情况）时回落到当前工作目录。
    pub fn discover() -> Self {
        let base = home_dir().join(".sonovel");
        Self {
            config_file: base.join("config.toml"),
            themes_dir: base.join("themes"),
            rules_dir: base.join("rules"),
            sources_config: base.join("sources_config.json"),
            tasks_file: base.join("tasks.json"),
        }
    }
}

/// 获取用户主目录，回落到当前工作目录。
pub fn home_dir() -> PathBuf {
    directories::BaseDirs::new().map_or_else(
        || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        |d| d.home_dir().to_path_buf(),
    )
}
