//! 用户 themes 目录同步: 创建 / 补缺失 / 不覆盖。
//!
//! 见 [`ensure_user_themes_dir`] 同步规则。

use std::path::Path;

use super::embedded::embedded_themes;

/// 把 embed 主题同步到用户 themes 目录。
///
/// 同步规则:
/// - **目录不存在** → 创建 + 写全部 21 个 embed 主题
/// - **目录已存在** → 只补缺失的 (app 升级新增主题时自动加进来)
/// - **不覆盖任何已存在文件** —— 用户可能改过、或全是自定义主题
///
/// # Errors
///
/// - `std::io::Error` — 创建目录 / 写入文件失败
pub(super) fn ensure_user_themes_dir(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        let mut added = 0usize;
        for (name, content) in embedded_themes() {
            let target = path.join(name);
            if !target.exists() {
                std::fs::write(&target, content)?;
                added += 1;
            }
        }
        if added > 0 {
            tracing::info!("added {} new themes to existing {:?}", added, path);
        }
    } else {
        std::fs::create_dir_all(path)?;
        for (name, content) in embedded_themes() {
            std::fs::write(path.join(name), content)?;
        }
        tracing::info!(
            "created themes dir at {:?} with {} embedded themes",
            path,
            embedded_themes().len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    /// 首次调用 → 创建目录 + 写入 21 个 embed 主题。
    #[test]
    fn ensure_user_themes_dir_creates_when_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("themes");
        assert!(!path.exists(), "precondition: dir should not exist");

        ensure_user_themes_dir(&path).expect("first call");

        assert!(path.is_dir(), "should create dir");
        let count = std::fs::read_dir(&path)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
            .count();
        assert_eq!(count, 21, "should write all 21 embedded themes");

        // 每个文件都能 parse 回合法 JSON。
        for entry in std::fs::read_dir(&path).unwrap().flatten() {
            let p = entry.path();
            let s = std::fs::read_to_string(&p).expect("read back");
            let _: serde_json::Value =
                serde_json::from_str(&s).unwrap_or_else(|e| panic!("bad json {p:?}: {e}"));
        }
    }

    /// 后续调用 → 已存在文件**不覆盖** (保留用户修改)。
    #[test]
    fn ensure_user_themes_dir_preserves_user_modifications() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("themes");
        ensure_user_themes_dir(&path).expect("first call");

        // 用户改了 adventure.json
        let modified = path.join("adventure.json");
        let custom_payload = r#"{"themes":[{"name":"my-custom","mode":"light"}]}"#;
        std::fs::write(&modified, custom_payload).expect("user modification");

        // 第二次调用不应覆盖
        ensure_user_themes_dir(&path).expect("second call");
        let content = std::fs::read_to_string(&modified).expect("read back");
        assert_eq!(
            content, custom_payload,
            "user-modified file should NOT be overwritten"
        );
    }

    /// 后续调用 → 缺失文件被补齐 (模拟 app 升级新增主题)。
    #[test]
    fn ensure_user_themes_dir_adds_missing_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("themes");
        ensure_user_themes_dir(&path).expect("first call");

        // 用户删了一个 embed 主题
        let removed = path.join("adventure.json");
        std::fs::remove_file(&removed).expect("delete");

        // 第二次调用应补回来
        ensure_user_themes_dir(&path).expect("second call");
        assert!(
            removed.exists(),
            "missing embedded theme should be re-added"
        );
        let content = std::fs::read_to_string(&removed).expect("read back");
        assert_eq!(
            content,
            super::super::embedded::THEME_ADVENTURE,
            "should match embedded content"
        );
    }
}
