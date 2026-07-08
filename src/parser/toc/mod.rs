//! 目录解析  对应 Java `parse.TocParser`.
//!
//! # 能力 (与 Java 端等价子集)
//!
//! - 单页目录直接抽 `toc.item`;
//! - 分页目录两种模式:
//!   1. **下拉菜单** (option/select): `nextPage` 命中带 `value`/`href` 属性的元素,
//!      一次性取出所有分页 URL;
//!   2. **下一页按钮** (递归): 每页抓一次, 按 `nextPage` 选择器找下一个,
//!      直到拿不到合法 URL;
//! - `isDesc=true` 倒序枚举 (69shuba);
//! - `Book.url` 正则提取书 ID 并填入 `toc.url` / `toc.baseUri` 模板 (`%s`);
//! - 章节 `title` 走 text、`url` 走 absUrl。
//!
//! # 不在本模块 (属阶段 3 / 后续)
//!
//! - 多线程并行抓取分页 (Java 的 `parseToc` TODO 同样未做)
//! - `chapter.url` 段含 `@js:` 后处理 (极少见)
//!
//! # 子模块
//!
//! - [`single`] — `parse_toc` 主入口 + `parse_one_toc_page` 单页抽取
//! - [`paginated`] — 分页 URL 收集 (option / 递归翻页)
//! - [`utils`] — `TocError` + 工具函数 (`extract_book_id` / `format_with_id` / `resolve_base_for_join`)

pub mod paginated;
pub mod single;
pub mod utils;

pub use single::{parse_one_toc_page, parse_toc};
pub use utils::TocError;
