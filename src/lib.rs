//! so-novel-rs — Rust 桌面客户端（GPUI + gpui-component，egui 已完全移除）。
//!
//! 模块划分：
//! - `gpui_app` — 新 GPUI GUI 入口（Stage 1+）。
//! - `app` / `db` / `crawler` / `config` / `models` / `parser` / `export` /
//!   `http` / `js` / `utils` / `cli` — 业务 + 数据层（GUI 解耦）。
//!
//! ## 工程规约
//!
//! - `unsafe_code = "forbid"`（`Cargo.toml [lints.rust]`）：仓库内禁止 `unsafe`。
//!   如确需启用, 必须先开 RFC 评审。
//! - `missing_docs = "warn"`（`Cargo.toml [lints.rust]`）：所有 `pub` 项必须有
//!   `///` 文档注释。`#[allow(missing_docs)]` 需在调用点写明原因。
//! - clippy pedantic + nursery 触发: 一次性 PR 收敛 `mut` 多余、`clone()` 多余、
//!   `must_use` 缺失等; 见 `Cargo.toml [lints.clippy]`。
//! - 错误体系: 领域错误 (`ExportError`/`WebError`/...) 保留在领域内, 通过
//!   `From` 归一到 `crate::error::AppError`; 二进制入口 (`main.rs`) 允许用
//!   `anyhow`。
//! - 工具: `crate::utils::*` (rename 自旧 `util/`, 2026-07-08 PR #1)。

#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

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
pub mod db;
pub mod error;
pub mod export;
#[cfg(feature = "gui")]
pub mod gpui_app;
pub mod http;
pub mod i18n;
pub mod js;
pub mod logging;
pub mod models;
pub mod parser;
pub mod startup;
pub mod utils;
#[cfg(feature = "web")]
pub mod web;
