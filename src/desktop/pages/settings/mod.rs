//! 设置 page：用 gpui-kit 的 `Settings` 组件搭的一级导航页面。
//!
//! 保存机制（auto-save，无手动按钮）：每个 setter 改完字段后**立即**调
//! `model.persist_settings()` 写盘，没有单独的"立即保存"按钮。
//!
//! `NumberFieldOptions` 接 `f64`；对 `Option<i32>` 用 sentinel `-1.0` 表示"不限制"。

mod ctx;
mod fields;
mod page_about;
mod page_crawl;
mod page_general;
mod page_proxy;
mod settings_page;

pub use settings_page::SettingsPage;
