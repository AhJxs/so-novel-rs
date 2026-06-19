//! reqwest 客户端工厂。对应 Java `core.OkHttpClientFactory`。
//!
//! 设计：
//! - **rustls** 替代 OpenSSL，避免在 Windows/CI 上引入 C 依赖；
//! - 默认跟随重定向、复用连接池、给所有请求加 `Accept-Language: zh-CN,zh;q=0.9,en;q=0.8`
//!   （等价 Java 端 OkHttpClientFactory 的拦截器行为）；
//! - 代理与 SSL 跳过通过 `ClientOptions` 控制；
//! - 同时提供 **blocking** 与 **async** 两种 client：
//!   - `build_blocking_client` 用于 parser 同步调用（被 `spawn_blocking` 包裹）；
//!   - `build_async_client` 用于直接在 tokio 任务里 `await`（不创建嵌套 runtime）。

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;

use crate::config::AppConfig;

/// 控制 client 行为的小结构。
///
/// 与 Java 端 `OkHttpClientFactory.create(AppConfig, unsafe)` 等价：
/// - `unsafe_ssl`：关闭 SSL 校验，针对老书源（rate-limit.json 里 `0xs.net`
///   的 `ignoreSsl: true`）。
#[derive(Debug, Clone, Default)]
pub struct ClientOptions {
    pub unsafe_ssl: bool,
}

/// 默认连接/读写超时（秒）。Java 端 `OkHttpClientFactory.TIMEOUT = 10`。
const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// 构造一个 blocking reqwest Client，按 `cfg` 决定是否启用代理。
///
/// ⚠️ **不要在 `tokio::task::spawn_blocking` 内部 drop 该 Client**：
/// reqwest 的 blocking 客户端内部维护一个独立 `current_thread` tokio runtime，
/// 在 tokio 阻塞池工作线程上 drop 会触发 "Cannot drop a runtime in a context
/// where blocking is not allowed" panic。请让 Client 在 spawn_blocking 闭包
/// 之外创建并 clone 进去，或改用 `build_async_client`。
pub fn build_blocking_client(cfg: &AppConfig, opts: &ClientOptions) -> Result<Client> {
    let mut builder = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .pool_idle_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .cookie_store(true)
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::ACCEPT_LANGUAGE,
                "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap(),
            );
            h
        });

    if cfg.proxy_enabled {
        let proxy_url = format!("http://{}:{}", cfg.proxy_host, cfg.proxy_port);
        let proxy = reqwest::Proxy::all(&proxy_url)
            .with_context(|| format!("invalid proxy URL: {proxy_url}"))?;
        builder = builder.proxy(proxy);
    }

    if opts.unsafe_ssl {
        builder = builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true);
    }

    builder
        .build()
        .context("build reqwest blocking client failed")
}

/// 构造一个 async reqwest Client。共用 tokio runtime，**不**创建嵌套 runtime，
/// 适合在 `tokio::spawn` 内部直接 `.await`。
pub fn build_async_client(cfg: &AppConfig, opts: &ClientOptions) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS))
        .pool_idle_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .cookie_store(true)
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::ACCEPT_LANGUAGE,
                "zh-CN,zh;q=0.9,en;q=0.8".parse().unwrap(),
            );
            h
        });

    if cfg.proxy_enabled {
        let proxy_url = format!("http://{}:{}", cfg.proxy_host, cfg.proxy_port);
        let proxy = reqwest::Proxy::all(&proxy_url)
            .with_context(|| format!("invalid proxy URL: {proxy_url}"))?;
        builder = builder.proxy(proxy);
    }

    if opts.unsafe_ssl {
        builder = builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true);
    }

    builder.build().context("build reqwest async client failed")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_with_default_config_succeeds() {
        let cfg = AppConfig::default();
        let _client = build_blocking_client(&cfg, &ClientOptions::default()).unwrap();
    }

    #[test]
    fn build_with_proxy_enabled_invalid_addr_still_constructs() {
        // reqwest 的 Proxy::all 只做 URL 解析；不真正连。
        let cfg = AppConfig {
            proxy_enabled: true,
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 1,
            ..AppConfig::default()
        };
        let _client = build_blocking_client(&cfg, &ClientOptions::default()).unwrap();
    }

    #[test]
    fn build_with_unsafe_ssl_succeeds() {
        let cfg = AppConfig::default();
        let opts = ClientOptions { unsafe_ssl: true };
        let _client = build_blocking_client(&cfg, &opts).unwrap();
    }

    /// 端到端：开本地 TCP listener 当 mock proxy，跑 `build_async_client`
    /// 走一次真实 HTTP GET，断言：
    /// 1) 客户端成功拿到 200；
    /// 2) **请求确实打到了 mock proxy**（HTTP 代理模式下 reqwest 把完整 URL
    ///    写进请求行，如 `GET http://example.com/ HTTP/1.1`，不打到目标主机，
    ///    直接打到 proxy）。
    /// 这才能证明 `cfg.proxy_enabled=true` 不只是"URL 解析没报错"，而是真的把
    /// 流量走了代理。
    #[tokio::test]
    async fn proxy_enabled_actually_routes_traffic_through_proxy() {
        use std::sync::{Arc, Mutex};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        // 1) 起 mock proxy
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = listener.local_addr().unwrap().port();
        let received: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let received_clone = received.clone();
        let proxy_task = tokio::spawn(async move {
            // 接受 1 个连接（我们的 client 只发 1 个请求），读取请求行后回 200。
            if let Ok((mut sock, _)) = listener.accept().await {
                let mut buf = vec![0u8; 1024];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                *received_clone.lock().unwrap() = Some(req);
                let _ = sock
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
                    )
                    .await;
                let _ = sock.shutdown().await;
            }
        });

        // 2) 走工厂构造 client，proxy_enabled=true
        let cfg = AppConfig {
            proxy_enabled: true,
            proxy_host: "127.0.0.1".into(),
            proxy_port: proxy_port as u16,
            ..AppConfig::default()
        };
        let client = build_async_client(&cfg, &ClientOptions::default()).unwrap();

        // 3) 真实 GET —— URL 故意选 `example.com` 但 mock proxy 永远不连它，
        //    走代理就一定命中本 listener。
        let resp = client
            .get("http://example.com/proxy-check")
            .send()
            .await
            .expect("request through proxy should succeed");
        assert!(
            resp.status().is_success(),
            "proxy returned {}",
            resp.status()
        );

        // 4) mock proxy 那边要收到"代理形态"的请求行（完整 URL 写在 request line）
        let _ = proxy_task.await;
        let req_line = received
            .lock()
            .unwrap()
            .clone()
            .expect("mock proxy should have seen the request");
        let first_line = req_line.lines().next().unwrap_or("");
        assert!(
            first_line.starts_with("GET http://example.com/proxy-check"),
            "expected absolute-URL request line (proxy mode), got: {first_line:?}"
        );
    }
}
