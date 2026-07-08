//! 详情页解析 (PR #17 拆分, 2026-07-08). 对应 Java `parse.BookParser`.
//!
//! # 能力 (与 Java 端等价子集)
//!
//! - GET 详情页 (编码兜底已由 fetch 层完成);
//! - 检测 Cloudflare (命中返回 `BookError::Cloudflare`, 不在本阶段做旁路);
//! - bookName / author 必填, 否则报错;
//! - 其余字段 (intro / category / coverUrl / latestChapter / lastUpdateTime /
//!   status) 的字段查询字符串如果以 `meta[` 开头, 按 `ATTR_CONTENT` 抽, 否则按 `TEXT` 抽,
//!   与 Java `BookParser#getContentType` 等价;
//! - 选 coverUrl 时 attr_content 是相对路径的话, 用 `abs_url` 拼绝对 (Java 用 `absUrl`)。
//!
//! # 不在本模块 (后续阶段)
//!
//! - `CoverUpdater` (起点 cookie 取最新封面), 属阶段 4 / 阶段 5;
//! - 简繁转换 (属阶段 5);
//! - CF bypass 旁路 (属阶段 2c)。
//!
//! # 子模块
//!
//! - [`meta`] — `BookError` + `parse_book_detail` (主入口) + `parse_book_html` (离线测试)
//! - [`cover`] — 封面 URL 抽取 + CoverUpdater 集成

pub mod cover;
pub mod meta;

pub use cover::content_type_for;
pub use meta::{BookError, parse_book_detail, parse_book_html};
