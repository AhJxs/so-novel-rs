//! 持久化层：运行数据（书架 / 下载任务 / 书源选择）存进嵌入式 `SurrealKV`
//! （[`StateStore`]，`~/.sonovel/state`），书源规则仍是 `~/.sonovel/rules/` 下的 JSON 文件。
//!
//! **错误**: 领域级 `RulesError` 保留路径与原因; 顶层 `DaoError` 统一归一, 业务层 `?` 一步透传到
//! `AppError`。
//!
//! **同步 I/O**: 全用 `std::fs` —— dao 函数被 gpui 启动等同步上下文直接
//! 调用; 迁到 `tokio::fs` 是 sync → async 的行为变更, 需全仓 caller 同步改。
//!
//! **[`write_atomically`] 是配置/规则的原子写核心**: 写 tmp → fsync → rename, 断电最坏情况"老文件还在"。

mod atomic_write;
mod dao;
mod rules;
mod sources_config;
mod state_store;
mod tasks;
mod tasks_init;

pub use atomic_write::write_atomically;
pub use dao::DaoError;
pub use rules::{
    META_AUTHOR, META_BOOK_NAME, META_CATEGORY, META_COVER_URL, META_INTRO, META_LAST_UPDATE_TIME,
    META_LATEST_CHAPTER, META_LATEST_CHAPTER_URL, META_STATUS, RulesError, apply_default_rule,
    init_rules_dir, list_rule_files, load_active_rules, load_rules_from_path,
};
pub use sources_config::SourcesConfig;
pub use state_store::{
    KEY_LIBRARY, KEY_SCHEMA, KEY_SOURCES, KEY_TASKS, STATE_SCHEMA_VERSION, StateOpenError,
    StateSnapshot, StateStore,
};
pub use tasks::{parse as parse_tasks, tasks_json_with_trim};
pub use tasks_init::{LoadedTasks, load_tasks_from_bytes};
