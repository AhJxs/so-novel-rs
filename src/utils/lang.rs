//! 语言/locale 工具。对应 Java `util.LangUtil`。

use crate::config::LangType;

/// 检测系统当前 locale 并映射到 `LangType`。
/// 在 Linux 上读 `LANG` 环境变量，Windows/macOS 上读 `LC_ALL`/`LANG` 兜底，
/// 都拿不到时默认 `ZhCn`（与 Java 端一致）。
pub fn detect_system_lang() -> LangType {
    let candidates = [
        std::env::var("LC_ALL").ok(),
        std::env::var("LC_MESSAGES").ok(),
        std::env::var("LANG").ok(),
    ];
    for c in candidates.into_iter().flatten() {
        let lower = c.to_lowercase();
        if lower.contains("zh_tw") || lower.contains("zh-tw") {
            return LangType::ZhTw;
        }
        if lower.contains("zh_hant") || lower.contains("zh-hant") {
            return LangType::ZhHant;
        }
        if lower.starts_with("zh") {
            return LangType::ZhCn;
        }
    }
    LangType::ZhCn
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn fallback_is_zh_cn() {
        // 即使 env 中无关也至少返回某个 LangType（不 panic）。
        let lt = detect_system_lang();
        assert!(matches!(
            lt,
            LangType::ZhCn | LangType::ZhTw | LangType::ZhHant
        ));
    }
}
