//! JavaScript `引擎（boa_engine）。对应` Java `util.JsCaller`。
//!
//! 选 boa 而非 rquickjs：纯 Rust 无 C 工具链依赖（本机没有 cl.exe，CI 上更不可控），
//! 现有规则的 18 处 `@js:` 片段全部为 ES5/ES6 子集 boa 完整支持。
//!
//! 详见 audit §6.1 修订。

mod runtime;

pub use runtime::{eval_function_returning_string, post_process};
