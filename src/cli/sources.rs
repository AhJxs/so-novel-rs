//! `sources` 子命令: list / enable / disable。
//!
//! 启用/禁用读写 `~/.sonovel/sources_config.json` 的 `disabled_urls`, **以 URL 为 key**
//! (ID 在不同书源文件里可能重复)。

use anyhow::{Context, Result};

use crate::config::ConfigPaths;
use crate::core::sources as core_sources;
use crate::db::SourcesConfig;
use crate::models::Rule;

/// 列出当前激活书源（人类可读 / JSON 两种格式）。
pub fn run_list(paths: &ConfigPaths, json: bool) -> Result<()> {
    let sources_config = SourcesConfig::load(&paths.sources_config);
    let rules: Vec<Rule> =
        core_sources::load_active(&paths.rules_dir, &sources_config).context("加载规则失败")?;

    if json {
        // 机器可读：Rule 已 derive(Serialize)。
        println!("{}", serde_json::to_string(&rules)?);
        return Ok(());
    }

    let enabled = rules.iter().filter(|r| !r.disabled).count();
    let disabled = rules.iter().filter(|r| r.disabled).count();
    println!(
        "书源文件: {}（启用 {} / 禁用 {}）",
        paths.rules_dir.join(&sources_config.active_file).display(),
        enabled,
        disabled
    );
    println!();
    for r in &rules {
        let mark = if r.disabled { "✗" } else { "✓" };
        let proxy = if r.need_proxy { " [proxy]" } else { "" };
        let lang = if r.language.is_empty() {
            String::new()
        } else {
            format!(" [{}]", r.language)
        };
        let search = if r.search.as_ref().is_some_and(|s| !s.disabled) {
            " [search]"
        } else {
            ""
        };
        println!(
            "  {mark} #{:>3} {}{}{}{}  {}",
            r.id, r.name, proxy, lang, search, r.url
        );
    }
    Ok(())
}

/// 设置指定 ID 书源的禁用状态: 把规则 URL 加入 / 移出 `disabled_urls` 并写回磁盘。
///
/// 找不到 ID → 错误; 已是目标状态 → 幂等早退, 不写盘。
pub fn run_set_disabled(paths: &ConfigPaths, id: i32, disable: bool) -> Result<()> {
    let mut sources_config = SourcesConfig::load(&paths.sources_config);
    let rules: Vec<Rule> =
        core_sources::load_active(&paths.rules_dir, &sources_config).context("加载规则失败")?;

    // 用 ID 找 URL（ID 是 sources_config.active_file 文件内的局部编号，不跨文件）。
    let rule = core_sources::find_rule_by_id(&rules, id)
        .with_context(|| format!("找不到 ID={id} 的书源"))?;
    let url = rule.url.clone();
    let name = &rule.name;

    // URL 键归一必须走 `core_sources::disabled_url_key`, 与 `SourcesConfig::toggle_disabled`
    // 同源 —— 否则两边键不一致, 禁用状态读不回来。
    let key = core_sources::disabled_url_key(&url);
    let already = sources_config.disabled_urls.contains(&key);
    let state = if disable { "已禁用" } else { "已启用" };

    // 幂等：已是目标状态 → 早退，不写盘。
    if already == disable {
        println!("{state} #{id} {name}（{url}）");
        return Ok(());
    }

    if disable {
        sources_config.disabled_urls.insert(key);
    } else {
        sources_config.disabled_urls.remove(&key);
    }
    sources_config
        .save(&paths.sources_config)
        .context("写回 sources_config.json 失败")?;

    println!("✓ {state} #{id} {name}（{url}）");
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::config::ConfigPaths;

    /// 临时目录 + minimal rules.json (2 条规则) + `ConfigPaths`; 跳过会读用户主目录的
    /// `ConfigPaths::discover()`, 直接拼路径。
    fn setup_two_sources() -> (tempfile::TempDir, ConfigPaths) {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).unwrap();
        let rules = serde_json::json!([
            {
                "id": 1,
                "name": "A",
                "url": "https://a.test/",
                "language": "zh",
            },
            {
                "id": 2,
                "name": "B",
                "url": "https://b.test/",
                "language": "zh",
            }
        ]);
        std::fs::write(rules_dir.join("main.json"), rules.to_string()).unwrap();

        let paths = ConfigPaths {
            config_file: dir.path().join("config.toml"),
            themes_dir: dir.path().join("themes"),
            rules_dir,
            sources_config: dir.path().join("sources_config.json"),
            tasks_file: dir.path().join("tasks.json"),
        };
        (dir, paths)
    }

    fn rule_count(paths: &ConfigPaths, disabled: bool) -> usize {
        let sources_config = SourcesConfig::load(&paths.sources_config);
        let rules = core_sources::load_active(&paths.rules_dir, &sources_config).unwrap();
        rules.iter().filter(|r| r.disabled == disabled).count()
    }

    #[test]
    fn disable_then_enable_round_trip() {
        let (_dir, paths) = setup_two_sources();

        assert_eq!(rule_count(&paths, true), 0);
        assert_eq!(rule_count(&paths, false), 2);

        run_set_disabled(&paths, 1, true).unwrap();
        assert_eq!(rule_count(&paths, true), 1);
        assert_eq!(rule_count(&paths, false), 1);

        // 重新加载 ConfigPaths 后状态依然保留（确认写盘生效）
        assert!(
            SourcesConfig::load(&paths.sources_config)
                .disabled_urls
                .contains("https://a.test/")
        );

        run_set_disabled(&paths, 1, false).unwrap();
        assert_eq!(rule_count(&paths, true), 0);
        assert_eq!(rule_count(&paths, false), 2);
        assert!(
            !SourcesConfig::load(&paths.sources_config)
                .disabled_urls
                .contains("https://a.test/")
        );
    }

    #[test]
    fn disable_unknown_id_errors() {
        let (_dir, paths) = setup_two_sources();
        let result = run_set_disabled(&paths, 999, true);
        assert!(result.is_err(), "未知 ID 应报错");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("999"), "错误信息应包含 ID: {msg}");
    }

    #[test]
    fn disable_already_disabled_is_idempotent() {
        let (_dir, paths) = setup_two_sources();
        run_set_disabled(&paths, 1, true).unwrap();
        // 重复禁用：不应再写盘，但仍返回 Ok
        run_set_disabled(&paths, 1, true).unwrap();
        assert_eq!(
            SourcesConfig::load(&paths.sources_config)
                .disabled_urls
                .len(),
            1
        );
    }

    #[test]
    fn list_json_outputs_rules() {
        let (_dir, paths) = setup_two_sources();
        // json 输出难以在进程内断言, 这里只验证非 json 路径不报错。
        run_list(&paths, false).unwrap();
    }
}
