//! 书架数据层：书籍模型、持久化、扫描、元数据、封面、查询。
//!
//! 不依赖任何 GUI 类型；`desktop` 层只通过本模块的公开 API 读写书架。

pub mod book;
pub mod cover;
pub mod meta;
pub mod paths;
pub mod query;
pub mod scan;
pub mod store;

pub use book::{
    Book, BookFormat, BookOrigin, Bookmark, ReadingProgress, ShelfStatus, absolute_dir, hash64,
    id_for_key, mtime_secs, normalize_path_key, now_unix,
};
pub use cover::{
    cover_file_name, cover_palette, remove_cover_file, rename_cover_for_relink, write_cover,
};
pub use meta::{ExtractedMeta, extract};
pub use paths::{
    BOOKS_DIR_NAME, CACHE_DIR_NAME, COVERS_DIR_NAME, cache_root, covers_dir_for, hide_dir,
};
pub use query::{ShelfQuery, ShelfSort, ShelfStatusFilter, visible_books};
pub use scan::{ScannedFile, scan_download_dir};
pub use store::{AppliedMeta, LIBRARY_VERSION, LibraryStore, path_key_is_within};

pub use crate::config::ReadingMode;
