//! `AppConfig` 的"空字符串视作 None"helper。
//!
//! desktop 读 `cf_bypass` / `qidian_cookie` 时统一走"trim 后空 → None"
//! 语义，集中在这里，避免每个调用方各写一份判断。

use crate::config::AppConfig;

/// `AppConfig.global.cf_bypass` 的 "空串视作 None" 包装：trim 后为空 → `None`（**不**走 bypass）。
///
/// 返回 `Option<String>` 而非 `Option<&str>`：调用方需要 `'static` 放进 `CrawlerOpts::cf_bypass`。
pub fn cf_bypass(cfg: &AppConfig) -> Option<String> {
    let trimmed = cfg.global.cf_bypass.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(cfg.global.cf_bypass.clone())
    }
}

/// 同 [`cf_bypass`]，用于起点中文网 cookie（订阅章节专用）。
pub fn qidian_cookie(cfg: &AppConfig) -> Option<String> {
    let trimmed = cfg.cookie.qidian_cookie.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(cfg.cookie.qidian_cookie.clone())
    }
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
        // 空白串视作空 —— 这是函数内部用 trim 的原因
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
        // trim 只用于判空，返回值保留原始空白（调用方自己 trim）
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
}
