//! 书籍模型：格式 / 来源 / 阅读进度 / 书签，以及路径规范化与稳定 ID。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use rust_i18n::t;
use serde::{Deserialize, Serialize};

use crate::config::ReadingMode;

/// 库内书籍标识与元数据。字段顺序即 `library.json` 的键顺序。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShelfBook {
    /// 稳定 ID：`path_key` 的 FNV-1a 64 位哈希（16 hex），用于封面文件名与 UI 键。
    #[serde(default)]
    pub id: String,
    /// 规范化查找键：绝对路径，Windows 下转小写；不展示给用户。
    #[serde(default)]
    pub path_key: String,
    /// 展示用绝对路径（保留用户原始大小写）。
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub format: BookFormat,
    #[serde(default)]
    pub origin: BookOrigin,
    /// 所属扫描根（规范化路径键）；Imported 且不在任何扫描根内时为 None。
    #[serde(default)]
    pub source_root: Option<String>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub intro: Option<String>,
    /// 封面缓存文件名（相对 `<下载目录>/.sonovel/covers`，形如 `<id>-<内容哈希>.<ext>`）；None = 生成封面。
    #[serde(default)]
    pub cover_file: Option<String>,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub file_mtime: i64,
    #[serde(default)]
    pub added_unix: i64,
    #[serde(default)]
    pub last_opened_unix: Option<i64>,
    /// 用户手动改过元数据 → 扫描时不再自动覆盖 title / author / cover。
    #[serde(default)]
    pub metadata_edited: bool,
    /// 从书架移除（仅对扫描来源生效，扫描不会再加回）。
    #[serde(default)]
    pub hidden: bool,
    /// 文件当前不存在。运行时由扫描更新，持久化便于离线展示。
    #[serde(default)]
    pub missing: bool,
    /// TXT 编码覆盖；None = 自动探测。
    #[serde(default)]
    pub encoding: Option<String>,
    /// 首次成功打开后写入。
    #[serde(default)]
    pub chapter_count: Option<u32>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub progress: ReadingProgress,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}

impl ShelfBook {
    /// 由磁盘文件构造新记录。
    ///
    /// # Errors
    ///
    /// 文件不存在 / 不是普通文件 / 后缀不支持时返回可直接进 toast 的文案：
    /// 后两种走 t! 本地化，IO 失败透传系统原始文案。
    #[tracing::instrument(name = "library::ShelfBook::from_file", skip_all, fields(path = %path.display()))]
    pub fn from_file(path: &Path, origin: BookOrigin) -> Result<Self, String> {
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err(t!("Library.error.not_a_file").to_string());
        }
        let format = BookFormat::from_path(path)
            .ok_or_else(|| t!("Library.error.unsupported_format").to_string())?;
        let path_key = normalize_path_key(path);
        Ok(Self {
            id: id_for_key(&path_key),
            path_key,
            path: display_path(path),
            format,
            origin,
            source_root: None,
            title: file_stem(path),
            author: String::new(),
            intro: None,
            cover_file: None,
            size_bytes: metadata.len(),
            file_mtime: mtime_secs(&metadata),
            added_unix: now_unix(),
            last_opened_unix: None,
            metadata_edited: false,
            hidden: false,
            missing: false,
            encoding: None,
            chapter_count: None,
            favorite: false,
            progress: ReadingProgress::default(),
            bookmarks: Vec::new(),
        })
    }

    /// 设置所属扫描根（规范化键），由扫描合并调用。
    #[must_use]
    pub fn with_source_root(mut self, root: &Path) -> Self {
        self.source_root = Some(normalize_path_key(root));
        self
    }

    /// 派生状态（不持久化）。
    #[must_use]
    pub const fn status(&self) -> ShelfStatus {
        if self.progress.finished {
            ShelfStatus::Finished
        } else if self.progress.updated_unix > 0 {
            ShelfStatus::Reading
        } else {
            ShelfStatus::Unread
        }
    }

    /// 状态文案 key（`Library.status.*`），由 UI 层翻译。
    #[must_use]
    pub const fn status_key(&self) -> &'static str {
        match self.status() {
            ShelfStatus::Unread => "Library.status.unread",
            ShelfStatus::Reading => "Library.status.reading",
            ShelfStatus::Finished => "Library.status.finished",
        }
    }
}

/// 书籍格式。其他格式首版只展示、不在应用内阅读。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BookFormat {
    /// 纯文本。
    #[default]
    Txt,
    /// EPUB 电子书。
    Epub,
    /// Markdown。
    Markdown,
    /// HTML / HTM。
    Html,
    /// ZIP 打包的 HTML。
    Zip,
    /// PDF。
    Pdf,
}

impl BookFormat {
    /// `txt` / `epub` / `md|markdown` / `html|htm` / `zip` / `pdf`；未知后缀返回 `None`。
    ///
    /// 书架扫描 / 导入 / 重新定位共用这一处：这是「哪些后缀算一本书」的唯一事实来源
    /// （`htm` / `markdown` 是别名，同样收进书架）。
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "txt" => Some(Self::Txt),
            "epub" => Some(Self::Epub),
            "md" | "markdown" => Some(Self::Markdown),
            "html" | "htm" => Some(Self::Html),
            "zip" => Some(Self::Zip),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }

    /// 取路径后缀并按 [`Self::from_extension`] 解析。
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension().and_then(|value| value.to_str())?;
        Self::from_extension(ext)
    }

    /// 首版只有 TXT / EPUB 能在应用内阅读。
    #[must_use]
    pub const fn is_readable(self) -> bool {
        matches!(self, Self::Txt | Self::Epub)
    }

    /// 徽标与文件对话框过滤用的标签（技术名词，不翻译）。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Txt => "TXT",
            Self::Epub => "EPUB",
            Self::Markdown => "MD",
            Self::Html => "HTML",
            Self::Zip => "ZIP",
            Self::Pdf => "PDF",
        }
    }

    /// 文件对话框过滤用的扩展名（无点、小写）。
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Epub => "epub",
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Zip => "zip",
            Self::Pdf => "pdf",
        }
    }
}

/// 书籍来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BookOrigin {
    /// 下载目录扫描到的文件。
    #[default]
    Downloaded,
    /// 用户从任意位置导入（只记录路径，不复制文件）。
    Imported,
}

/// 书架派生状态（不持久化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShelfStatus {
    /// 从未打开。
    Unread,
    /// 读过但没读完。
    Reading,
    /// 已读完。
    Finished,
}

/// 阅读进度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingProgress {
    /// 0-based 章节下标。
    #[serde(default)]
    pub chapter: usize,
    /// 章内相对进度 0.0..=1.0。滚动 = `offset/max_offset`；翻页 = `page_index/(page_count-1)`。
    #[serde(default)]
    pub chapter_ratio: f32,
    /// 全书百分比 0..=100；`chapter_count` 未知时为 0。
    #[serde(default)]
    pub percent: f32,
    /// 上次阅读模式，重开时优先恢复。
    #[serde(default)]
    pub mode: ReadingMode,
    #[serde(default)]
    pub updated_unix: i64,
    #[serde(default)]
    pub finished: bool,
}

impl Default for ReadingProgress {
    fn default() -> Self {
        Self {
            chapter: 0,
            chapter_ratio: 0.0,
            percent: 0.0,
            mode: ReadingMode::Scroll,
            updated_unix: 0,
            finished: false,
        }
    }
}

/// 章节内书签。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// `<chapter>-<毫秒时间戳>`。
    #[serde(default)]
    pub id: String,
    /// 0-based 章节下标。
    #[serde(default)]
    pub chapter: usize,
    /// 章内相对进度。
    #[serde(default)]
    pub chapter_ratio: f32,
    /// 添加时当前段落前 40 个字符。
    #[serde(default)]
    pub excerpt: String,
    #[serde(default)]
    pub created_unix: i64,
}

/// 上一个书签 ID 用到的毫秒数。同一毫秒内连续创建要单调 +1，否则同章两个书签会撞 ID，
/// 按 ID 删除时会把两个一起删掉。
static LAST_BOOKMARK_MILLIS: AtomicU64 = AtomicU64::new(0);

impl Bookmark {
    /// 生成唯一 ID（`<chapter>-<毫秒时间戳>`）：毫秒值单调递增，同一毫秒内连续创建时
    /// 等于上一次 +1，其余时候等于当前时间。
    #[must_use]
    pub fn new(chapter: usize, chapter_ratio: f32, excerpt: String) -> Self {
        let now_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
            });
        let millis = LAST_BOOKMARK_MILLIS
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
                Some(last.saturating_add(1).max(now_millis))
            })
            .map_or(now_millis, |previous| {
                previous.saturating_add(1).max(now_millis)
            });
        Self {
            id: format!("{chapter}-{millis}"),
            chapter,
            chapter_ratio,
            excerpt,
            created_unix: now_unix(),
        }
    }
}

/// FNV-1a 64 位哈希：稳定、跨平台、无外部依赖。
#[must_use]
pub fn hash64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// `path_key` → 16 位小写 hex 的稳定 ID。
#[must_use]
pub fn id_for_key(path_key: &str) -> String {
    format!("{:016x}", hash64(path_key.as_bytes()))
}

/// 规范化路径键：优先 `canonicalize`，失败退回「绝对路径」，Windows 下统一反斜杠并转小写。
///
/// `canonicalize` 在 Windows 输出反斜杠，而退回的绝对路径会保留用户输入里的 `/`；
/// 两者必须归一，否则同一文件会因写法不同算出不同 ID。
#[must_use]
pub fn normalize_path_key(path: &Path) -> String {
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| absolute_path(path));
    let text = absolute.to_string_lossy();
    if cfg!(windows) {
        text.replace('/', "\\").to_lowercase()
    } else {
        text.into_owned()
    }
}

/// 当前 Unix 秒。
#[must_use]
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().cast_signed())
}

/// 文件修改时间（Unix 秒）；读不到时为 0。
#[must_use]
pub fn mtime_secs(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs().cast_signed())
}

/// 把可能是相对路径的目录转成绝对路径；**不** `canonicalize`（目录可能还不存在，
/// 而 `canonicalize` 对不存在的路径会失败）。
#[must_use]
pub fn absolute_dir(path: &Path) -> PathBuf {
    absolute_path(path)
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
}

fn display_path(path: &Path) -> String {
    absolute_path(path).to_string_lossy().into_owned()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn format_maps_extensions() {
        assert_eq!(BookFormat::from_extension("EPUB"), Some(BookFormat::Epub));
        assert_eq!(
            BookFormat::from_extension("markdown"),
            Some(BookFormat::Markdown)
        );
        assert_eq!(BookFormat::from_extension("htm"), Some(BookFormat::Html));
        assert_eq!(BookFormat::from_extension("docx"), None);
        assert!(BookFormat::Epub.is_readable());
        assert!(BookFormat::Txt.is_readable());
        assert!(!BookFormat::Pdf.is_readable());
    }

    #[test]
    fn extension_matches_format() {
        assert_eq!(BookFormat::Txt.extension(), "txt");
        assert_eq!(BookFormat::Epub.extension(), "epub");
        assert_eq!(BookFormat::Markdown.extension(), "md");
        assert_eq!(BookFormat::Html.extension(), "html");
        assert_eq!(BookFormat::Zip.extension(), "zip");
        assert_eq!(BookFormat::Pdf.extension(), "pdf");
    }

    #[test]
    fn id_is_stable_and_depends_on_key() {
        assert_eq!(id_for_key("c:/books/a.txt"), id_for_key("c:/books/a.txt"));
        assert_ne!(id_for_key("c:/books/a.txt"), id_for_key("c:/books/b.txt"));
        assert_eq!(id_for_key("c:/books/a.txt").len(), 16);
    }

    #[test]
    fn path_key_lowercases_only_on_windows() {
        // 用真实存在的临时文件：既走 canonicalize 成功分支，又不依赖 C:\Books 是否存在。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Novel.TXT");
        std::fs::write(&path, b"x").unwrap();
        let key = normalize_path_key(&path);
        if cfg!(windows) {
            assert_eq!(key, key.to_lowercase());
            assert!(key.ends_with("novel.txt"));
        } else {
            assert!(key.ends_with("Novel.TXT"));
        }
        assert_eq!(key, normalize_path_key(&path));
    }

    #[test]
    fn absolute_dir_keeps_absolute_and_joins_relative() {
        let absolute = if cfg!(windows) {
            Path::new("C:/books")
        } else {
            Path::new("/books")
        };
        assert_eq!(absolute_dir(absolute), absolute);
        let relative = absolute_dir(Path::new("books"));
        assert!(relative.is_absolute(), "相对路径必须转成绝对路径");
        assert!(relative.ends_with("books"));
    }

    #[test]
    fn from_file_fills_metadata_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("斗破苍穹.txt");
        std::fs::write(&path, "第一章\n正文").unwrap();

        let book = ShelfBook::from_file(&path, BookOrigin::Imported).expect("book");
        assert_eq!(book.title, "斗破苍穹");
        assert_eq!(book.format, BookFormat::Txt);
        assert_eq!(book.origin, BookOrigin::Imported);
        assert_eq!(book.size_bytes, "第一章\n正文".len() as u64);
        assert_eq!(book.id, id_for_key(&book.path_key));
        assert_eq!(book.status(), ShelfStatus::Unread);
        assert!(!book.missing);
        assert!(!book.favorite);
    }

    #[test]
    fn from_file_rejects_unsupported_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.docx");
        std::fs::write(&path, b"x").unwrap();
        assert!(ShelfBook::from_file(&path, BookOrigin::Downloaded).is_err());
    }

    #[test]
    fn status_follows_progress() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.epub");
        std::fs::write(&path, b"x").unwrap();
        let mut book = ShelfBook::from_file(&path, BookOrigin::Downloaded).expect("book");

        assert_eq!(book.status(), ShelfStatus::Unread);
        book.progress.updated_unix = 10;
        book.progress.percent = 12.0;
        assert_eq!(book.status(), ShelfStatus::Reading);
        book.progress.finished = true;
        assert_eq!(book.status(), ShelfStatus::Finished);
        assert_eq!(book.status_key(), "Library.status.finished");
    }

    #[test]
    fn bookmark_ids_are_unique_per_creation() {
        let first = Bookmark::new(2, 0.5, "摘录".to_string());
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = Bookmark::new(2, 0.5, "摘录".to_string());
        assert_eq!(first.chapter, 2);
        assert_ne!(first.id, second.id);
        assert!(first.created_unix > 0);
    }

    #[test]
    fn bookmark_ids_are_unique_within_one_millisecond() {
        // 不 sleep：32 次连续创建必然落在同一毫秒内，只有单调计数能保证 ID 互不相同。
        let ids: std::collections::HashSet<_> = (0..32)
            .map(|_| Bookmark::new(3, 0.5, String::new()).id)
            .collect();
        assert_eq!(ids.len(), 32, "同一毫秒内连续创建的书签 ID 必须互不相同");
    }
}
