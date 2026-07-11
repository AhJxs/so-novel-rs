//! Per-request locale extractor。
//!
//! 解决 1 个问题：`rust_i18n::locale()` 是全局 atomic，web 多请求并发会互相踩。
//! 解法：从 `Accept-Language` 头解析每个请求的 locale（降级：`AppConfig.language`
//! → `"en"`），axum extractor 在 handler 入口处把 locale 字符串拿进来，
//! 后续所有翻译走 `crate::i18n::ts_for_locale(locale, key)` —— 显式传 locale，
//! **不**碰全局。
//!
//! ## 用法
//!
//! ```ignore
//! use crate::web::locale::Locale;
//!
//! async fn handler(Locale(locale): Locale, ...) -> Result<...> {
//!     let msg = ts_for_locale(locale, "WebErrors.not_found");
//!     // ...
//! }
//! ```
//!
//! ## `Accept-Language` 解析
//!
//! 简化版 BCP-47（不依赖 `accept-language` crate，自己手搓 ~80 LOC）：
//! - 按 `,` 分割条目
//! - 每条目 `<tag>[;q=<0..1>]`，q 缺省 = 1.0
//! - 按 q 降序排列
//! - 取第一个**精确**匹配 `["en", "zh-CN", "zh-TW"]` 的 tag
//! - 没有精确匹配 → 取第一个以 `zh` / `en` 开头的 tag（兼容 `zh-HK` / `zh-Hans` /
//!   `zh-Hant` / `en-US` 等）
//! - 都没有 → 走 `AppConfig.language` fallback → `"en"`。
//!
//! 不是工业级 BCP-47（处理 wildcard `*` / 范围 `*;q=0.1` 不支持）—— 只覆盖浏览器
//! 实际发出的 `Accept-Language` 形态。3 个 locale 不需要 wildcard。

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::config::Language;
use crate::utils::lock::rw_read_or;
use crate::web::SharedState;

/// 我们接受的 3 个 locale tag（精确匹配）。
///
/// 与 `crate::i18n::locale_for` 返回的 tag 一致（zh-CN / zh-TW / en）——
/// 不接受 `zh-HK`（gpui-component 内部用）因为前端 JSON 文件名是 `zh-TW`。
const SUPPORTED_LOCALES: &[&str] = &["en", "zh-CN", "zh-TW"];

/// Handler 入口拿到的 per-request locale。
///
/// 内部存 `&'static str` —— `SUPPORTED_LOCALES` 里的字面量都是 `'static`，所有
/// 解析路径（Accept-Language / `AppConfig` fallback / "en" 兜底）最终都指向
/// `SUPPORTED_LOCALES` 之一，**没有** `String`/`Box` 分配。
///
/// `Copy + Clone` 让 handler 可以自由复制传给 SSE 闭包、SSE error event 构造点等
/// 多处使用点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locale(pub &'static str);

impl Locale {
    /// 直接构造（仅 `SUPPORTED_LOCALES` 内的 tag 才能拿到，编译期不强制，
    /// 但 `from_str_tag` 是唯一受信任入口）。测试 / 内部使用。
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
/// 优先级：
/// 1. `Accept-Language` 头（q 值最高且被支持的 tag）
/// 2. `AppConfig.language` （从 `state.config` 读 —— 即用户配置的语言）
/// 3. `"en"` 兜底
///
/// 锁毒化（`state.config` 不可用）走 fallback `"en"` —— 不返回错误，避免
/// locale 问题阻塞业务逻辑。tracing warn 留痕。
impl<S> FromRequestParts<S> for Locale
where
    S: Send + Sync,
    SharedState: axum::extract::FromRef<S>,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1. 尝试 Accept-Language 头
        let header_locale = parts
            .headers
            .get(axum::http::header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_accept_language);

        if let Some(tag) = header_locale {
            return Ok(Self(tag));
        }

        // 2. fallback: AppConfig.language
        let shared: SharedState = axum::extract::FromRef::from_ref(state);
        // `significant_drop_tightening` —— 把锁 guard 嵌在表达式里, 让作用域
        // 紧贴使用点, 锁尽早释放。
        let lang = rw_read_or("web::locale", &shared.config)
            .map_or(Language::English, |c| c.global.language);
        Ok(Self(crate::i18n::locale_for(lang)))
    }
}

/// 把 `Accept-Language` 头解析成我们接受的 locale tag。
///
/// 规则：
/// - 按 `,` 切条目
/// - 每条目剥 `;q=xxx`，剥空白
/// - q 缺省 = 1.0
/// - 按 q 降序排序
/// - 第一个**精确**匹配 `SUPPORTED_LOCALES` 的 tag
/// - 没有精确匹配 → 取第一个以 `zh` / `en` 开头的 tag 重新匹配 `SUPPORTED_LOCALES`
///   前缀（兼容 `zh-HK` → `zh-TW`、`zh-Hant` → `zh-TW`、`en-US` → `en` 等）
/// - 都没有 → `None`
///
/// 不依赖外部 crate（`accept-language` 等），~50 LOC 够用。
pub fn parse_accept_language(header: &str) -> Option<&'static str> {
    let mut candidates: Vec<(&str, f32)> = Vec::new();
    for entry in header.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        // 拆 tag 和 q-value
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

    // 1. 精确匹配
    for (tag, _) in &candidates {
        if let Some(&supported) = SUPPORTED_LOCALES.iter().find(|s| **s == *tag) {
            return Some(supported);
        }
    }

    // 2. 前缀匹配：zh / en 开头的 tag 重新映射
    for (tag, _) in &candidates {
        let lower = tag.to_ascii_lowercase();
        let mapped = match lower.as_str() {
            t if t.starts_with("zh") => Some("zh-TW"),
            // zh-* 全部映射到 zh-TW（包含 zh-CN / zh-HK / zh-Hans / zh-Hant / zh-TW）
            // —— 前端 JSON 文件名是 zh-TW，但中文内容跟 zh-CN 高度重合，用户体验
            // 上比 fallback 到 en 好。区分简繁的语义留给前端 i18n 自身处理。
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
        // q 高者优先
        assert_eq!(
            parse_accept_language("en;q=0.5, zh-CN;q=0.9"),
            Some("zh-CN")
        );
        assert_eq!(parse_accept_language("zh-CN;q=0.5, en;q=0.9"), Some("en"));
    }

    #[test]
    fn parse_unsupported_falls_back_to_prefix() {
        // zh-HK / zh-Hans / zh-Hant / zh-TW 都映射到 zh-TW
        assert_eq!(parse_accept_language("zh-HK"), Some("zh-TW"));
        assert_eq!(parse_accept_language("zh-Hant"), Some("zh-TW"));
        assert_eq!(parse_accept_language("zh-Hans"), Some("zh-TW"));
        // en-US / en-GB 映射到 en
        assert_eq!(parse_accept_language("en-US"), Some("en"));
        assert_eq!(parse_accept_language("en-GB"), Some("en"));
    }

    #[test]
    fn parse_unsupported_unsupported_returns_none() {
        // 全部不支持的语言 → None（让上层走 AppConfig fallback）
        assert_eq!(parse_accept_language("ja"), None);
        assert_eq!(parse_accept_language("fr-FR"), None);
        assert_eq!(parse_accept_language("ko-KR"), None);
    }

    #[test]
    fn parse_malformed_q_returns_default_q() {
        // q=foo 解析失败 → 默认 q=1.0
        assert_eq!(parse_accept_language("en;q=foo"), Some("en"));
        // 空 q → 跳过
        assert_eq!(parse_accept_language(";q=0.5"), None);
    }

    #[test]
    fn parse_case_insensitive_tag() {
        // 我们的精确匹配是 case-sensitive（en vs EN）—— 但前缀匹配走 lowercase
        // 所以 EN / ZH 都能落进前缀分支
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
        // q 缺省 = 1.0，第一个出现的应该胜出（同 q 保持原序）
        assert_eq!(parse_accept_language("zh-TW, en;q=0.5"), Some("zh-TW"));
    }

    #[test]
    fn locale_struct_is_copy() {
        // 编译期不变量：Locale 必须 Copy 让 SSE 闭包等多处用点自由复制
        let l = Locale::new("en");
        let l2 = l;
        let _ = l; // 不能 move，只能 copy
        assert_eq!(l2.as_str(), "en");
    }

    #[test]
    fn all_supported_locales_in_app_yml() {
        // 关键不变量：SUPPORTED_LOCALES 里的 tag 必须在 app.yml 有对应翻译。
        // 用 ts_for_locale 验一遍（不返 key 自身 = 翻译存在）。
        for &tag in SUPPORTED_LOCALES {
            // 任意 key —— Nav.tasks 在 3 locale 都有翻译
            let v = crate::i18n::ts_for_locale(tag, "Nav.tasks");
            assert!(
                !v.is_empty() && v != "Nav.tasks",
                "{tag} 在 app.yml 缺翻译：got {v:?}"
            );
        }
    }
}
