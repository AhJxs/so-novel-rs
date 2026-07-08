//! 字符串 / 时间 / 大小格式化。
//!
//! 业务领域相关的（书名 / 作者 / 章节标题）由各 page 自己按需 truncate；
//! 这里只放通用工具：
//! - [`truncate`]: 字符级截断 + 省略号（按 display width 的可用近似）
//! - [`format_duration`]: 复用 [`super::time::format_duration`]
//! - [`format_size`]: 复用 [`super::fs::format_size`]
//! - [`format_local_unix_secs`]: unix 秒 → "YYYY-MM-DD HH:MM"（本地时区），
//!   0 / 解析失败 / 格式化失败各自走独立的 i18n fallback。
//!
//! ## 模块位置历史
//!
//! 早期在 `gpui_app::components::formatting`，但 `truncate` / `format_local_unix_secs`
//! 本身零 GUI 依赖（前者只吃 `String`/`&str`/`char`，后者只吃 `i64` + 3 个 i18n key）。
//! 挪到 `util::formatting` 后：
//! - `app/` 和 `gpui_app/` 都能直接 `use crate::utils::formatting::*`；
//! - `gpui_app::components` 仍 `pub use crate::utils::formatting::{format_size, truncate}`
//!   保持向后兼容，UI 内部 page 模块不用改。

pub use super::fs::format_size;
pub use super::time::format_duration;

/// 字符级截断，超过 `max_chars` 时末尾加 `…`。
///
/// 中文 / 表情符号按 1 个字符计数（不做 width 估算 — 列表里出现的话术
/// 多数是中文，简单 truncate 即可）。如果未来要支持 CJK width，迁移到
/// `unicode-width` crate。
///
/// 行为：
/// - `max_chars == 0` → 返回空串
/// - `max_chars >= s.chars().count()` → 原样返回
/// - 否则截到 `max_chars - 1` 个字符 + `…`
pub fn truncate(s: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_string();
    }
    if max_chars == 1 {
        return "…".to_string();
    }
    let keep = max_chars - 1;
    let mut out = String::with_capacity(keep * 4);
    for c in s.chars().take(keep) {
        out.push(c);
    }
    out.push('…');
    out
}

/// unix 秒 → "YYYY-MM-DD HH:MM"（本地时区）。
///
/// 三个错误分支走独立 i18n key：
/// - `secs <= 0` → `unknown_key`（最常见：未设置 / 启动后还没记录时间）
/// - `OffsetDateTime::from_unix_timestamp` 解析失败 → `invalid_key`
///   （理论上 i64 范围内都合法，保留兜底）
/// - `Rfc3339` 格式化失败 → `format_failed_key`（理论上 `time` crate 自保证，保留兜底）
///
/// callers that don't differentiate（tasks page）pass `unknown_key` 三次。
///
/// 取代了原先散在 `library.rs::format_unix_secs` + `tasks.rs::format_started_time`
/// 的两份重复实现（PR0 helper extraction，task #47）。
pub fn format_local_unix_secs(
    secs: i64,
    unknown_key: &'static str,
    invalid_key: &'static str,
    format_failed_key: &'static str,
) -> String {
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    if secs <= 0 {
        return crate::i18n::ts(unknown_key).to_string();
    }
    let Ok(dt) = OffsetDateTime::from_unix_timestamp(secs) else {
        return crate::i18n::ts(invalid_key).to_string();
    };
    let local =
        dt.to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC));
    local
        .format(&Rfc3339)
        .ok()
        .map(|s| s[..16].replace('T', " "))
        .unwrap_or_else(|| crate::i18n::ts(format_failed_key).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_passthrough() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("你好", 5), "你好");
    }

    #[test]
    fn truncate_with_ellipsis() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("凡人修仙传", 3), "凡人…");
    }

    #[test]
    fn truncate_edge_cases() {
        assert_eq!(truncate("abc", 0), "");
        assert_eq!(truncate("abc", 1), "…");
        assert_eq!(truncate("abcd", 1), "…");
        assert_eq!(truncate("abcd", 2), "a…");
    }

    #[test]
    fn truncate_exact_boundary() {
        assert_eq!(truncate("abcd", 4), "abcd");
        assert_eq!(truncate("abcd", 5), "abcd");
    }

    #[test]
    fn format_local_unix_secs_zero_returns_unknown_key() {
        let s = format_local_unix_secs(
            0,
            "Library.time.unknown",
            "Library.time.invalid",
            "Library.time.format_failed",
        );
        assert_eq!(s, "(unknown)");
    }

    #[test]
    fn format_local_unix_secs_negative_returns_unknown_key() {
        let s = format_local_unix_secs(
            -1,
            "Library.time.unknown",
            "Library.time.invalid",
            "Library.time.format_failed",
        );
        assert_eq!(s, "(unknown)");
    }

    #[test]
    fn format_local_unix_secs_valid_returns_local_time() {
        // 2024-01-15 08:30:00 UTC
        let s = format_local_unix_secs(
            1705307400,
            "Library.time.unknown",
            "Library.time.invalid",
            "Library.time.format_failed",
        );
        // 本地时区不可预测，但格式必须是 "YYYY-MM-DD HH:MM"（16 字符）
        assert_eq!(s.len(), 16);
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[7], b'-');
        assert_eq!(s.as_bytes()[10], b' ');
        assert_eq!(s.as_bytes()[13], b':');
    }

    #[test]
    fn format_local_unix_secs_same_key_three_times() {
        // tasks page 风格：3 个分支共用同一个 key
        let s = format_local_unix_secs(
            0,
            "Tasks.card.meta.time_unknown",
            "Tasks.card.meta.time_unknown",
            "Tasks.card.meta.time_unknown",
        );
        assert_eq!(s, "(unknown time)");
    }
}
