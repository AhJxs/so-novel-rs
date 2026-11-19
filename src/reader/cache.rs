//! 整本书的解析缓存：`<下载目录>/.sonovel/books/<书 id>.json`。
//!
//! 打开一本书本来要整本解码 + 切章（实测 EPUB ≈0.9 s、TXT ≈0.6 s），命中缓存时只剩
//! 反序列化。文件按书籍 id（路径哈希，稳定）命名，命中条件见 [`load`]：大小 / 修改时间 /
//! 格式 / 编码请求 / 缓存版本全部对得上才用。目录随小说目录一起删，删书时调 [`remove`]。

use std::path::{Path, PathBuf};
use std::{fs, io};

use serde::{Deserialize, Serialize};

use super::ReaderBook;
/// 缓存根目录名（`<下载目录>/.sonovel`），与封面缓存共用。
pub use crate::library::CACHE_DIR_NAME;
use crate::library::{BOOKS_DIR_NAME, BookFormat, mtime_secs};
/// 缓存格式版本；解析行为或字段变了就 +1，旧缓存自动失效。
pub const CACHE_VERSION: u32 = 1;

/// `<下载目录>/.sonovel`；下载目录为空时返回 `None`（不缓存）。
///
/// 路径规则与封面缓存共用 [`crate::library::cache_root`]，两处不能各算一套。
#[must_use]
pub fn dir_for_download(download_path: &str) -> Option<PathBuf> {
    crate::library::cache_root(download_path)
}

/// 缓存文件：解析结果 + 原文件的校验信息。
#[derive(Deserialize)]
struct CacheFile {
    version: u32,
    format: BookFormat,
    size_bytes: u64,
    mtime: i64,
    /// 生成缓存时**请求**的编码（`None` = 自动探测）。它与 `book.encoding`（实际结果）
    /// 分开存：首次自动探测成功后书架数据会记下结果，下次带着标签来打开，
    /// 只看结果字段会永远判 miss。
    requested_encoding: Option<String>,
    book: ReaderBook,
}

/// 写入用的借用版本：缓存动辄几 MB，序列化时不为它整本 clone。
#[derive(Serialize)]
struct CacheRef<'a> {
    version: u32,
    format: BookFormat,
    size_bytes: u64,
    mtime: i64,
    requested_encoding: Option<&'a str>,
    book: &'a ReaderBook,
}

/// 读缓存；任一校验不通过就返回 `None`，调用方按原流程解析并调用 [`store`] 覆盖。
#[must_use]
pub fn load(
    dir: &Path,
    book_id: &str,
    path: &Path,
    format: BookFormat,
    requested: Option<&str>,
) -> Option<ReaderBook> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let raw = fs::read_to_string(book_path(dir, book_id)).ok()?;
    let cached: CacheFile = serde_json::from_str(&raw).ok()?;
    let fresh = cached.version == CACHE_VERSION
        && cached.format == format
        && cached.size_bytes == metadata.len()
        && cached.mtime == mtime_secs(&metadata)
        && encoding_matches(requested, &cached);
    fresh.then_some(cached.book)
}

/// 解析成功后写缓存；失败只记日志，绝不能影响阅读。
pub fn store(
    dir: &Path,
    book_id: &str,
    path: &Path,
    format: BookFormat,
    requested: Option<&str>,
    book: &ReaderBook,
) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if !metadata.is_file() {
        return;
    }
    let payload = CacheRef {
        version: CACHE_VERSION,
        format,
        size_bytes: metadata.len(),
        mtime: mtime_secs(&metadata),
        requested_encoding: requested,
        book,
    };
    if let Err(error) = write_json(dir, book_id, &payload) {
        tracing::warn!(
            "写入阅读缓存失败 dir={} book={book_id}: {error}",
            dir.display()
        );
    }
}

/// 删掉一本书的缓存（删文件 / 移出书架 / 重新定位后旧 id 都走这里）。
pub fn remove(dir: &Path, book_id: &str) {
    let _ = fs::remove_file(book_path(dir, book_id));
}

/// 请求编码与缓存是否等价。
///
/// - 完全相同 → 命中；
/// - 缓存是自动探测、这次请求的正是当次探测出的编码 → 等价（见 `requested_encoding`）；
/// - 其余（换了编码、或从显式切回自动）都要重解析。
fn encoding_matches(requested: Option<&str>, cached: &CacheFile) -> bool {
    match (requested, cached.requested_encoding.as_deref()) {
        (this, that) if this == that => true,
        (Some(label), None) => cached.book.encoding.as_deref() == Some(label),
        _ => false,
    }
}

fn book_path(dir: &Path, book_id: &str) -> PathBuf {
    dir.join(BOOKS_DIR_NAME)
        .join(format!("{}.json", safe_file_stem(book_id)))
}

/// 缓存文件名只允许 ASCII 字母、数字、`-`、`_`；书架数据可能被手工改过，
/// 带路径分隔符的 id 不能拼进路径（防目录穿越）。
fn safe_file_stem(book_id: &str) -> String {
    book_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

fn write_json(dir: &Path, book_id: &str, payload: &CacheRef<'_>) -> io::Result<()> {
    let path = book_path(dir, book_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    crate::library::hide_dir(dir);
    let json = serde_json::to_vec(payload).map_err(io::Error::other)?;
    // 先写临时文件再改名：写到一半崩掉也不会留下半截 JSON 被下次当成有效缓存。
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::reader::ReaderChapter;

    fn sample_book(encoding: Option<&str>) -> ReaderBook {
        ReaderBook {
            title: "斗破苍穹".to_string(),
            author: "天蚕土豆".to_string(),
            chapters: vec![ReaderChapter {
                title: "第一章".to_string(),
                paragraphs: vec!["正文一".to_string(), "正文二".to_string()],
            }],
            encoding: encoding.map(str::to_string),
        }
    }

    fn write_source(dir: &Path) -> PathBuf {
        let path = dir.join("book.txt");
        std::fs::write(&path, "第一章\n正文一\n正文二").unwrap();
        path
    }

    #[test]
    fn roundtrip_returns_cached_book() {
        let dir = tempfile::tempdir().unwrap();
        let book = sample_book(Some("GBK"));
        let source = write_source(dir.path());
        let cache = dir.path().join(CACHE_DIR_NAME);

        store(
            &cache,
            "abc123",
            &source,
            BookFormat::Txt,
            Some("GBK"),
            &book,
        );
        let loaded = load(&cache, "abc123", &source, BookFormat::Txt, Some("GBK")).expect("命中");
        assert_eq!(loaded.title, book.title);
        assert_eq!(loaded.chapters.len(), 1);
        assert_eq!(loaded.chapters[0].paragraphs, vec!["正文一", "正文二"]);
    }

    #[test]
    fn changed_source_invalidates_cache() {
        let dir = tempfile::tempdir().unwrap();
        let source = write_source(dir.path());
        let cache = dir.path().join(CACHE_DIR_NAME);
        store(
            &cache,
            "abc123",
            &source,
            BookFormat::Txt,
            None,
            &sample_book(None),
        );

        // 内容变了（大小 + mtime 都会变）。
        std::fs::write(&source, "第一章\n新的正文，比原来长很多").unwrap();
        assert!(
            load(&cache, "abc123", &source, BookFormat::Txt, None).is_none(),
            "原文件变了必须重新解析"
        );
    }

    #[test]
    fn explicit_encoding_from_auto_detected_cache_still_hits() {
        let dir = tempfile::tempdir().unwrap();
        let source = write_source(dir.path());
        let cache = dir.path().join(CACHE_DIR_NAME);
        // 首次自动探测，记下 GBK。
        store(
            &cache,
            "abc123",
            &source,
            BookFormat::Txt,
            None,
            &sample_book(Some("GBK")),
        );
        assert!(
            load(&cache, "abc123", &source, BookFormat::Txt, Some("GBK")).is_some(),
            "第二次带着探测结果来打开也算命中"
        );
        assert!(
            load(&cache, "abc123", &source, BookFormat::Txt, Some("UTF-8")).is_none(),
            "换编码必须重新解析"
        );
        assert!(
            load(&cache, "abc123", &source, BookFormat::Txt, None).is_some(),
            "请求与缓存都是自动探测"
        );
    }

    #[test]
    fn explicit_cache_is_not_reused_for_auto_requests() {
        let dir = tempfile::tempdir().unwrap();
        let source = write_source(dir.path());
        let cache = dir.path().join(CACHE_DIR_NAME);
        store(
            &cache,
            "abc123",
            &source,
            BookFormat::Txt,
            Some("UTF-8"),
            &sample_book(Some("UTF-8")),
        );
        assert!(
            load(&cache, "abc123", &source, BookFormat::Txt, None).is_none(),
            "显式解码的结果不能冒充自动探测"
        );
    }

    #[test]
    fn corrupt_cache_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let source = write_source(dir.path());
        let cache = dir.path().join(CACHE_DIR_NAME);
        let path = book_path(&cache, "abc123");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(load(&cache, "abc123", &source, BookFormat::Txt, None).is_none());
    }

    #[test]
    fn remove_deletes_only_that_book() {
        let dir = tempfile::tempdir().unwrap();
        let source = write_source(dir.path());
        let cache = dir.path().join(CACHE_DIR_NAME);
        store(
            &cache,
            "aaa",
            &source,
            BookFormat::Txt,
            None,
            &sample_book(None),
        );
        store(
            &cache,
            "bbb",
            &source,
            BookFormat::Txt,
            None,
            &sample_book(None),
        );

        remove(&cache, "aaa");
        assert!(load(&cache, "aaa", &source, BookFormat::Txt, None).is_none());
        assert!(load(&cache, "bbb", &source, BookFormat::Txt, None).is_some());
    }

    #[test]
    fn path_separators_in_book_id_cannot_escape_cache_dir() {
        let dir = tempfile::tempdir().unwrap();
        let source = write_source(dir.path());
        let cache = dir.path().join(CACHE_DIR_NAME);
        store(
            &cache,
            "../evil",
            &source,
            BookFormat::Txt,
            None,
            &sample_book(None),
        );
        assert!(
            !dir.path().join("evil.json").exists(),
            "带 .. 的 id 不能写到缓存目录以外"
        );
        assert!(
            load(&cache, "../evil", &source, BookFormat::Txt, None).is_some(),
            "同一 id 读写用同一份文件名"
        );
    }

    #[test]
    fn empty_download_path_has_no_cache_dir() {
        assert!(dir_for_download("   ").is_none());
        let dir = dir_for_download("C:/books").expect("有目录");
        assert!(dir.ends_with(CACHE_DIR_NAME));
    }
}
