//! so-novel-rs — Rust 桌面客户端（GPUI + gpui-component，egui 已完全移除）。
//!
//! 模块划分：
//! - `gpui_app` — 新 GPUI GUI 入口（Stage 1+）。
//! - `app` / `db` / `crawler` / `config` / `models` / `parser` / `export` /
//!   `http` / `js` / `utils` / `cli` — 业务 + 数据层（GUI 解耦）。
//!
//! ## 工程规约
//!
//! - `unsafe_code = "deny"`（`#![deny(unsafe_code)]` in lib.rs）：仓库内禁止 `unsafe`。
//!   如确需启用, 必须先开 RFC 评审。
//! - 文档政策: 重要 public fn 必带 `#[tracing::instrument]` + `# Errors` + `# Examples`
//!   (PR #18–#20 已覆盖); struct/enum 顶层 doc 由各模块顶部 `//!` 文档承担。
//!   字段级 docs 不强制 (serde-derived 字段跟 JSON 一一对应, 加 `///` 是噪声)。
//! - clippy pedantic + nursery 触发: 一次性 PR 收敛 `mut` 多余、`clone()` 多余、
//!   `must_use` 缺失等; 见 lib.rs 顶部的 `#![warn(clippy::*)]` 部分。
//! - 错误体系: 领域错误 (`ExportError`/`WebError`/...) 保留在领域内, 通过
//!   `From` 归一到 `crate::error::AppError`; 二进制入口 (`main.rs`) 允许用
//!   `anyhow`。
//! - 工具: `crate::utils::*` (rename 自旧 `util/`, 2026-07-08 PR #1)。

// -------------------------------------------------------------------------------------
// lint 配置（从 Cargo.toml [lints.*] 迁入，便于统一管理）
// 设计原则:
//   - rust 编译期 lint 全开, 不允许 `unsafe_code`（仓库无 unsafe 需求）
//   - clippy 走 pedantic + nursery 渐进式收紧, 当前阶段以 warn 为主
// -------------------------------------------------------------------------------------

#![deny(unsafe_code)]
#![allow(missing_docs)]
#![warn(dead_code)]
#![warn(invalid_value)]
#![warn(rustdoc::broken_intra_doc_links)]
// clippy: pedantic + nursery 整体 warn
#![warn(clippy::pedantic, clippy::nursery)]
// clippy: 安全/正确性子集单独 warn
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![warn(clippy::todo, clippy::unimplemented)]
// clippy: 允许项（见设计原则注释）
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

// `rust_i18n::i18n!` 必须在 crate root 调一次 —— 它在 crate root 生成 `_rust_i18n_t`
// 函数 + locale 表，`t!` 宏和 `rust_i18n::set_locale` 都依赖它。
// gpui-component 在它自己的 crate root 也调了一次（加载 `locales/ui.yml`），两套 i18n
// 实例各管各的 key 表，但**全局 locale 共享**（同一 `CURRENT_LOCALE`）。
rust_i18n::i18n!("locales");

pub mod app;
pub mod cli;
pub mod config;
pub mod constant;
pub mod crawler;
pub mod db;
pub mod error;
pub mod export;
#[cfg(feature = "gui")]
pub mod gpui_app;
pub mod http;
pub mod i18n;
pub mod js;
pub mod logger;
pub mod models;
pub mod parser;
pub mod startup;
pub mod utils;
#[cfg(feature = "web")]
pub mod web;
