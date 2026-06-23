//! 书源规则的加载与默认值填充。对应 Java `util.SourceUtils` 中的 rule 部分
//! 与 `core.Source` 的有效配置派生。
//!
//! 注意：`load_active_rules`、`init_rules_dir`、`list_rule_files` 已迁移到
//! `crate::persistent` 模块。本模块保留 `load_rules_from_path`、`apply_default_rule`
//! 和 META 常量供 parser 模块使用。

mod loader;
mod source;

pub use loader::{
    META_AUTHOR, META_BOOK_NAME, META_CATEGORY, META_COVER_URL, META_INTRO, META_LAST_UPDATE_TIME,
    META_LATEST_CHAPTER, META_STATUS, RulesError, apply_default_rule, load_rules_from_path,
};
pub use source::{EffectiveCrawl, Source};
