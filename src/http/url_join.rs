//! URL 拼接工具。对应 Java jsoup 的 `Element.absUrl(attrName)`。
//!
//! parser 拿到元素 `href` 原始值后，以当前页面 baseUri 为基准求绝对 URL
//! （`Url::join` 已覆盖 `/abs`、`./rel`、`../up`、`?query`、`#frag`、协议相对 `//host/...`）。

use url::Url;

/// 把 `href` 解析为绝对 URL（`base` 必须是绝对 URL）；空串或解析失败返回 `None`。
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

/// 取 URL 的 origin（`scheme://host[:port]/`）用作 Referer 头；解析失败返回原串。
pub fn origin_or_self(url: &str) -> String {
    Url::parse(url).map_or_else(
        |_| url.to_string(),
        |u| {
            let origin = u.origin();
            // opaque origin 时 unicode_serialization() 会返回 "null"；书源一定是
            // http(s)，所以用 ascii_serialization。
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
