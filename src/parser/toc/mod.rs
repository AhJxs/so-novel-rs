//! 目录解析  对应 Java `parse.TocParser`.
//!
//! - 单页目录直接抽 `toc.item`;
//! - 分页两种模式：**下拉菜单** (`nextPage` 命中带 `value`/`href` 的元素，一次性取出
//!   所有分页 URL) 与**下一页按钮** (递归抓页，直到拿不到合法 URL);
//! - `isDesc=true` 倒序枚举; `Book.url` 正则提取书 ID 填入 `toc.url` / `toc.baseUri`
//!   模板 (`%s`); 章节 `title` 走 text、`url` 走 absUrl;
//! - 分页不并行抓取; [`single`] 是主入口, [`paginated`] 收集分页 URL, [`utils`] 放 `TocError` 与工具函数。

pub mod paginated;
pub mod single;
pub mod utils;

pub use single::{parse_one_toc_page, parse_toc};
pub use utils::TocError;
