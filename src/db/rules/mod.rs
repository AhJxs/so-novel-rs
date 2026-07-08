//! 书源规则: 目录初始化 + 文件解析 + 默认值填充 + 活跃规则加载
//!
//! 一切围绕 `Rule` (定义在 `crate::models::rule`) 这个结构:
//!
//! - **常量** (`META_*` / `BUNDLED_RULES`) — 模板字符串和编译期嵌入的规则文件
//! - **解析** (`load_rules_from_path` / `load_active_rules`) — 从 `.json` / `.json5`
//!   文件或目录读出 `Vec<Rule>`, 分配自增 ID
//! - **默认值** (`apply_default_rule`) — 给空字段回填 `baseUri` / `timeout` /
//!   `book.*` 的 meta 后备查询 (与 Java 端 `util.SourceUtils#applyDefaultRule` 等价)
//! - **目录初始化** (`init_rules_dir` / `list_rule_files`) — 首次启动把编译期
//!   嵌入的规则文件落到 `~/.sonovel/rules/`, 并按 `.json` / `.json5` 枚举现有文件
//!
//! # 子模块
//!
//! - [`constants`] — `META_*` + `BUNDLED_RULES`
//! - [`error`] — `RulesError` 枚举
//! - [`loader`] — 公共 `load_rules_from_path` / `load_active_rules` + 私有 `walk` / `parse` / `apply_disabled_urls`
//! - [`apply_default`] — 公共 `apply_default_rule`
//! - [`init`] — 公共 `init_rules_dir` / `list_rule_files`

pub mod apply_default;
pub mod constants;
pub mod error;
pub mod init;
pub mod loader;

pub use apply_default::apply_default_rule;
pub use constants::{
    META_AUTHOR, META_BOOK_NAME, META_CATEGORY, META_COVER_URL, META_INTRO, META_LAST_UPDATE_TIME,
    META_LATEST_CHAPTER, META_LATEST_CHAPTER_URL, META_STATUS,
};
pub use error::RulesError;
pub use init::{init_rules_dir, list_rule_files};
pub use loader::{load_active_rules, load_rules_from_path};
