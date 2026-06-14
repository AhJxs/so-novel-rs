//! 书源规则的加载与默认值填充。对应 Java `util.SourceUtils` 中的 rule 部分
//! 与 `core.Source` 的有效配置派生。

mod loader;
mod overrides;
mod source;

pub use loader::{
    apply_default_rule, load_rules_from_db, load_rules_from_path, RulesError, META_AUTHOR,
    META_BOOK_NAME, META_CATEGORY, META_COVER_URL, META_INTRO, META_LAST_UPDATE_TIME,
    META_LATEST_CHAPTER, META_STATUS,
};
pub use overrides::SourceOverrides;
pub use source::{EffectiveCrawl, Source};
