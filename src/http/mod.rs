//! HTTP 抓取层：client 工厂 + 单次请求 + 编码兜底 + CF 旁路 + URL / UA 工具。
//!
//! 只提供 async client —— blocking 版在 tokio `spawn_blocking` 里 drop 会 panic。

pub mod cf;
pub mod client;
pub mod clients;
pub mod encoding;
pub mod fetch;
pub mod ua;
pub mod url_join;
pub mod util;

pub use cf::{fetch_via_cf_bypass, has_cloudflare};
pub use client::ClientOptions;
pub use clients::HttpClients;
pub use encoding::decode_response_bytes;
pub use fetch::{
    CfFallbackError, FetchRequest, FetchResponse, HttpMethod, fetch, fetch_with_cf_fallback,
};
pub use ua::random_ua;
pub use url_join::{abs_url, origin_or_self};
pub use util::{
    build_form_data, clean_invisible_chars, format_url_query, random_interval_ms,
    random_retry_interval_ms,
};
