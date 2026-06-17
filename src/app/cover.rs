//! 封面字节解码 + URI 生成。
//!
//! `CoverEntry` 是 UI 中立结构：只存原始图片字节 + 源 / URL 元信息，
//! 解码成可显示格式由 UI 层负责（GPUI 端用 `image::ImageReader` 解码 RGBA）。

/// 封面缓存条目。`Ready` 持有 `Vec<u8>`（已校验为有效图片的原始字节）；
/// `Failed` 保留错误文案以便 UI 给出可见反馈而非静默。
///
/// UI 层负责把字节解码为可显示格式。
pub enum CoverEntry {
    Ready {
        /// 原始图片字节（PNG / JPEG / WebP 等）。
        bytes: Vec<u8>,
        /// 唯一 URI（`cover://{source_id}/{hash}`），用于 GPUI 端做纹理缓存去重。
        uri: String,
    },
    Failed(String),
}

/// 把后台下载的字节构造为 `CoverEntry`。
/// 失败（空 body / 解码错误）时给出中文短文案，UI 仍会显示一行小字提示。
pub(crate) fn cover_entry_from_bytes(
    source_id: i32,
    cover_url: &str,
    bytes: Option<Vec<u8>>,
) -> CoverEntry {
    match bytes {
        None => CoverEntry::Failed("下载为空或失败".to_string()),
        Some(b) => {
            // 提前用 image::ImageReader 验证字节是真的图片（避免 lazy 解码时 ui.add 失败）。
            let probe = image::ImageReader::new(std::io::Cursor::new(&b))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.decode().ok());
            match probe {
                Some(_) => {
                    let uri = format!("cover://{source_id}/{}", hash_short(cover_url));
                    CoverEntry::Ready { bytes: b, uri }
                }
                None => CoverEntry::Failed("图片解码失败（非有效图片或格式不支持）".to_string()),
            }
        }
    }
}

/// 短哈希（fnv-like 64-bit → 16 hex），仅用于 URI 去重 key，**不是**密码学用途。
pub fn hash_short(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod cover_tests {
    use super::*;
    use std::io::Cursor;

    /// 构造一个 2x2 RGBA 红色像素的 PNG 字节流。
    fn make_png_bytes() -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("write png");
        buf
    }

    #[test]
    fn cover_entry_from_bytes_decodes_valid_png() {
        let png = make_png_bytes();
        assert!(!png.is_empty(), "PNG 字节流不应为空");
        let entry = cover_entry_from_bytes(7, "https://example.com/cover.png", Some(png));
        match entry {
            CoverEntry::Ready { bytes: _, uri: _ } => {
                // bytes 持有原图字节，uri 用于 GPUI 纹理缓存去重。
            }
            CoverEntry::Failed(e) => panic!("期望 Ready，实际 Failed: {e}"),
        }
    }

    #[test]
    fn cover_entry_from_bytes_rejects_garbage() {
        let entry = cover_entry_from_bytes(
            1,
            "https://example.com/bad.png",
            Some(b"this is not a valid image".to_vec()),
        );
        match entry {
            CoverEntry::Failed(msg) => assert!(msg.contains("解码失败"), "错误文案: {msg}"),
            CoverEntry::Ready { .. } => panic!("垃圾字节不应成功解码"),
        }
    }

    #[test]
    fn cover_entry_from_bytes_handles_none() {
        let entry = cover_entry_from_bytes(1, "https://example.com/x.png", None);
        assert!(matches!(entry, CoverEntry::Failed(_)));
    }

    #[test]
    fn cover_entry_from_bytes_uses_distinct_uris() {
        let png = make_png_bytes();
        let a = cover_entry_from_bytes(1, "https://a.com/c.png", Some(png.clone()));
        let b = cover_entry_from_bytes(2, "https://a.com/c.png", Some(png.clone()));
        let c = cover_entry_from_bytes(1, "https://b.com/c.png", Some(png));
        assert!(matches!(a, CoverEntry::Ready { .. }));
        assert!(matches!(b, CoverEntry::Ready { .. }));
        assert!(matches!(c, CoverEntry::Ready { .. }));
    }

    #[test]
    fn hash_short_is_deterministic_and_distinct() {
        let h1 = hash_short("https://a.com/c.png");
        let h2 = hash_short("https://a.com/c.png");
        assert_eq!(h1, h2, "相同输入应得到相同哈希");
        assert_eq!(h1.len(), 16, "应为 16 hex chars (64-bit)");
        let h3 = hash_short("https://b.com/c.png");
        assert_ne!(h1, h3, "不同输入应得到不同哈希");
    }
}
