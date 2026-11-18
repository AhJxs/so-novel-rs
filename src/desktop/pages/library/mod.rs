//! Library 页面：本地书库（卡片书架）。
//!
//! - 卡片网格手写（`flex_wrap`），不用 `List`：虚拟列表要求等高行，与封面网格对不上。
//! - 扫描 / 元数据提取在后台线程，drain 循环回传后自动重绘。
//! - 卡片点击通过 `RootView` 注入的回调打开阅读器。

mod card;
mod library_page;

pub use library_page::LibraryPage;
