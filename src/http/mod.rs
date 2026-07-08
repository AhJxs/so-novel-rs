//! HTTP 抓取层。对应 Java `core.OkHttpClientFactory` + `util.CrawlUtils`
//! + `util.RandomUA` + `util.HttpClientContext`。
//!
//! 提供 `build_async_client` 异步 `reqwest::Client，供` crawler 层（tokio runtime）用。
//! blocking client 路径已移除（reqwest blocking 在 tokio `spawn_blocking` 里 drop 会 panic）。

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
