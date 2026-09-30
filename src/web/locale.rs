//! Per-request locale extractor。
//!
//! `rust_i18n::locale()` 是全局 atomic，web 多请求并发会互相踩。这里在 handler
//! 入口从 `Accept-Language` 解析出 locale 字符串（降级：`AppConfig.language` → `"en"`），
//! 之后所有翻译都走 `crate::i18n::ts_for_locale(locale, key)`，**不**碰全局。
//!
//! 解析是简化版 BCP-47：按 q 降序，先精确匹配 `SUPPORTED_LOCALES`，再按 `zh` / `en`
//! 前缀兜底（`zh-HK` → `zh-TW`）；不支持 wildcard `*`，够覆盖浏览器实际发出的头。

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::config::Language;
use crate::utils::lock::rw_read_or;
use crate::web::SharedState;

/// 我们接受的 3 个 locale tag（精确匹配）。
///
/// 与 `crate::i18n::locale_for` 的返回值一致；前端 JSON 文件名统一用 `zh-TW`。
const SUPPORTED_LOCALES: &[&str] = &["en", "zh-CN", "zh-TW"];

/// Handler 入口拿到的 per-request locale。
///
/// 内部存 `&'static str`（所有解析路径最终都指向 `SUPPORTED_LOCALES` 之一），
/// **没有** `String`/`Box` 分配；`Copy` 让 handler 可以自由复制传多处使用点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locale(pub &'static str);

impl Locale {
    /// 直接构造（编译期不校验 tag；解析走 [`Self::from_request_parts`]）。
    pub const fn new(tag: &'static str) -> Self {
        Self(tag)
    }

    /// 拿 `&str` 视图（handler 里调 `ts_for_locale(locale.0, key)` 用）。
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

/// `axum::extract::FromRequestParts` —— 每次进入 handler 自动调用。
///
/// 优先级：`Accept-Language` 头 → `AppConfig.language` → `"en"`。
/// 锁毒化走 `"en"` 兜底并留 warn —— locale 问题不该阻塞业务逻辑。
impl<S> FromRequestParts<S> for Locale
where
    S: Send + Sync,
    SharedState: axum::extract::FromRef<S>,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let header_locale = parts
            .headers
            .get(axum::http::header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_accept_language);

        if let Some(tag) = header_locale {
            return Ok(Self(tag));
        }

        let shared: SharedState = axum::extract::FromRef::from_ref(state);
        // 锁 guard 嵌在表达式里（`significant_drop_tightening`），用完即释放。
        let lang = rw_read_or("web::locale", &shared.config)
            .map_or(Language::English, |c| c.global.language);
        Ok(Self(crate::i18n::locale_for(lang)))
    }
}

/// 把 `Accept-Language` 头解析成我们接受的 locale tag。
///
/// 按 `,` 切条目、剥 `;q=`（缺省 1.0）、按 q 降序：先精确匹配 `SUPPORTED_LOCALES`，
/// 再按 `zh` / `en` 前缀兜底（`zh-HK` → `zh-TW`、`en-US` → `en`），都没有 → `None`。
/// 不依赖外部 crate。
pub fn parse_accept_language(header: &str) -> Option<&'static str> {
    let mut candidates: Vec<(&str, f32)> = Vec::new();
    for entry in header.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (tag, q) = match entry.split_once(';') {
            Some((tag, rest)) => {
                let q = rest
                    .trim()
                    .strip_prefix("q=")
                    .or_else(|| rest.trim().strip_prefix("Q="))
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(1.0);
                (tag.trim(), q)
            }
            None => (entry, 1.0),
        };
        if tag.is_empty() {
            continue;
        }
        candidates.push((tag, q));
    }
    // q 降序，同 q 保持原序
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (tag, _) in &candidates {
        if let Some(&supported) = SUPPORTED_LOCALES.iter().find(|s| **s == *tag) {
            return Some(supported);
        }
    }

    for (tag, _) in &candidates {
        let lower = tag.to_ascii_lowercase();
        let mapped = match lower.as_str() {
            t if t.starts_with("zh") => Some("zh-TW"),
            // zh-CN / zh-HK / zh-Hans / zh-Hant 全部映射到 zh-TW —— 中文内容高度
            // 重合，比 fallback 到 en 好；区分简繁交给前端 i18n 处理。
            t if t.starts_with("en") => Some("en"),
            _ => None,
        };
        if let Some(s) = mapped {
            return Some(s);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn parse_exact_match_wins() {
        assert_eq!(parse_accept_language("zh-CN"), Some("zh-CN"));
        assert_eq!(parse_accept_language("zh-TW"), Some("zh-TW"));
        assert_eq!(parse_accept_language("en"), Some("en"));
    }

    #[test]
    fn parse_q_value_orders_correctly() {
        assert_eq!(
            parse_accept_language("en;q=0.5, zh-CN;q=0.9"),
            Some("zh-CN")
        );
        assert_eq!(parse_accept_language("zh-CN;q=0.5, en;q=0.9"), Some("en"));
    }

    #[test]
    fn parse_unsupported_falls_back_to_prefix() {
        assert_eq!(parse_accept_language("zh-HK"), Some("zh-TW"));
        assert_eq!(parse_accept_language("zh-Hant"), Some("zh-TW"));
        assert_eq!(parse_accept_language("zh-Hans"), Some("zh-TW"));
        assert_eq!(parse_accept_language("en-US"), Some("en"));
        assert_eq!(parse_accept_language("en-GB"), Some("en"));
    }

    #[test]
    fn parse_unsupported_unsupported_returns_none() {
        assert_eq!(parse_accept_language("ja"), None);
        assert_eq!(parse_accept_language("fr-FR"), None);
        assert_eq!(parse_accept_language("ko-KR"), None);
    }

    #[test]
    fn parse_malformed_q_returns_default_q() {
        assert_eq!(parse_accept_language("en;q=foo"), Some("en"));
        assert_eq!(parse_accept_language(";q=0.5"), None);
    }

    #[test]
    fn parse_case_insensitive_tag() {
        // 精确匹配是 case-sensitive，但前缀匹配先 lowercase，故 EN / ZH 能命中
        assert_eq!(parse_accept_language("EN"), Some("en"));
        assert_eq!(parse_accept_language("ZH-CN"), Some("zh-TW"));
    }

    #[test]
    fn parse_empty_or_whitespace_returns_none() {
        assert_eq!(parse_accept_language(""), None);
        assert_eq!(parse_accept_language("   "), None);
        assert_eq!(parse_accept_language(",,,"), None);
    }

    #[test]
    fn parse_multiple_with_default_q() {
        // 同 q 保持原序，第一个出现的胜出
        assert_eq!(parse_accept_language("zh-TW, en;q=0.5"), Some("zh-TW"));
    }

    #[test]
    fn locale_struct_is_copy() {
        // 不变量：Locale 必须 Copy（多处使用点自由复制）
        let l = Locale::new("en");
        let l2 = l;
        let _ = l; // 不能 move，只能 copy
        assert_eq!(l2.as_str(), "en");
    }

    #[test]
    fn all_supported_locales_in_app_yml() {
        // 关键不变量：SUPPORTED_LOCALES 里的 tag 必须在 app.yml 有翻译。
        for &tag in SUPPORTED_LOCALES {
            let v = crate::i18n::ts_for_locale(tag, "Nav.tasks");
            assert!(
                !v.is_empty() && v != "Nav.tasks",
                "{tag} 在 app.yml 缺翻译：got {v:?}"
            );
        }
    }
}
