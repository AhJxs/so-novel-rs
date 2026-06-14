//! HTML 解析层。当前阶段 2a 只暴露 dom 子模块（CSS 选择器 + @js: 后处理）。
//! 阶段 2b/2c 会在此目录下加 search/book/toc/chapter 等具体 parser。

pub mod book;
pub mod chapter;
pub mod dom;
pub mod filter;
pub mod formatter;
pub mod search;
pub mod search_filter;
pub mod search_quanben5;
pub mod toc;

pub use book::{parse_book_detail, BookError};
pub use chapter::{parse_chapter, ChapterError};
pub use dom::{
    clear_all_attributes, dom_select_text, remove_tags, select_and_invoke_js, ContentType,
    SelectError,
};
pub use filter::filter_chapter;
pub use formatter::format_chapter;
pub use search::{parse_search_results, search_one, SearchError};
pub use search_filter::filter_sort;
pub use toc::{parse_one_toc_page, parse_toc, TocError};
