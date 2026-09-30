//! 字符串 / 时间 / 大小格式化。业务领域相关的（书名 / 作者 / 章节标题）由各 page
//! 自己按需 truncate，这里只放通用工具：
//!
//! - [`truncate`]：字符级截断 + 省略号；[`format_local_unix_secs`]：unix 秒 → 本地
//!   `YYYY-MM-DD HH:MM`（0 / 解析失败 / 格式化失败各走独立 i18n fallback）。
//! - [`format_size`] / [`format_duration`] 分别 re-export 自 [`super::fs`] / [`super::time`]。

pub use super::fs::format_size;
pub use super::time::format_duration;

/// 字符级截断，超过 `max_chars` 时末尾加 `…`；中文 / 表情按 1 个字符计数
/// （不做 display width 估算，列表里的话术多为中文）。
///
/// `max_chars == 0` → 空串；`max_chars >= 字符数` → 原样；否则截到 `max_chars - 1` 个字符 + `…`。
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
/// 三个错误分支各走独立 i18n key：`secs <= 0`（未设置 / 还没记录）→ `unknown_key`；
/// `from_unix_timestamp` 失败 → `invalid_key`；`Rfc3339` 格式化失败 →
/// `format_failed_key`（后两者理论上有 `time` crate 保证，保留兜底）。不区分的
/// caller（tasks page）三个 key 传同一个。
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
    local.format(&Rfc3339).ok().map_or_else(
        || crate::i18n::ts(format_failed_key).to_string(),
        |s| s[..16].replace('T', " "),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
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
            1_705_307_400,
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
