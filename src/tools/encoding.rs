//! 编码探测与解码。BOM 优先，其次用户显式指定，最后 chardetng 猜测。

use std::path::{Path, PathBuf};

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

/// TXT 编码转换：解码后按目标编码重新写出，文件名追加编码标签。
///
/// # Errors
///
/// 源文件不是 `.txt`、编码未知、目标编码无法表示全部字符或写入失败时返回 [`ToolError`]。
pub fn run_encoding(
    source: &Path,
    out_dir: &Path,
    source_encoding: Option<&str>,
    target_encoding: &str,
) -> Result<PathBuf, ToolError> {
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "txt" {
        return Err(ToolError::UnsupportedFormat(extension));
    }

    let bytes = std::fs::read(source)?;
    let (text, _detected) = decode_bytes(&bytes, parse_encoding(source_encoding)?);

    let target = parse_target_encoding(target_encoding)?;
    let (encoded, _, had_errors) = target.encode(&text);
    if had_errors {
        return Err(ToolError::Unencodable(target.name().to_string()));
    }

    std::fs::create_dir_all(out_dir)?;
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("未命名");
    let filename = format!("{stem} ({}).txt", target.name());
    let path = crate::export::exporter::unique_path(out_dir, &filename);
    std::fs::write(&path, encoded.as_ref())?;
    Ok(path)
}

/// 解析可写出的编码。`encoding_rs` 的 UTF-16 只支持解码，因此排除。
fn parse_target_encoding(label: &str) -> Result<&'static Encoding, ToolError> {
    let target = Encoding::for_label(label.trim().as_bytes())
        .ok_or_else(|| ToolError::UnknownEncoding(label.to_string()))?;
    if target.output_encoding() != target {
        return Err(ToolError::UnknownEncoding(label.to_string()));
    }
    Ok(target)
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

    #[test]
    fn converts_utf8_to_gbk() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "中文测试").unwrap();

        let path = super::run_encoding(&source, tmp.path(), None, "GBK").unwrap();

        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("(GBK)")
        );
        let bytes = std::fs::read(&path).unwrap();
        let (text, _, _) = GBK.decode(&bytes);
        assert_eq!(text, "中文测试");
    }

    #[test]
    fn strips_bom_when_writing_utf8() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend_from_slice("中文".as_bytes());
        std::fs::write(&source, bytes).unwrap();

        let path = super::run_encoding(&source, tmp.path(), None, "UTF-8").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), "中文".as_bytes());
    }

    #[test]
    fn rejects_unknown_target_encoding() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "中文").unwrap();

        let result = super::run_encoding(&source, tmp.path(), None, "nope");

        assert!(matches!(result, Err(super::ToolError::UnknownEncoding(_))));
    }

    #[test]
    fn rejects_non_txt_source() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.md");
        std::fs::write(&source, "中文").unwrap();

        let result = super::run_encoding(&source, tmp.path(), None, "GBK");

        assert!(matches!(
            result,
            Err(super::ToolError::UnsupportedFormat(_))
        ));
    }
}
