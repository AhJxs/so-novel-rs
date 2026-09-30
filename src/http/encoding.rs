//! 编码兜底：reqwest / Jsoup 的自动编码检测在中文老站点上都会出错
//! （声明 UTF-8 实际是 GBK，或者根本不声明）。
//!
//! 优先级：Content-Type charset → body 内 `<meta charset>` → chardetng 探测 → UTF-8。
//!
//! `decode_response_bytes` 只吃 body bytes + 可选的 Content-Type 头（不吃
//! `reqwest::Response`），以便脱离 HTTP 上下文单测。

use encoding_rs::{Encoding, UTF_8};
use regex::Regex;
use std::sync::LazyLock;

/// 静态正则字面量写错即程序员错误，故直接 panic（避开 `clippy::expect_used`）。
#[allow(
    clippy::panic,
    reason = "static regex literal must compile; failure = programmer error"
)]
fn compile_static_re(pattern: &'static str) -> Regex {
    match Regex::new(pattern) {
        Ok(re) => re,
        Err(e) => panic!("static regex `{pattern}` should compile: {e}"),
    }
}

static META_CHARSET_RE: LazyLock<Regex> = LazyLock::new(|| {
    // 同时匹配 <meta charset="GBK"> 与 <meta http-equiv="Content-Type" content="...; charset=GBK">
    compile_static_re(
        r#"(?is)<meta[^>]*?(?:charset\s*=\s*["']?([\w-]+)|content\s*=\s*["'][^"']*?charset\s*=\s*([\w-]+))"#,
    )
});

/// 解码 HTTP 响应 body。`content_type` 传响应头 `Content-Type` 的原始字符串（不要预解析）。
pub fn decode_response_bytes(bytes: &[u8], content_type: Option<&str>) -> String {
    if let Some(ct) = content_type
        && let Some(charset) = parse_charset_from_content_type(ct)
        && let Some(enc) = Encoding::for_label(charset.as_bytes())
    {
        let (cow, _, _) = enc.decode(bytes);
        return cow.into_owned();
    }

    // 只在前 4 KB 里搜，足够覆盖 <head>，也避免在大正文上跑正则。
    let head_slice = &bytes[..bytes.len().min(4096)];
    // 头部不是合法 UTF-8 时用 latin1 兜一下，正则里的 ASCII 关键字仍能命中。
    let head_text: String = std::str::from_utf8(head_slice).map_or_else(
        |_| head_slice.iter().map(|&b| b as char).collect(),
        str::to_string,
    );
    if let Some(cap) = META_CHARSET_RE.captures(&head_text) {
        let label = cap.get(1).or_else(|| cap.get(2)).map_or("", |m| m.as_str());
        if let Some(enc) = Encoding::for_label(label.as_bytes()) {
            let (cow, _, _) = enc.decode(bytes);
            return cow.into_owned();
        }
    }

    // chardetng 探测：中文站点不需要 ISO-2022-JP（Deny）；若 body 全是合法 UTF-8
    // 字节则信任 UTF-8（Allow）—— "声明 UTF-8 实际 GBK" 的坏例已被前两步拦下。
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let enc = detector.guess(None, chardetng::Utf8Detection::Allow);
    let (cow, _, _) = enc.decode(bytes);
    if !cow.is_empty() {
        return cow.into_owned();
    }

    UTF_8.decode(bytes).0.into_owned()
}

/// 从 `Content-Type: text/html; charset=GBK` 这类字符串里提取 charset 值。
fn parse_charset_from_content_type(ct: &str) -> Option<String> {
    for part in ct.split(';') {
        let part = part.trim();
        if let Some(rest) = part
            .strip_prefix("charset=")
            .or_else(|| part.strip_prefix("CHARSET="))
        {
            let rest = rest.trim().trim_matches(|c| c == '"' || c == '\'');
            return Some(rest.to_string());
        }
        if let Some(eq) = part.find('=') {
            let (k, v) = part.split_at(eq);
            if k.trim().eq_ignore_ascii_case("charset") {
                let v = v.trim_start_matches('=').trim();
                let v = v.trim_matches(|c| c == '"' || c == '\'');
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn decodes_via_content_type_header() {
        // GBK 里"中文"两字
        let gbk_bytes = [0xD6, 0xD0, 0xCE, 0xC4]; // 中文
        let s = decode_response_bytes(&gbk_bytes, Some("text/html; charset=GBK"));
        assert_eq!(s, "中文");
    }

    #[test]
    fn decodes_via_meta_charset_when_header_missing() {
        let mut buf = Vec::new();
        buf.extend_from_slice(
            b"<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=gbk\"></head><body>",
        );
        buf.extend_from_slice(&[0xD6, 0xD0, 0xCE, 0xC4]);
        buf.extend_from_slice(b"</body></html>");
        let s = decode_response_bytes(&buf, None);
        assert!(s.contains("中文"), "decoded: {s:?}");
    }

    #[test]
    fn decodes_via_meta_short_form() {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"<html><head><meta charset='GBK'></head><body>");
        buf.extend_from_slice(&[0xD6, 0xD0, 0xCE, 0xC4]);
        buf.extend_from_slice(b"</body></html>");
        let s = decode_response_bytes(&buf, None);
        assert!(s.contains("中文"));
    }

    #[test]
    fn falls_back_to_utf8_for_normal_html() {
        let html = "<html><body>中文</body></html>";
        let s = decode_response_bytes(html.as_bytes(), None);
        assert!(s.contains("中文"));
    }

    #[test]
    fn parse_charset_handles_quoted() {
        assert_eq!(
            parse_charset_from_content_type("text/html; charset=\"UTF-8\""),
            Some("UTF-8".to_string())
        );
        assert_eq!(
            parse_charset_from_content_type("text/html;charset=gbk"),
            Some("gbk".to_string())
        );
    }
}
