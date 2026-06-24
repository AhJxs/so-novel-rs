//! 数据模型层。对应 Java 包 `com.pcdd.sonovel.model`。

pub mod book;
pub mod chapter;
pub mod content_type;
pub mod rule;
pub mod search;
pub mod source_info;
pub mod task_record;

pub use book::Book;
pub use chapter::Chapter;
pub use content_type::ContentType;
pub use rule::{
    EffectiveCrawl, Rule, RuleBook, RuleChapter, RuleCrawl, RuleSearch, RuleToc, Source,
};
pub use search::SearchResult;
pub use source_info::SourceInfo;
pub use task_record::{DownloadTaskRecord, FailureRecord, FinishedReason};
