//! Search 页面：关键词搜索 + 书源过滤 + 结果列表 + 选章下载 Dialog。
//!
//! 两个大坑：书源下拉靠 render 差量同步；Dialog 的 OK 回调里不能直接开新 Dialog。

mod delegate;
mod detail_dialog;
mod range_dialog;
mod result_row;
mod search_page;
mod source_select;
mod toolbar;

pub use search_page::SearchPage;
