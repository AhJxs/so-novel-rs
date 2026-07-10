//! 工具函数
//!
//! 通用工具集合, **纯函数** + **零业务依赖**。业务侧编排代码 (`crawler` /
//! `desktop` / `cli`) 通过 `crate::utils::*` 复用。
//!
//! # 子模块分组 (按职责细分)
//!
//! - [`formatting`] — 字符串/时间/大小格式化 (`truncate`, `format_size`,
//!   `format_local_unix_secs`)
//! - [`fs`] — 文件系统 + 日志字段 (`sanitize_filename`, `truncate_log`,
//!   `to_absolute`, `format_size`)
//! - [`lang`] — 系统 locale 探测 (`detect_system_lang`)
//! - [`lock`] — 锁 poison 防护 (`mutex_or`, `rw_read_or`, `rw_write_or`)
//! - [`system`] — 跨平台 "用系统程序打开" (`open_path`, `reveal_in_folder`)
//! - [`time`] — unix 时间戳 + Duration 渲染 (`now_unix_secs`, `format_unix_local`)
//! - [`tty`] — TTY 进度行 (`print_in_place_line`)
//! - [`zhconv`] — 简繁中文转换 (`convert_text`, `convert_book_meta`)
//!
//! # 设计原则
//!
//! 1. **零业务依赖**: 不引用 `crawler` / `parser` / `models`; 唯一例外是
//!    [`zhconv::convert_book_meta`] 收 `Book` 入参, 但只读字段, 不耦合业务逻辑
//! 2. **纯函数优先**: 除 `lock` / `system` / `tty` (有副作用但仅限 OS 调用),
//!    其余子模块函数均为纯函数, 易测
//! 3. **不抢 business 抽象**: 业务编排的 `Result<T, String>` / `AppResult<T>`
//!    归口 [`crate::error::AppError`], 不在本模块定义
//!
//! # 不在本模块 (避免越界)
//!
//! - HTTP 相关 (UA 池 / Cloudflare bypass): 在 `crate::http::*`, **不**抽到 utils
//!   (utils 是 GUI / CLI / Web 三端共享, 抽 HTTP 进来会污染其他端)
//! - 字符串编码 (GBK / Big5): 在 `crate::http::encoding`, 同样是 HTTP 领域
//! - i18n 翻译: 在 `crate::i18n::*`, 是独立横切关注点

pub mod formatting;
pub mod fs;
pub mod lang;
pub mod lock;
pub mod system;
pub mod time;
pub mod tty;
pub mod zhconv;
