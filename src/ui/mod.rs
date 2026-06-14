//! UI 子模块。
//!
//! 阶段 1：导航 + 6 页占位 + 设置页接入 config.toml 真实读写。其它页明确显示
//! "功能尚未迁移"占位，并按审计文档的设计列出待实现项，避免给用户错觉。

pub mod nav;
pub mod pages;
pub mod theme;
pub mod title_bar;
