//! so-novel-rs — Rust + egui 桌面客户端（迁移中）。
//!
//! 模块划分对应审计文档 §5。当前阶段 1 仅实现：app/config/models/rules/ui/util。
//! HTTP / 解析 / 下载 / 导出由后续阶段补齐。

// 整个 crate 默认 deny unsafe；window 模块单独允许（Windows DWM FFI）。
#![deny(unsafe_code)]

pub mod app;
pub mod cli;
pub mod config;
pub mod crawler;
pub mod db;
pub mod export;
pub mod http;
pub mod js;
pub mod models;
pub mod parser;
pub mod rules;
pub mod ui;
pub mod util;
pub mod window;
