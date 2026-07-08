//! URL 拼接工具。对应 Java jsoup 的 `Element.absUrl(attrName)`。
//!
//! 现有规则中：
//! - 详情页 / 章节页的相对 href 占大多数；
//! - 搜索结果 href 一般在响应里就是绝对 URL，但少数书源会给相对路径。
//!
//! 这一层调用方都是 parser，要拿到选中元素的 `href` 原始值后再问"以
//! 当前页面的 baseUri 为基准，绝对路径是什么"。`url::Url::join` 会处理
//! `/abs`、`./rel`、`../up`、`?query`、`#frag`、协议相对 `//host/...` 等情况。

use url::Url;

/// 把 `href` 解析为绝对 URL；返回 `None` 表示 href 为空或解析失败。
///
/// `base` 是当前页面的 URL（必须是绝对 URL）。
pub fn abs_url(base: &str, href: &str) -> Option<String> {
    let trimmed = href.trim();
    if trimmed.is_empty() {
        return None;
    }
    // 对绝对 URL 做一次 parse 也无害，且能规整化。
    if let Ok(u) = Url::parse(trimmed) {
        return Some(u.to_string());
    }
    let base_url = Url::parse(base).ok()?;
    base_url.join(trimmed).ok().map(|u| u.to_string())
}

/// 取一个 URL 的 origin（scheme://host[:port]/），用作 Referer 头。
/// 解析失败时返回原串。
pub fn origin_or_self(url: &str) -> String {
    Url::parse(url).map_or_else(
        |_| url.to_string(),
        |u| {
            let origin = u.origin();
            // origin.unicode_serialization() 在 opaque origin 时返回 "null"；
            // 我们在书源场景下一定是 http(s)，可以用 ascii_serialization。
            origin.ascii_serialization()
        },
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn relative_to_absolute() {
        let abs = abs_url("https://www.22biqu.com/ss/", "/biqu123/").unwrap();
        assert_eq!(abs, "https://www.22biqu.com/biqu123/");
    }

    #[test]
    fn absolute_passes_through() {
        let abs = abs_url("https://www.22biqu.com/", "https://other.example/x.html").unwrap();
        assert_eq!(abs, "https://other.example/x.html");
    }

    #[test]
    fn protocol_relative() {
        let abs = abs_url("https://www.22biqu.com/", "//cdn.example/img.jpg").unwrap();
        assert_eq!(abs, "https://cdn.example/img.jpg");
    }

    #[test]
    fn dot_paths_resolve() {
        let abs = abs_url("https://www.22biqu.com/biqu1/123.html", "../biqu2/456.html").unwrap();
        assert_eq!(abs, "https://www.22biqu.com/biqu2/456.html");
    }

    #[test]
    fn empty_returns_none() {
        assert!(abs_url("https://x.test/", "").is_none());
        assert!(abs_url("https://x.test/", "   ").is_none());
    }

    #[test]
    fn origin_basic() {
        assert_eq!(
            origin_or_self("https://www.22biqu.com/path/?q=1"),
            "https://www.22biqu.com"
        );
    }
}
