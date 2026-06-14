//! 书源规则的 DB 仓库。
//!
//! 表 schema：
//! ```sql
//! sources(id PK, ord INTEGER, data TEXT)         -- data = JSON(Rule)
//! source_overrides(source_id PK)                 -- 仅记录禁用的 id
//! ```
//!
//! 行为：
//! - 启动时若 `sources` 表为空，自动 `seed_from_default()` 把 `bundle/rules/main.json`
//!   编译期嵌入的内容写进去，这样删 DB 重启就能恢复出厂源；
//! - `list()` 返回带 `disabled` 已合并到 `Rule.disabled` 的 `Vec<Rule>`，调用方拿来直接用；
//! - `toggle_disabled()` 直接改 `source_overrides` 表。

use anyhow::{Context, Result};
use rusqlite::{params, Connection};

use crate::models::Rule;

/// 编译期嵌入的默认规则集（仓库内 `bundle/rules/main.json`），首次启动 seed 用。
/// 不读运行目录下的文件，所以删 DB 重启不依赖部署时是否带了 bundle/。
const DEFAULT_RULES_JSON: &str = include_str!("../../bundle/rules/main.json");

/// 把默认规则 seed 进 `sources` 表（仅当表为空时）。
///
/// 返回插入的行数。已存在数据时返回 0。
pub fn seed_from_default(conn: &Connection) -> Result<usize> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))
        .unwrap_or(0);
    if count > 0 {
        return Ok(0);
    }

    let rules: Vec<Rule> =
        serde_json::from_str(DEFAULT_RULES_JSON).context("解析嵌入的默认 main.json")?;

    let mut inserted = 0;
    for (idx, rule) in rules.iter().enumerate() {
        let data = serde_json::to_string(rule).context("序列化默认规则")?;
        // id 由 ord+1 直接复用：跟之前 load_rules_from_path 的"按出现顺序自增 id"一致。
        let id = (idx + 1) as i64;
        let ord = idx as i64;
        conn.execute(
            "INSERT INTO sources (id, ord, data) VALUES (?1, ?2, ?3)",
            params![id, ord, data],
        )?;
        inserted += 1;
    }
    Ok(inserted)
}

/// 读取所有书源规则，合并 `source_overrides` 里的禁用状态到 `Rule.disabled`。
///
/// 返回的规则按 `ord` 升序，等价于 seed 时的文件顺序。
pub fn list_with_overrides(conn: &Connection) -> Result<Vec<Rule>> {
    let mut stmt = conn
        .prepare("SELECT id, data FROM sources ORDER BY ord ASC, id ASC")
        .context("prepare list sources")?;
    let mut rows = stmt.query([]).context("query sources")?;
    let mut out: Vec<Rule> = Vec::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let data: String = row.get(1)?;
        match serde_json::from_str::<Rule>(&data) {
            Ok(mut r) => {
                r.id = id as i32;
                out.push(r);
            }
            Err(e) => {
                tracing::warn!("sources 行 id={id} 解析失败 ({e})，跳过");
            }
        }
    }

    // 合并 overrides
    let disabled = list_disabled(conn)?;
    for r in out.iter_mut() {
        if disabled.contains(&r.id) {
            r.disabled = true;
        }
    }

    Ok(out)
}

/// 仅返回被用户禁用的 source id 集合。
pub fn list_disabled(conn: &Connection) -> Result<std::collections::HashSet<i32>> {
    let mut stmt = conn
        .prepare("SELECT source_id FROM source_overrides")
        .context("prepare list source_overrides")?;
    let mut rows = stmt.query([])?;
    let mut set = std::collections::HashSet::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        set.insert(id as i32);
    }
    Ok(set)
}

/// 切换禁用状态。返回切换后的"是否禁用"。
pub fn toggle_disabled(conn: &Connection, source_id: i32) -> Result<bool> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM source_overrides WHERE source_id = ?1)",
            params![source_id as i64],
            |r| r.get(0),
        )
        .unwrap_or(false);
    if exists {
        conn.execute(
            "DELETE FROM source_overrides WHERE source_id = ?1",
            params![source_id as i64],
        )?;
        Ok(false)
    } else {
        conn.execute(
            "INSERT OR IGNORE INTO source_overrides (source_id) VALUES (?1)",
            params![source_id as i64],
        )?;
        Ok(true)
    }
}

/// 直接设置（true=禁用 / false=启用），用于一次性迁移老的 source-overrides.json。
pub fn set_disabled(conn: &Connection, source_id: i32, disabled: bool) -> Result<()> {
    if disabled {
        conn.execute(
            "INSERT OR IGNORE INTO source_overrides (source_id) VALUES (?1)",
            params![source_id as i64],
        )?;
    } else {
        conn.execute(
            "DELETE FROM source_overrides WHERE source_id = ?1",
            params![source_id as i64],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn fresh_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn seed_inserts_main_rules_then_idempotent() {
        let db = fresh_db();
        let n = seed_from_default(db.conn()).unwrap();
        assert!(n >= 5, "expected ≥5 seeded rules, got {n}");

        // 第二次调用不再插入
        let n2 = seed_from_default(db.conn()).unwrap();
        assert_eq!(n2, 0);
    }

    #[test]
    fn list_returns_seeded_rules_in_order() {
        let db = fresh_db();
        seed_from_default(db.conn()).unwrap();

        let rules = list_with_overrides(db.conn()).unwrap();
        assert!(!rules.is_empty());
        // ord 列升序 → id 应当连续从 1
        for (idx, r) in rules.iter().enumerate() {
            assert_eq!(r.id, (idx + 1) as i32);
        }
    }

    #[test]
    fn toggle_disabled_round_trip() {
        let db = fresh_db();
        seed_from_default(db.conn()).unwrap();

        let now = toggle_disabled(db.conn(), 1).unwrap();
        assert!(now);
        let rules = list_with_overrides(db.conn()).unwrap();
        assert!(rules[0].disabled);

        let now = toggle_disabled(db.conn(), 1).unwrap();
        assert!(!now);
        let rules = list_with_overrides(db.conn()).unwrap();
        assert!(!rules[0].disabled);
    }
}
