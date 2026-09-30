//! 详情页解析，对应 Java `parse.BookParser`。
//!
//! GET 详情页（编码兜底由 fetch 层完成）；命中 Cloudflare 返回 `BookError::Cloudflare`，
//! 旁路需外部配置；bookName / author 必填；字段查询串以 `meta[` 开头按 `ATTR_CONTENT` 抽，
//! 否则按 `TEXT` 抽；coverUrl 相对路径用 `abs_url` 拼绝对。
//!
//! [`meta`] 是主入口 + 离线解析; [`cover`] 管封面 URL 抽取与 `CoverUpdater` 集成。

pub mod cover;
pub mod meta;

pub use cover::content_type_for;
pub use meta::{BookError, parse_book_detail, parse_book_html};
