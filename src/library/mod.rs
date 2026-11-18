//! 书架数据层：书籍模型、持久化、扫描、元数据、封面、查询。
//!
//! 不依赖任何 GUI 类型；`desktop` 层只通过本模块的公开 API 读写书架。

pub mod book;
pub mod query;

pub use book::{
    Book, BookFormat, BookOrigin, Bookmark, ReadingProgress, ShelfStatus, absolute_dir, hash64,
    id_for_key, mtime_secs, normalize_path_key, now_unix,
};
pub use query::{ShelfQuery, ShelfSort, ShelfStatusFilter, visible_books};

pub use crate::config::ReadingMode;
