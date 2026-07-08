//! 全局常量与错误码表
//!
//! # 设计原则
//!
//! - **先有真实散落, 后有 constant/**: 本目录不是"把所有常量都搬过来", 而是
//!   收纳那些**真的**在多处重复、或者**真的**需要单点维护的值。
//!   Ponytail 原则: 已经在自己模块里组织良好的常量 (UA 池、theme 字号) 不动。
//!
//! - **每个子模块对应一类**: 新增值时先问"它属于哪类", 不要无脑放 `mod.rs`。
//!
//! # 现有子模块
//!
//! - [`error_code`] — 业务层错误码 (1xxx 规则 / 2xxx 解析 / 3xxx 资源 /
//!   4xxx 内部 / 5xxx 导出)。被 `web::WebError` 引用, 替代原 `match` 散落字符串。
//!
//! # 未来可加 (按需, 不抢跑)
//!
//! - `http.rs` — UA 池 (目前 `http/ua.rs` 内部, 等 ≥2 处用再抽)
//! - `theme.rs` — 字号/间距 (目前 `gpui_app/themes.rs` 内部, 同上)
//! - `limits.rs` — 限流阈值 (目前散在 `AppConfig::with_defaults`, 是配置非魔法)
//! - `paths.rs` — 目录名常量 (目前散在 `db/rules.rs` 等, 视 PR #11 db 重构时抽)

pub mod error_code;
