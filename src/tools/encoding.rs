//! 编码探测与解码。BOM 优先，其次用户显式指定，最后 chardetng 猜测。

use std::path::Path;

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

use super::ToolError;

/// 解码字节；返回 (文本, 实际使用的编码)。
#[must_use]
pub fn decode_bytes(
    bytes: &[u8],
    explicit: Option<&'static Encoding>,
) -> (String, &'static Encoding) {
    if let Some((encoding, bom_len)) = sniff_bom(bytes) {
        let (text, _, _) = encoding.decode(&bytes[bom_len..]);
        return (text.into_owned(), encoding);
    }
    let encoding = explicit.unwrap_or_else(|| detect_encoding(bytes));
    let (text, _, _) = encoding.decode(bytes);
    (text.into_owned(), encoding)
}

/// 只做探测，不消费文本。
#[must_use]
pub fn detect_encoding(bytes: &[u8]) -> &'static Encoding {
    if let Some((encoding, _)) = sniff_bom(bytes) {
        return encoding;
    }
    // 本地小说文件不涉及 ISO-2022-JP；全合法 UTF-8 时优先判为 UTF-8。
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    detector.guess(None, chardetng::Utf8Detection::Allow)
}

/// 读取文件并探测编码。
///
/// # Errors
///
/// 文件读取失败时返回 [`ToolError::Io`]。
pub fn detect_file_encoding(path: &Path) -> Result<&'static Encoding, ToolError> {
    let bytes = std::fs::read(path)?;
    Ok(detect_encoding(&bytes))
}

/// 解析用户传入的编码名；`auto` / 空串表示交给探测。
///
/// # Errors
///
/// 标签无法识别时返回 [`ToolError::UnknownEncoding`]。
pub fn parse_encoding(label: Option<&str>) -> Result<Option<&'static Encoding>, ToolError> {
    match label.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("auto") => Ok(None),
        Some(label) => Encoding::for_label(label.as_bytes())
            .map(Some)
            .ok_or_else(|| ToolError::UnknownEncoding(label.to_string())),
    }
}

fn sniff_bom(bytes: &[u8]) -> Option<(&'static Encoding, usize)> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        Some((UTF_8, 3))
    } else if bytes.starts_with(&[0xff, 0xfe]) {
        Some((UTF_16LE, 2))
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        Some((UTF_16BE, 2))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use encoding_rs::{GBK, UTF_8};

    use super::{decode_bytes, detect_encoding};

    #[test]
    fn utf8_bom_is_detected_and_stripped() {
        let bytes = [0xef, 0xbb, 0xbf, b'h', b'i'];
        assert_eq!(detect_encoding(&bytes), UTF_8);
        let (text, encoding) = decode_bytes(&bytes, None);
        assert_eq!(text, "hi");
        assert_eq!(encoding, UTF_8);
    }

    #[test]
    fn gbk_text_is_detected() {
        let text = "这是一段用于检测编码的中文文本，应该被识别为 GBK 编码而不是 UTF-8。";
        let (bytes, _, _) = GBK.encode(text);
        assert_eq!(detect_encoding(&bytes), GBK);
    }

    #[test]
    fn explicit_encoding_wins_without_bom() {
        let text = "中文";
        let (bytes, _, _) = GBK.encode(text);
        let (decoded, used) = decode_bytes(&bytes, Some(GBK));
        assert_eq!(decoded, text);
        assert_eq!(used, GBK);
    }
}
