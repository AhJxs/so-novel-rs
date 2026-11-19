//! 阅读器核心：内容加载、分页排版、书内搜索。全部 UI 中立。

pub mod cache;
pub mod content;
pub mod layout;
pub mod search;

pub use content::{ReaderBook, ReaderChapter, ReaderError, load_book, load_book_relaxed};
pub use layout::{LayoutFingerprint, ParagraphMetrics, ReaderLayout, paginate};
pub use search::{HITS_PER_CHAPTER, HITS_TOTAL, SearchGroup, SearchHit, run as run_search};
