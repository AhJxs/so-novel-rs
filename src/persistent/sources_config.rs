//! 书源配置管理：活跃书源文件选择 + 禁用书源列表。
//!
//! 配置文件路径：`~/.sonovel/sources_config.json`

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::write_atomically;

/// 书源配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcesConfig {
    /// 当前选中的书源 JSON 文件名（相对于 `~/.sonovel/rules/`）。
    pub active_file: String,
    /// 被禁用的书源 URL 列表（用 URL 而非 ID，因为 ID 在不同文件中不同）。
    #[serde(default)]
    pub disabled_urls: HashSet<String>,
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            active_file: "main.json".to_string(),
            disabled_urls: HashSet::new(),
        }
    }
}

impl SourcesConfig {
    /// 从 JSON 文件加载。文件不存在时返回默认值。
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                tracing::warn!("sources_config.json 解析失败，使用默认值: {e}");
                Self::default()
            }),
            Err(e) => {
                tracing::warn!("sources_config.json 读取失败，使用默认值: {e}");
                Self::default()
            }
        }
    }

    /// 保存到 JSON 文件（原子写入）。
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        write_atomically(path, content.as_bytes())?;
        Ok(())
    }

    /// 切换书源禁用状态。返回切换后的"是否禁用"。
    pub fn toggle_disabled(&mut self, url: &str) -> bool {
        let key = url.trim().to_lowercase();
        if key.is_empty() {
            return false;
        }
        if self.disabled_urls.contains(&key) {
            self.disabled_urls.remove(&key);
            false
        } else {
            self.disabled_urls.insert(key);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：检查书源是否被禁用。
    fn is_disabled(cfg: &SourcesConfig, url: &str) -> bool {
        let key = url.trim().to_lowercase();
        cfg.disabled_urls.contains(&key)
    }

    #[test]
    fn default_config() {
        let cfg = SourcesConfig::default();
        assert_eq!(cfg.active_file, "main.json");
        assert!(cfg.disabled_urls.is_empty());
    }

    #[test]
    fn toggle_disabled_round_trip() {
        let mut cfg = SourcesConfig::default();
        let url = "https://example.com";

        assert!(!is_disabled(&cfg, url));
        let now_disabled = cfg.toggle_disabled(url);
        assert!(now_disabled);
        assert!(is_disabled(&cfg, url));

        let now_disabled = cfg.toggle_disabled(url);
        assert!(!now_disabled);
        assert!(!is_disabled(&cfg, url));
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sources_config.json");

        let mut cfg = SourcesConfig {
            active_file: "cloudflare.json".to_string(),
            ..Default::default()
        };
        cfg.disabled_urls.insert("https://a.com".to_string());
        cfg.disabled_urls.insert("https://b.com".to_string());

        cfg.save(&path).unwrap();
        let loaded = SourcesConfig::load(&path);

        assert_eq!(loaded.active_file, "cloudflare.json");
        assert_eq!(loaded.disabled_urls.len(), 2);
        assert!(loaded.disabled_urls.contains("https://a.com"));
        assert!(loaded.disabled_urls.contains("https://b.com"));
    }

    #[test]
    fn load_missing_file_returns_default() {
        let cfg = SourcesConfig::load(Path::new("/definitely/does/not/exist.json"));
        assert_eq!(cfg.active_file, "main.json");
        assert!(cfg.disabled_urls.is_empty());
    }
}
