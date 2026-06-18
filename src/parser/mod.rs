//! HTML 解析层。dom 子模块提供 CSS 选择器 + @js: 后处理；search/book/toc/chapter
//! 等子模块各自对应一类页面解析，filter/formatter/search_filter 是结果后处理。

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
