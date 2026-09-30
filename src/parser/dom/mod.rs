//! 选择器封装 + @js: 后处理 + HTML 转换  对应 Java `util.JsoupUtils`
//!
//! - [`selector`] — 选择 + 内容抽取 + `@js:` 后处理 + 极小 `XPath` 改写
//! - [`transform`] — `clear_all_attributes` / `remove_tags` 两种 HTML 转换
//!
//! `XPath` 只覆盖现有规则出现的两类 (`//*[@id=...]/script[N]` 和绝对路径 `/html` 系列)；
//! `@js:` 委托 `crate::js::post_process`；转换用正则清属性，不走 DOM API，否则 scraper 会重新包 `<html><body>`。

pub mod selector;
pub mod transform;

pub use selector::{
    SelectError, dom_select_text, select_and_invoke_js, select_and_invoke_js_within, split_js,
};
pub use transform::{clear_all_attributes, remove_tags};

// 重新导出 ContentType (实际定义在 models), 让 dom 模块的用户不用绕到 models。
pub use crate::models::ContentType;
