//! HTTP 抓取层。对应 Java `core.OkHttpClientFactory` + `util.CrawlUtils`
//! + `util.RandomUA` + `util.HttpClientContext`。
//!
//! 同时提供两种 client：
//! - `build_blocking_client`：同步 reqwest::blocking，供 parser 层 / 测试用，
//!   让解析逻辑不被 async 渗透；
//! - `build_async_client`：异步 reqwest::Client，供 crawler 层（tokio runtime）用。

pub mod cf;
pub mod client;
pub mod encoding;
pub mod fetch;
pub mod ua;
pub mod url_join;
pub mod util;

pub use cf::{fetch_via_cf_bypass, has_cloudflare};
pub use client::{ClientOptions, build_blocking_client};
pub use encoding::decode_response_bytes;
pub use fetch::{FetchRequest, FetchResponse, HttpMethod, fetch};
pub use ua::random_ua;
pub use url_join::{abs_url, origin_or_self};
pub use util::{
    build_form_data, clean_invisible_chars, format_url_query, random_interval_ms,
    random_retry_interval_ms,
};
