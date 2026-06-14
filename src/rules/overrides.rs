//! 用户对书源的"启用/禁用"覆写。
//!
//! 阶段二之前用独立 `source-overrides.json` sidecar 文件存；阶段二起改为 SQLite
//! 表 `source_overrides`，跟下载任务同库（`sonovel.db`）。本模块作为薄包装暴露
//! 给 UI / CLI，让调用方不用直接和 `rusqlite::Connection` 打交道。

use std::collections::HashSet;

use crate::models::Rule;

/// 用户书源覆写。当前只有"禁用"语义；扩展时可加 `enabled` 白名单等。
#[derive(Debug, Clone, Default)]
pub struct SourceOverrides {
    /// 被用户显式禁用的书源 ID 集合。
    pub disabled: HashSet<i32>,
}

impl SourceOverrides {
    /// 从 SQLite `source_overrides` 表加载。失败返回空集合 + warn，不阻塞启动。
    pub fn load_from_db(conn: &rusqlite::Connection) -> Self {
        match crate::db::sources::list_disabled(conn) {
            Ok(disabled) => Self { disabled },
            Err(e) => {
                tracing::warn!("source_overrides 表加载失败，使用默认（空）: {e:#}");
                Self::default()
            }
        }
    }

    /// 应用到一组规则上：把覆写的 `disabled` 字段覆盖到 rule 上。
    /// 注意只覆盖"禁用"方向：上游规则原本 disabled=true 但不在 overrides 里也仍然保持禁用。
    ///
    /// 一般场景下不需要手动调 — `db::sources::list_with_overrides` 已经合并好。
    /// 留这个方法主要给单元测试 / mock 数据用。
    pub fn apply_to_rules(&self, rules: &mut [Rule]) {
        for r in rules.iter_mut() {
            if self.disabled.contains(&r.id) {
                r.disabled = true;
            }
        }
    }

    pub fn is_disabled(&self, source_id: i32) -> bool {
        self.disabled.contains(&source_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::models::Rule;

    #[test]
    fn load_from_empty_db_returns_empty() {
        let db = Db::open_in_memory().unwrap();
        let o = SourceOverrides::load_from_db(db.conn());
        assert!(o.disabled.is_empty());
    }

    #[test]
    fn round_trip_through_db() {
        let db = Db::open_in_memory().unwrap();
        crate::db::sources::set_disabled(db.conn(), 3, true).unwrap();
        crate::db::sources::set_disabled(db.conn(), 7, true).unwrap();

        let loaded = SourceOverrides::load_from_db(db.conn());
        assert!(loaded.is_disabled(3));
        assert!(loaded.is_disabled(7));
        assert!(!loaded.is_disabled(99));
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
