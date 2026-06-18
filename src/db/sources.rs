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
use rusqlite::{Connection, params};

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

/// 提取 DB 中所有书源的 `url` 字段（小写比较前 trim），返回集合 —— 给
/// `add_sources_from_file` 做 dedup 用。
///
/// 同一 URL（忽略大小写、首尾空格）只导入一次。空 URL 不计入（让 `Rule::url` 为空、
/// `Rule::name` 也不空的"占位规则"仍能导入）。
///
/// 不带 overlap with `list_with_overrides` 是因为 dedup 只需要 url 字段，没必要反
/// 序列化整个 Rule —— 1000 条规则时省 ~80% 时间 + 内存。
pub fn list_existing_urls(conn: &Connection) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn
        .prepare("SELECT data FROM sources")
        .context("prepare list existing urls")?;
    let mut rows = stmt.query([]).context("query sources for urls")?;
    let mut set = std::collections::HashSet::new();
    while let Some(row) = rows.next()? {
        let data: String = row.get(0)?;
        if let Ok(rule) = serde_json::from_str::<Rule>(&data) {
            let url = rule.url.trim();
            if !url.is_empty() {
                // 标准化：to_lowercase 集中所有大小写变体（http vs HTTP, trailing slash 区分由 url 原文保留）
                set.insert(url.to_lowercase());
            }
        }
    }
    Ok(set)
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

/// 批量追加书源到 `sources` 表。
///
/// - `id` 取当前最大 id + 1 起递增；`ord` 取当前最大 ord + 1 起递增。
/// - 整批包在一个事务里：任意一条插失败回滚整批，避免半成品状态污染列表。
/// - 入参的 `Rule.id` 字段被忽略 — 重新分配，避免与已有源冲突。
///
/// 返回成功插入的条数（= rules.len()，否则会以 Err 抛出）。
pub fn insert_many(conn: &mut Connection, rules: &[Rule]) -> Result<usize> {
    if rules.is_empty() {
        return Ok(0);
    }

    let tx = conn.transaction().context("begin tx")?;

    // 当前最大 id / ord（COUNT 不准 — 若中途删过会留空洞，但新增应延续最大）
    let max_id: i64 = tx
        .query_row("SELECT COALESCE(MAX(id), 0) FROM sources", [], |r| r.get(0))
        .unwrap_or(0);
    let max_ord: i64 = tx
        .query_row("SELECT COALESCE(MAX(ord), -1) FROM sources", [], |r| {
            r.get(0)
        })
        .unwrap_or(-1);

    for (idx, rule) in rules.iter().enumerate() {
        let data = serde_json::to_string(rule).context("序列化新增规则")?;
        let id = max_id + (idx as i64) + 1;
        let ord = max_ord + (idx as i64) + 1;
        tx.execute(
            "INSERT INTO sources (id, ord, data) VALUES (?1, ?2, ?3)",
            params![id, ord, data],
        )
        .with_context(|| format!("插入第 {} 条新规则失败", idx + 1))?;
    }

    tx.commit().context("commit tx")?;
    Ok(rules.len())
}

/// 按 id 删除一条书源；同步删 `source_overrides` 表里同 id 的行。
///
/// 返回是否真的删了行（id 不存在时返回 false 但不报错）。
/// 不重排剩余源的 id —— 让 UI 看到的 id 在整个会话里保持稳定，
/// 避免删一条后所有 id 后移、用户记忆里的"#7"突然指向了别的源。
pub fn delete_one(conn: &mut Connection, source_id: i32) -> Result<bool> {
    let tx = conn.transaction().context("begin tx")?;
    let n = tx
        .execute(
            "DELETE FROM sources WHERE id = ?1",
            params![source_id as i64],
        )
        .context("delete from sources")?;
    // 同时清理 overrides — 不然下次又把这个 id 标成"禁用"会产生孤儿记录。
    tx.execute(
        "DELETE FROM source_overrides WHERE source_id = ?1",
        params![source_id as i64],
    )
    .context("delete from source_overrides")?;
    tx.commit().context("commit tx")?;
    Ok(n > 0)
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

    #[test]
    fn insert_many_appends_with_new_ids_after_seed() {
        let mut db = fresh_db();
        seed_from_default(db.conn()).unwrap();
        let before = list_with_overrides(db.conn()).unwrap();
        let max_before = before.iter().map(|r| r.id).max().unwrap();

        let new_rules = vec![
            Rule {
                url: "https://example.com/a".into(),
                name: "A".into(),
                ..Rule::default()
            },
            Rule {
                url: "https://example.com/b".into(),
                name: "B".into(),
                ..Rule::default()
            },
        ];
        let n = insert_many(db.conn_mut(), &new_rules).unwrap();
        assert_eq!(n, 2);

        let after = list_with_overrides(db.conn()).unwrap();
        assert_eq!(after.len(), before.len() + 2);
        // 新书源的 id 应当 > 之前的最大 id
        let ids_after: Vec<i32> = after.iter().map(|r| r.id).collect();
        assert!(ids_after.contains(&(max_before + 1)));
        assert!(ids_after.contains(&(max_before + 2)));
        // 新增名称按追加顺序在末尾
        assert_eq!(after[after.len() - 2].name, "A");
        assert_eq!(after[after.len() - 1].name, "B");
    }

    #[test]
    fn delete_one_removes_source_and_override() {
        let mut db = fresh_db();
        seed_from_default(db.conn()).unwrap();
        // 先把 id=2 标成禁用 → source_overrides 表留下记录
        set_disabled(db.conn(), 2, true).unwrap();
        assert!(list_disabled(db.conn()).unwrap().contains(&2));

        // 删 id=2
        let removed = delete_one(db.conn_mut(), 2).unwrap();
        assert!(removed);

        // sources 表里没有 id=2 了
        let after = list_with_overrides(db.conn()).unwrap();
        assert!(after.iter().all(|r| r.id != 2));
        // overrides 表也清掉了同 id 的孤儿记录
        assert!(!list_disabled(db.conn()).unwrap().contains(&2));

        // 不存在的 id 删返回 false 但不 error
        let removed = delete_one(db.conn_mut(), 9999).unwrap();
        assert!(!removed);
    }
}
