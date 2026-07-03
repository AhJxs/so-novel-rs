//! so-novel-rs — Rust 桌面客户端（GPUI + gpui-component，egui 已完全移除）。
//!
//! 模块划分：
//! - `gpui_app` — 新 GPUI GUI 入口（Stage 1+）。
//! - `app` / `db` / `crawler` / `config` / `models` / `parser` / `export` /
//!   `http` / `js` / `util` / `cli` — 业务 + 数据层（GUI 解耦）。

// `rust_i18n::i18n!` 必须在 crate root 调一次 —— 它在 crate root 生成 `_rust_i18n_t`
// 函数 + locale 表，`t!` 宏和 `rust_i18n::set_locale` 都依赖它。
// gpui-component 在它自己的 crate root 也调了一次（加载 `locales/ui.yml`），两套 i18n
// 实例各管各的 key 表，但**全局 locale 共享**（同一 `CURRENT_LOCALE`）。
//
// 注：rust-i18n 3.1.5 的 `fallback = "en"` 参数生成的 `static _RUST_I18N_FALLBACK_LOCALE`
// 实际是 `Some(&[...])`，里面的 `&[...]` 是临时栈数组，触发 E0716 "temporary value
// dropped while borrowed"。我们不传 fallback —— 缺翻译时 rust_i18n 默认返回 key 字符串
// 本身（开发期可见漏翻译），生产体验等价。
rust_i18n::i18n!("locales");

pub mod app;
pub mod cli;
pub mod config;
pub mod crawler;
pub mod export;
#[cfg(feature = "gui")]
pub mod gpui_app;
pub mod http;
pub mod i18n;
pub mod js;
pub mod logging;
pub mod models;
pub mod parser;
pub mod persistent;
pub mod util;
#[cfg(feature = "web")]
pub mod web;
