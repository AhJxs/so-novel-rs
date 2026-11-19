//! 应用关心的几条文件路径（`config.toml` / themes / rules / 运行数据目录）。

use std::path::PathBuf;

/// `~/.sonovel/` 下应用依赖的各文件/目录路径。
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    /// `config.toml` 路径。
    pub config_file: PathBuf,
    /// 主题目录 `~/.sonovel/themes/`：首次启动写入 21 个 embed 主题，之后 watcher 监听
    /// 该目录并热加载用户手动放入的自定义 *.json。
    pub themes_dir: PathBuf,
    /// 书源规则目录 `~/.sonovel/rules/`：存放书源 JSON 文件。
    pub rules_dir: PathBuf,
    /// 运行数据目录 `~/.sonovel/state/`：书架 / 下载任务 / 书源选择三份数据由
    /// [`crate::db::StateStore`] 统一存这里（SurrealKV），不再拆成独立 JSON。
    pub state_dir: PathBuf,
}

impl ConfigPaths {
    /// 路径约定：所有文件统一在 `~/.sonovel/` 下，首次启动自动创建；
    /// 取不到主目录（极端情况）时回落到当前工作目录。
    #[must_use]
    pub fn discover() -> Self {
        let base = home_dir().join(".sonovel");
        Self {
            config_file: base.join("config.toml"),
            themes_dir: base.join("themes"),
            rules_dir: base.join("rules"),
            state_dir: base.join("state"),
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
