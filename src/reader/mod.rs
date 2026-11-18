//! 阅读器核心：内容加载、分页排版、书内搜索。全部 UI 中立。

pub mod content;
pub mod layout;

pub use content::{ReaderBook, ReaderChapter, ReaderError, load_book};
pub use layout::{LayoutFingerprint, ParagraphMetrics, ReaderLayout, paginate};
