//! `AppConfig` 的"空字符串视作 None"和"路径校验"helper。
//!
//! 三端（cli / web / desktop）在调用 `cfg.global.cf_bypass`、`cfg.cookie.qidian_cookie` 时，
//! 都用同一套"trim 后空 → None"语义；这个模块把语义集中在一处，避免每处 handler 各写
//! `if x.trim().is_empty() { None } else { Some(x.clone()) }` 散落到代码库里。
//!
//! `validate_download_path` 同样从 `web/handlers/settings.rs:132-140` 抽出：
//! 桌面设置面板和 CLI `--download-path` 参数迟早也会需要同样的校验。

use crate::config::AppConfig;

/// `AppConfig.global.cf_bypass` 的 "空串视作 None" 包装。
///
/// - 字符串 trim 后为空 → `None`（**不**走 bypass）
/// - 非空 → `Some(clone)`（直接传给 crawler opts）
///
/// 三端调用方一致，避免各自写 `if x.trim().is_empty() { None } else { Some(x.clone()) }`。
///
/// 返回 `Option<String>` 而非 `Option<&str>`：调用方需要 `'static` 生命周期放进
/// `CrawlerOpts::cf_bypass`，clone 在所难免。
pub fn cf_bypass(cfg: &AppConfig) -> Option<String> {
    let trimmed = cfg.global.cf_bypass.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(cfg.global.cf_bypass.clone())
    }
}

/// `AppConfig.cookie.qidian_cookie` 的 "空串视作 None" 包装。
///
/// 同 [`cf_bypass`]，但用于起点中文网 cookie（订阅章节专用）。
pub fn qidian_cookie(cfg: &AppConfig) -> Option<String> {
    let trimmed = cfg.cookie.qidian_cookie.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(cfg.cookie.qidian_cookie.clone())
    }
}

/// 校验 `download_path`：非空 + 路径存在 + 是目录。
///
/// 错误字符串语义稳定，方便调用方映射到稳定的错误码：
/// - 空字符串 → `"download_path_empty"`
/// - 不存在 → `"download_path_not_found"`
/// - 是文件不是目录 → `"download_path_not_dir"`
///
/// 之所以返回 `Result<(), String>`（不抛 anyhow）：
/// - web handler 当前用 `(StatusCode, String)` 直接返回，需要稳定短码做 i18n 键
/// - anyhow 的 `format!("{e:#}")` 会泄露内部路径（C:\\Users\\xxx）
///
/// 调用方负责把字符串转成自己需要的形态（web → `BadRequest` / cli → `anyhow!`）。
pub fn validate_download_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("download_path_empty".to_string());
    }
    let p = std::path::Path::new(trimmed);
    if !p.exists() {
        return Err("download_path_not_found".to_string());
    }
    if !p.is_dir() {
        return Err("download_path_not_dir".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn cfg_with_cf_bypass(s: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.global.cf_bypass = s.to_string();
        cfg
    }

    fn cfg_with_qidian(s: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.cookie.qidian_cookie = s.to_string();
        cfg
    }

    // ── cf_bypass ───────────────────────────────────────────────

    #[test]
    fn cf_bypass_empty_string_returns_none() {
        let cfg = cfg_with_cf_bypass("");
        assert!(cf_bypass(&cfg).is_none());
    }

    #[test]
    fn cf_bypass_whitespace_only_returns_none() {
        // " " / "\t\n" 视作空 — 这是为什么函数内部用 trim
        let cfg = cfg_with_cf_bypass("   \t\n  ");
        assert!(cf_bypass(&cfg).is_none());
    }

    #[test]
    fn cf_bypass_non_empty_returns_some_clone() {
        let cfg = cfg_with_cf_bypass("https://cf.example.com");
        assert_eq!(cf_bypass(&cfg).as_deref(), Some("https://cf.example.com"));
    }

    #[test]
    fn cf_bypass_does_not_trim_leading_whitespace_in_value() {
        // trim 只用于 "是否为空" 判断；返回值保留原始字符串（含前后空白）
        // 调用方如果关心 trim 应该自己做；这跟原 handler 行为一致
        let cfg = cfg_with_cf_bypass("  https://x  ");
        assert_eq!(cf_bypass(&cfg).as_deref(), Some("  https://x  "));
    }

    // ── qidian_cookie ──────────────────────────────────────────

    #[test]
    fn qidian_cookie_empty_returns_none() {
        let cfg = cfg_with_qidian("");
        assert!(qidian_cookie(&cfg).is_none());
    }

    #[test]
    fn qidian_cookie_non_empty_returns_some() {
        let cfg = cfg_with_qidian("qidian_sess=abc123");
        assert_eq!(qidian_cookie(&cfg).as_deref(), Some("qidian_sess=abc123"));
    }

    // ── validate_download_path ─────────────────────────────────

    #[test]
    fn validate_download_path_empty_string_rejected() {
        let err = validate_download_path("").unwrap_err();
        assert_eq!(err, "download_path_empty");
    }

    #[test]
    fn validate_download_path_whitespace_only_rejected() {
        let err = validate_download_path("   \t\n   ").unwrap_err();
        assert_eq!(err, "download_path_empty");
    }

    #[test]
    fn validate_download_path_nonexistent_rejected() {
        let err = validate_download_path("Z:/definitely/not/a/path/xyz123").unwrap_err();
        assert_eq!(err, "download_path_not_found");
    }

    #[test]
    fn validate_download_path_existing_file_rejected_as_not_dir() {
        // 临时文件（非目录）应拒绝
        let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        let path_str = tmp.path().to_str().expect("utf-8 path");
        let err = validate_download_path(path_str).unwrap_err();
        assert_eq!(err, "download_path_not_dir");
    }

    #[test]
    fn validate_download_path_existing_dir_accepted() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let path_str = tmp.path().to_str().expect("utf-8 path");
        assert!(validate_download_path(path_str).is_ok());
    }
}
