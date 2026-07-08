//! 应用关心的几条文件路径（`config.toml` / themes / rules / `sources_config` / tasks）。

use std::path::PathBuf;

/// 程序启动时关心的几条路径。
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    /// `config.toml` 路径。
    pub config_file: PathBuf,
    /// 主题目录 `~/.sonovel/themes/`：首次启动写入 21 个 embed 主题，
    /// 之后 watcher 监听这个目录，用户可手动放自定义 *.json 进去热加载。
    pub themes_dir: PathBuf,
    /// 书源规则目录 `~/.sonovel/rules/`：存放书源 JSON 文件。
    pub rules_dir: PathBuf,
    /// 书源配置文件 `~/.sonovel/sources_config.json`：管理活跃书源文件和禁用列表。
    pub sources_config: PathBuf,
    /// 下载任务文件 `~/.sonovel/tasks.json`：替代 `SQLite` 管理下载任务。
    pub tasks_file: PathBuf,
}

impl ConfigPaths {
    /// 路径约定：
    /// - 所有文件统一存放在用户主目录下的 `~/.sonovel/`；
    /// - 首次启动时各目录/文件不存在，会自动创建；
    /// - 如果无法获取主目录（极端情况），回落到当前工作目录。
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
