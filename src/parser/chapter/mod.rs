//! 单章正文解析  对应 Java `parse.ChapterParser`。
//!
//! # 能力 (与 Java 端等价子集)
//!
//! - 单页：抓一页，按 `chapter.content` 取 HTML 字符串；
//! - 分页：循环抓 → 拼接，下一页 URL 顺序：
//!   1. 配了 `nextPageInJs` → 用 `select_and_invoke_js` 从某段 script
//!      内容里执行 JS 抽取 URL；
//!   2. 否则按 `chapter.nextPage` 选元素，取 `first.href`；
//! - 终止条件：
//!   - `nextChapterLink` 配置且 candidate 命中正则 → 终止（说明已经跳到下一章）；
//!   - 兜底：URL 不像分页（`!matches(".*[-_]\\d\\.html")`）且下一页元素文本含
//!     `下一章/没有了/>>/书末页` → 终止；
//! - CF 命中 → 走 cf-bypass 兜底。
//!
//! **不在本阶段做**：
//! - 正文清洗（filterTxt 正则替换、filterTag 节点删除、不可见字符清理、HTML 实体清理、
//!   重复标题去除、HTML 模板渲染）— 全部归阶段 3 `ChapterFilter` + `ChapterFormatter`。
//! - 重试（配置在 `enable-retry`）— 归阶段 3 调度层。
//! - 简繁转换 — 归阶段 5。
//!
//! # 子模块
//!
//! - [`parse`] — `ChapterError` + `parse_chapter` (公共异步入口) + `parse_chapter_html` (离线测试) +
//!   单页抓取 + cf-fallback typed-error 包装
//! - [`pagination`] — 分页循环 + 终止判定 + `NextStep` + `resolve_next_url` / `is_last_page`

pub mod pagination;
pub mod parse;

pub use parse::{ChapterError, parse_chapter, parse_chapter_html};
