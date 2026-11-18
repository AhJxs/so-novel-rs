//! 封面缓存：魔数嗅探、带内容哈希的文件名、落盘 / 清理，以及生成封面的确定配色。

use std::path::Path;

use super::book::hash64;

/// 按魔数嗅探图片扩展名；不认识的格式返回 `None`（封面直接丢弃）。
#[must_use]
pub fn sniff_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("jpg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

/// 封面缓存文件名：`<id>-<内容哈希>.<ext>`。
///
/// 带内容哈希是为了让 GPUI 的图片缓存随内容失效：同一本书换了封面，
/// 文件名随之变化，不需要手动清缓存。
#[must_use]
pub fn cover_file_name(id: &str, bytes: &[u8]) -> Option<String> {
    let ext = sniff_extension(bytes)?;
    Some(format!("{id}-{:016x}.{ext}", hash64(bytes)))
}

/// 把封面字节写入缓存目录，返回文件名；目录不可写或格式不认识时返回 `None`。
#[must_use]
pub fn write_cover(dir: &Path, id: &str, bytes: &[u8]) -> Option<String> {
    let file = cover_file_name(id, bytes)?;
    std::fs::create_dir_all(dir).ok()?;
    std::fs::write(dir.join(&file), bytes).ok()?;
    Some(file)
}

/// 删除一个封面缓存文件；文件名来自 `library.json`，先拒绝路径分隔符防目录穿越。
pub fn remove_cover_file(dir: &Path, file: Option<&str>) {
    let Some(file) = file else { return };
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        tracing::warn!("忽略可疑的封面文件名: {file}");
        return;
    }
    let _ = std::fs::remove_file(dir.join(file));
}

/// 重新定位后把封面文件名里的 id 换掉（内容哈希与扩展名保持不变）。
///
/// 返回新文件名；没有封面或改名失败时返回 `None`。
#[must_use]
pub fn rename_cover_for_relink(
    dir: &Path,
    old_id: &str,
    new_id: &str,
    file: Option<&str>,
) -> Option<String> {
    let file = file?;
    let suffix = file.strip_prefix(&format!("{old_id}-"))?;
    let new_file = format!("{new_id}-{suffix}");
    std::fs::rename(dir.join(file), dir.join(&new_file)).ok()?;
    Some(new_file)
}

/// 生成封面的确定配色：`(色相角度, 顶部 RGB, 底部 RGB)`。
///
/// 色相由书籍 ID 哈希决定，同一本书任何一次渲染颜色都相同；两段色同色系、明度不同，
/// 保证白色首字在两段上都可读。
#[must_use]
pub fn cover_palette(id: &str) -> (f32, [u8; 3], [u8; 3]) {
    // 色相只需要整数度数，64 位哈希转 f32 的精度损失无影响。
    #[allow(clippy::cast_precision_loss)]
    let hue = (hash64(id.as_bytes()) % 360) as f32;
    let top = hsl_to_rgb(hue, 0.46, 0.58);
    let bottom = hsl_to_rgb((hue + 18.0) % 360.0, 0.52, 0.36);
    (hue, top, bottom)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> [u8; 3] {
    let c = (1.0 - 2.0f32.mul_add(lightness, -1.0).abs()) * saturation;
    let h = hue / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let rgb = match h as u32 {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    };
    let m = lightness - c / 2.0;
    rgb.map(|value| ((value + m).clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0, 0];
    const GIF: &[u8] = b"GIF89a";
    const WEBP: &[u8] = b"RIFF\x00\x00\x00\x00WEBPVP8 ";

    #[test]
    fn sniffs_known_magic_bytes() {
        assert_eq!(sniff_extension(PNG), Some("png"));
        assert_eq!(sniff_extension(JPEG), Some("jpg"));
        assert_eq!(sniff_extension(GIF), Some("gif"));
        assert_eq!(sniff_extension(WEBP), Some("webp"));
        assert_eq!(sniff_extension(b"not an image"), None);
        assert_eq!(sniff_extension(&[]), None);
    }

    #[test]
    #[allow(clippy::case_sensitive_file_extension_comparisons)]
    fn cover_name_contains_id_and_content_hash() {
        let a = cover_file_name("abc", PNG).expect("png");
        let b = cover_file_name("abc", PNG).expect("png");
        let c = cover_file_name("abc", JPEG).expect("jpg");
        assert_eq!(a, b);
        assert!(a.starts_with("abc-"));
        assert!(a.ends_with(".png"));
        assert!(c.ends_with(".jpg"));
        assert_ne!(a.replace(".png", ""), c.replace(".jpg", ""));
        assert_eq!(cover_file_name("abc", b"nope"), None);
    }

    #[test]
    fn writes_and_removes_cover() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_cover(dir.path(), "id1", PNG).expect("written");
        assert!(dir.path().join(&file).exists());
        remove_cover_file(dir.path(), Some(&file));
        assert!(!dir.path().join(&file).exists());
        // 目录穿越文件名的调用必须被忽略
        remove_cover_file(dir.path(), Some("../evil.png"));
    }

    #[test]
    fn palette_is_deterministic() {
        let first = cover_palette("book-id");
        let second = cover_palette("book-id");
        assert_eq!(first, second);
        assert!((0.0..360.0).contains(&first.0));
        assert_ne!(first.1, first.2);
        assert_ne!(cover_palette("other-id").0, first.0);
    }

    #[test]
    fn relink_renames_cover_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_cover(dir.path(), "old", PNG).expect("written");
        let renamed =
            rename_cover_for_relink(dir.path(), "old", "new", Some(&file)).expect("renamed");
        assert!(renamed.starts_with("new-"));
        assert!(dir.path().join(&renamed).exists());
        assert!(!dir.path().join(&file).exists());
    }
}
