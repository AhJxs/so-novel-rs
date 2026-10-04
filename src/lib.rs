//! so-novel-rs — Rust 桌面客户端（GPUI）。
//!
//! 模块划分：`desktop`（GPUI 入口 + 业务 model + 渲染/主题/导航/通知）；`db` / `crawler` /
//! `config` / `models` / `parser` / `export` / `http` / `js` / `utils` / `cli` / `core` 为与
//! GUI 解耦的业务 + 数据层。
//! 工程规约：仓库禁止 `unsafe`（确需启用须先过 RFC）；重要 public fn 必带
//! `#[tracing::instrument]` + `# Errors` + `# Examples`；struct/enum 顶层 doc 由模块 `//!`
//! 承担、字段级 docs 不强制（serde-derived 字段与 JSON 一一对应，加 `///` 是噪声）；
//! 领域错误经 `From` 归一到 `crate::error::AppError`，仅 `main.rs` 可用 `anyhow`。

// ---------------------------------------------------------------------------
// lint 配置（自 Cargo.toml [lints.*] 迁入，便于统一管理）
// 原则：rustc lint 全开且禁止 `unsafe_code`（仓库无 unsafe 需求）；
// clippy 走 pedantic + nursery 渐进收紧，当前阶段以 warn 为主。
// ---------------------------------------------------------------------------

#![deny(unsafe_code)]
#![allow(missing_docs)]
#![warn(dead_code)]
#![warn(invalid_value)]
#![warn(rustdoc::broken_intra_doc_links)]
#![warn(clippy::pedantic, clippy::nursery)]
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![warn(clippy::todo, clippy::unimplemented)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::missing_errors_doc,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_sign_loss,
    clippy::struct_excessive_bools,
    clippy::too_many_lines,
    clippy::result_large_err
)]

// `rust_i18n::i18n!` 必须在 crate root 调一次（生成 `_rust_i18n_t` 宏 + `_rust_i18n_try_translate`
// 查表函数 + locale 表，`t!` 与 `set_locale` 都依赖它）；组件库内部另调一次、各管各的 key 表，
// 但**全局 locale 共享**；`desktop::run` 再用 `rust_i18n::extend!` 把我们的表接到组件后端（先查 app.yml 的 `gpui_component:` 段，再回落 ui.yml）。
rust_i18n::i18n!("locales");

pub mod config;
pub mod core;
pub mod crawler;
pub mod db;
pub mod desktop;
pub mod error;
pub mod export;
pub mod http;
pub mod i18n;
pub mod js;
pub mod logger;
pub mod models;
pub mod parser;
pub mod utils;
