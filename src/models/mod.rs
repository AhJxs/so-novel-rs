//! 数据模型层 (PR #10 文档化, 2026-07-08). 对应 Java 包 `com.pcdd.sonovel.model`.
//!
//! # 设计原则
//!
//! 本项目**不严格区分** DTO/PO/Param/Resp 四种类型。理由:
//! - 业务侧只有一个持久化格式 (JSON), 字段命名在 web-ui 前端已经定型
//! - 各结构体总规模 535 LOC, 强行拆 3-4 套类型会让"哪个是源"难追踪
//! - 现有测试覆盖已经够, 拆 4 套会引入 N 个 `From<Po> for Dto` 转换代码,
//!   而这些转换大部分是 `clone()` 字段, 没有真正的领域逻辑
//!
//! 实际做法:
//! - **PO + DTO 同体** (`Book` / `Chapter` / `SearchResult` 等): 同时承担
//!   持久化和传输角色, `#[serde(rename = "...")]` 控制 JSON 字段名
//! - **领域枚举单点** (`FinishedReason` / `ContentType`):
//!   在各自模块, `Display + FromStr + Serialize + Deserialize` 一起
//! - **Rule 拆分**: `Rule` 是书源规则的根, 内部 5 个 sub-struct 拆 `search/
//!   book/toc/chapter/crawl` 子节, 跟 JSON 实际结构对应
//!
//! # 后续可优化 (按需, 不抢跑)
//!
//! - 真要拆 DTO 时, 优先拆 `Book` (web 响应可能想隐藏 `cover_url_bytes` 这类
//!   内部字段, 加 `BookResponse { book_name, author, ... }`)
//! - `Rule` 在 web 写入时可能想校验 (e.g. `url` 必须 http(s)), 那是
//!   `Rule::validate()` 的事, 不需要单独 DTO

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
