//! 字符串 / 时间 / 大小格式化。
//!
//! 业务领域相关的（书名 / 作者 / 章节标题）由各 page 自己按需 truncate；
//! 这里只放通用工具：
//! - [`truncate`]: 字符级截断 + 省略号（按 display width 的可用近似）
//! - [`format_duration`]: 复用 [`crate::util::time::format_duration`]
//! - [`format_size`]: 复用 [`crate::util::fs::format_size`]
//!
//! 不重新发明；这里只做"在新 GUI 栈下也能从 gpui_app 直接调"的 re-export。

pub use crate::util::fs::format_size;
pub use crate::util::time::format_duration;

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
}
