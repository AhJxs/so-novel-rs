//! 数据模型层  对应 Java 包 `com.pcdd.sonovel.model`.
//!
//! **不严格区分** DTO/PO/Param/Resp: 业务侧只有一种持久化格式 (JSON), 字段命名已由 web-ui 前端
//! 定型。实际做法: PO + DTO 同体 (`Book` / `Chapter` / `SearchResult`, 用 `#[serde(rename)]`
//! 控字段名); 领域枚举 (`FinishedReason` / `ContentType`) 各自模块内配 `Display + FromStr`;
//! `Rule` 拆 `search/book/toc/chapter/crawl` 子节, 与 JSON 实际结构对应。
//! 真要拆 DTO 时优先拆 `Book`, 属于按需优化, 不抢跑。

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
