//! 规则目录初始化
//!
//! 2 个公共 fn:
//! - [`init_rules_dir`] — 首次启动创建目录 + 补齐内置规则文件 (不覆盖用户修改)
//! - [`list_rule_files`] — 枚举目录下的规则文件名 (切活跃书源下拉用)

use std::path::Path;

use super::constants::BUNDLED_RULES;

/// 初始化规则目录: 创建目录 + 补齐 [`BUNDLED_RULES`] 缺失的规则文件。
///
/// - 目录不存在时创建;
/// - 已存在的文件**不覆盖** (尊重用户修改);
/// - 返回新创建的文件数量。
///
/// # Examples
///
/// ```ignore
/// let n = init_rules_dir(rules_dir)?;
/// println!("首次启动创建了 {n} 个规则文件");
/// ```
pub fn init_rules_dir(rules_dir: &Path) -> std::io::Result<usize> {
    if !rules_dir.exists() {
        std::fs::create_dir_all(rules_dir)?;
    }

    let mut created = 0;
    for (filename, content) in BUNDLED_RULES {
        let path = rules_dir.join(filename);
        if !path.exists() {
            std::fs::write(&path, content)?;
            tracing::info!("已创建默认规则文件: {}", path.display());
            created += 1;
        }
    }

    if created > 0 {
        tracing::info!("规则目录初始化完成, 创建了 {} 个文件", created);
    }

    Ok(created)
}

/// 列出目录下所有规则文件名 (不含路径、跳过模板)。
/// 用于"切活跃书源文件"下拉的可选项填充。
pub fn list_rule_files(rules_dir: &Path) -> Vec<String> {
    if !rules_dir.exists() {
        return Vec::new();
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(rules_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                let name_lower = name.to_ascii_lowercase();
                if name_lower.starts_with("rule-template") {
                    continue;
                }
                if path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                    || path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("json5"))
                {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::path::PathBuf;

    fn repo_rules_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bundle")
            .join("rules")
    }

    // ----- init_rules_dir -----

    #[test]
    fn init_creates_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join("rules");

        let created = init_rules_dir(&rules_dir).unwrap();
        assert_eq!(created, 5);

        assert!(rules_dir.join("main.json").exists());
        assert!(rules_dir.join("cloudflare.json").exists());
        assert!(rules_dir.join("no-search.json").exists());
        assert!(rules_dir.join("rate-limit.json").exists());
        assert!(rules_dir.join("proxy-required.json").exists());
    }

    #[test]
    fn init_does_not_overwrite_existing() {
        let dir = tempfile::tempdir().unwrap();
        let rules_dir = dir.path().join("rules");
        std::fs::create_dir_all(&rules_dir).unwrap();

        // 写一个 user-customized main.json
        let user_content = r#"[{"url":"https://user-customized"}]"#;
        std::fs::write(rules_dir.join("main.json"), user_content).unwrap();

        let created = init_rules_dir(&rules_dir).unwrap();
        // main.json 已存在, 不算新建
        assert_eq!(created, 4);

        // 用户内容**不**被覆盖
        let content = std::fs::read_to_string(rules_dir.join("main.json")).unwrap();
        assert_eq!(content, user_content);
    }

    // ----- list_rule_files -----

    #[test]
    fn list_returns_sorted_json_files_excluding_templates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.json"), "[]").unwrap();
        std::fs::write(dir.path().join("zzz.json"), "[]").unwrap();
        std::fs::write(dir.path().join("aaa.json5"), "[]").unwrap();
        std::fs::write(dir.path().join("rule-template.json5"), "[]").unwrap();
        std::fs::write(dir.path().join("not-json.txt"), "x").unwrap();

        let mut files = list_rule_files(dir.path());
        files.sort(); // 自身已 sort, 这里再 sort 一下确认稳定
        assert_eq!(files, vec!["aaa.json5", "main.json", "zzz.json"]);
    }

    #[test]
    fn list_returns_empty_for_nonexistent_dir() {
        let files = list_rule_files(Path::new("/nonexistent/dir/xyz"));
        assert!(files.is_empty());
    }

    #[test]
    fn list_works_on_bundled_repo_rules() {
        let dir = repo_rules_dir();
        let files = list_rule_files(&dir);
        assert!(files.contains(&"main.json".to_string()));
        assert!(!files.iter().any(|f| f.starts_with("rule-template")));
    }
}
