//! so-novel-rs — Rust 桌面客户端（GPUI + gpui-component，egui 已完全移除）。
//!
//! 模块划分：
//! - `gpui_app` — 新 GPUI GUI 入口（Stage 1+）。
//! - `app` / `db` / `crawler` / `config` / `models` / `parser` / `export` /
//!   `http` / `js` / `rules` / `util` / `cli` — 业务 + 数据层（GUI 解耦）。
//! - 旧 `ui` / `design_system` / `material_icons` 已在 Stage 11 整体删除。

// 整个 crate 默认 deny unsafe（egui 时代的 windows.rs 已删除，目前没有 FFI）。
#![deny(unsafe_code)]

pub mod app;
pub mod cli;
pub mod config;
pub mod crawler;
pub mod db;
pub mod export;
pub mod gpui_app;
pub mod http;
pub mod js;
pub mod models;
pub mod parser;
pub mod rules;
pub mod util;
