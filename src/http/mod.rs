//! HTTP 抓取层：client 工厂 + 单次请求 + 编码兜底 + CF 旁路 + URL / UA 工具。
//!
//! 只提供 async client —— blocking 版在 tokio `spawn_blocking` 里 drop 会 panic。

pub mod client;
pub mod cloudflare;
pub mod encoding;
pub mod fetch;
pub mod http_clients;
pub mod system_proxy;
pub mod url_join;
pub mod user_agent;
pub mod util;

pub use client::ClientOptions;
pub use cloudflare::{fetch_via_cf_bypass, has_cloudflare};
pub use encoding::decode_response_bytes;
pub use fetch::{
    CfFallbackError, FetchRequest, FetchResponse, HttpMethod, fetch, fetch_with_cf_fallback,
};
pub use http_clients::HttpClients;
// `detect` 在本模块内重命名为 `detect_system_proxy`：`crate::http::detect()` 看不出在探什么。
pub use system_proxy::{
    AbsentReason, SystemProxy, detect as detect_system_proxy, resolve_proxy_url,
};
pub use url_join::{absolute_url, origin_or_self};
pub use user_agent::random_user_agent;
pub use util::{
    build_form_data, clean_invisible_chars, format_url_query, random_interval_ms,
    random_retry_interval_ms,
};
