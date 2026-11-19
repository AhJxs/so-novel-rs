//! 书源配置管理: 活跃书源文件选择 + 禁用书源列表。
//!
//! blob 由 [`crate::db::StateStore`] 存在 `~/.sonovel/state`，这里只管字节。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// 书源选择状态，序列化后由 `StateStore` 存在 `~/.sonovel/state`。
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
    /// 从 JSON 字节解析；损坏时记日志并回落默认值（书源选择不值得为它挡住启动）。
    #[must_use]
    pub fn from_json(bytes: &[u8]) -> Self {
        serde_json::from_slice(bytes).unwrap_or_else(|e| {
            tracing::warn!("书源选择数据解析失败，使用默认值: {e}");
            Self::default()
        })
    }

    /// 序列化成待写入的 JSON 字节。
    ///
    /// # Errors
    ///
    /// JSON 序列化失败时返回错误。
    pub fn to_json(&self) -> anyhow::Result<Vec<u8>> {
        Ok(serde_json::to_vec_pretty(self)?)
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
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

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
    fn json_round_trip() {
        let mut cfg = SourcesConfig {
            active_file: "cloudflare.json".to_string(),
            ..Default::default()
        };
        cfg.disabled_urls.insert("https://a.com".to_string());
        cfg.disabled_urls.insert("https://b.com".to_string());

        let bytes = cfg.to_json().unwrap();
        let loaded = SourcesConfig::from_json(&bytes);
        assert_eq!(loaded.active_file, "cloudflare.json");
        assert_eq!(loaded.disabled_urls.len(), 2);
        assert!(loaded.disabled_urls.contains("https://a.com"));
        assert!(loaded.disabled_urls.contains("https://b.com"));
    }

    #[test]
    fn malformed_json_falls_back_to_default() {
        let cfg = SourcesConfig::from_json(b"{ not json");
        assert_eq!(cfg.active_file, "main.json");
        assert!(cfg.disabled_urls.is_empty());
    }
}
