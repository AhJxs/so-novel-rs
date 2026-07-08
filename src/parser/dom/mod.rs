//! 选择器封装 + @js: 后处理 + HTML 转换 (PR #17 拆分, 2026-07-08).
//!
//! 来自原 `parser/dom.rs` (581 LOC), 拆为两个职责:
//!
//! - [`selector`] — 选择 + 内容抽取 + `@js:` 后处理 + 极小 XPath 改写
//! - [`transform`] — `clear_all_attributes` / `remove_tags` 两种 HTML 转换
//!
//! 对应 Java `util.JsoupUtils`:
//! - 选择器: CSS 用 `scraper` (html5ever); XPath 走极小改写, 覆盖现有规则
//!   出现的两类 (`//*[@id=...]/script[N]` 和纯绝对路径 `/html` 系列)
//! - `@js:` 后处理: 委托 `crate::js::post_process`
//! - 转换: 用正则清属性 (不走 DOM API, 避免 scraper 重新包 `<html><body>`)

pub mod selector;
pub mod transform;

pub use selector::{SelectError, dom_select_text, select_and_invoke_js, select_and_invoke_js_within, split_js};
pub use transform::{clear_all_attributes, remove_tags};

// 重新导出 ContentType (实际定义在 models), 让 dom 模块的用户不用绕到 models。
pub use crate::models::ContentType;
