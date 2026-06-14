//! 用户对书源的"启用/禁用"覆写，独立 sidecar JSON 文件持久化。
//!
//! 设计：
//! - 不直接编辑 `bundle/rules/*.json` — 那是上游（freeok）的规则文件；
//! - 用一个 `<config>/source-overrides.json` 记录用户的偏好；
//! - 加载规则后调用 `apply_to_rules` 把 disabled 字段覆盖；
//! - UI 端 toggle 时立即调用 `save` 持久化。

use std::collections::HashSet;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::models::Rule;

/// 用户书源覆写。当前只有"禁用"语义；扩展时可加 `enabled` 白名单等。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceOverrides {
    /// 被用户显式禁用的书源 ID 集合。
    #[serde(default)]
    pub disabled: HashSet<i32>,
}

impl SourceOverrides {
    /// 从 sidecar 文件加载；文件不存在 / 解析失败时返回 default + warn。
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(path)
            .with_context(|| format!("read {}", path.display()))
            .and_then(|s| {
                serde_json::from_str::<SourceOverrides>(&s)
                    .with_context(|| format!("parse {}", path.display()))
            }) {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!("source-overrides 加载失败，使用默认（空）: {e:#}");
                Self::default()
            }
        }
    }

    /// 写回 sidecar 文件。父目录会自动创建。
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let json = serde_json::to_string_pretty(self).context("serialize overrides")?;
        std::fs::write(path, json).with_context(|| format!("write {}", path.display()))?;
        Ok(())
    }

    /// 应用到一组规则上：把覆写的 `disabled` 字段覆盖到 rule 上。
    /// 注意我们**只覆盖"禁用"方向**：如果某规则原本 disabled=true 但不在 overrides 里，
    /// 仍然保持禁用状态（上游规则文件可能有 disabled 字段）。
    pub fn apply_to_rules(&self, rules: &mut [Rule]) {
        for r in rules.iter_mut() {
            if self.disabled.contains(&r.id) {
                r.disabled = true;
            }
        }
    }

    pub fn toggle(&mut self, source_id: i32) -> bool {
        if self.disabled.contains(&source_id) {
            self.disabled.remove(&source_id);
            false
        } else {
            self.disabled.insert(source_id);
            true
        }
    }

    pub fn is_disabled(&self, source_id: i32) -> bool {
        self.disabled.contains(&source_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Rule;

    #[test]
    fn round_trip_through_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source-overrides.json");

        let mut o = SourceOverrides::default();
        o.disabled.insert(3);
        o.disabled.insert(7);
        o.save(&path).unwrap();

        let loaded = SourceOverrides::load(&path);
        assert!(loaded.is_disabled(3));
        assert!(loaded.is_disabled(7));
        assert!(!loaded.is_disabled(99));
    }

    #[test]
    fn missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope.json");
        let o = SourceOverrides::load(&p);
        assert!(o.disabled.is_empty());
    }

    #[test]
    fn corrupt_file_returns_default_with_warn() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bad.json");
        std::fs::write(&p, "{not json").unwrap();
        // 不崩
        let o = SourceOverrides::load(&p);
        assert!(o.disabled.is_empty());
    }

    #[test]
    fn toggle_flips_state() {
        let mut o = SourceOverrides::default();
        assert!(!o.is_disabled(5));
        let now = o.toggle(5);
        assert!(now);
        assert!(o.is_disabled(5));
        let now2 = o.toggle(5);
        assert!(!now2);
        assert!(!o.is_disabled(5));
    }

    #[test]
    fn apply_to_rules_disables_listed_ids_only() {
        let mut rules = vec![
            Rule {
                id: 1,
                disabled: false,
                ..Rule::default()
            },
            Rule {
                id: 2,
                disabled: false,
                ..Rule::default()
            },
            Rule {
                id: 3,
                disabled: true, // 上游已禁，无 override 时保持
                ..Rule::default()
            },
        ];
        let mut o = SourceOverrides::default();
        o.disabled.insert(1);
        o.apply_to_rules(&mut rules);

        assert!(rules[0].disabled, "id=1 应被 override 禁用");
        assert!(!rules[1].disabled, "id=2 不在 override，保持启用");
        assert!(rules[2].disabled, "id=3 上游禁用，不在 override 也保持禁用");
    }
}
