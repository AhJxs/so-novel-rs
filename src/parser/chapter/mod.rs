//! 单章正文解析，对应 Java `parse.ChapterParser`。
//!
//! 单页按 `chapter.content` 取 HTML；分页循环抓 → 拼接，下一页 URL 先看 `nextPageInJs`
//! （`select_and_invoke_js` 执行 script 取 URL），否则按 `chapter.nextPage` 取 `first.href`；
//! `nextChapterLink` 命中正则说明已跳到下一章，兜底是 URL 不像分页
//! （`!matches(".*[-_]\\d\\.html")`）且下一页元素文本含 `下一章/没有了/>>/书末页`。
//! CF 命中走 cf-bypass 兜底；正文清洗 / 重试 / 简繁转换归 filter、formatter 与调度层。
//!
//! [`parse`] 是公共入口 + 离线解析；[`pagination`] 管分页循环与终止判定。

pub mod pagination;
pub mod parse;

pub use parse::{ChapterError, parse_chapter, parse_chapter_html};
