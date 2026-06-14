//! HTTP 抓取层。对应 Java `core.OkHttpClientFactory` + `util.CrawlUtils`
//! + `util.RandomUA` + `util.HttpClientContext`。
//!
//! 阶段 2a 仅暴露**同步、阻塞**的 reqwest::blocking::Client 包装。
//! 这是为了让 parser 层不被 async 渗透到测试中（解析的逻辑是同步的）。
//! 阶段 3 接下载调度时，会再加 reqwest::Client（async）的 client_async 工厂。

pub mod cf;
pub mod client;
pub mod encoding;
pub mod fetch;
pub mod ua;
pub mod url_join;
pub mod util;

pub use cf::{fetch_via_cf_bypass, has_cloudflare};
pub use client::{build_blocking_client, ClientOptions};
pub use encoding::decode_response_bytes;
pub use fetch::{fetch, FetchRequest, FetchResponse, HttpMethod};
pub use ua::random_ua;
pub use url_join::{abs_url, origin_or_self};
pub use util::{
    build_form_data, clean_invisible_chars, format_url_query, random_interval_ms,
    random_retry_interval_ms,
};
