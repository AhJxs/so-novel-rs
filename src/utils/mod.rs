//! 通用工具集合：**纯函数** + **零业务依赖**，供 `crawler` / `desktop` / `cli` 复用。
//!
//! 子模块：`formatting`（字符串 / 时间 / 大小）、`fs`（文件名、日志脱敏、绝对路径）、
//! `lang`（系统 locale）、`lock`（锁 poison 防护）、`system`（系统程序打开）、`time`
//! （unix 时间戳）、`tty`（TTY 进度行）、`zhconv`（简繁转换）。
//!
//! 边界：HTTP / 编码 / i18n **不**下沉到这里（三端共享，抽进来会污染其他端）；唯一例外是
//! `zhconv::convert_book_meta` 收 `Book` 入参，但只读字段。

pub mod formatting;
pub mod fs;
pub mod lang;
pub mod lock;
pub mod system;
pub mod time;
pub mod tty;
pub mod zhconv;
