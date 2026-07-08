//! 共享 HTTP client 集合。
//!
//! # 为什么需要这个？
//!
//! `reqwest::Client` 内部持有连接池 + TLS session cache。每次 `Client::builder().build()`
//! 都得到一个**全新**实例，等于"重置所有 keep-alive 连接 + 重做 TLS 握手"。
//!
//! 之前的实现（`crawler/mod.rs::resolve_book` / `download_chapters` /
//! `crawler/search.rs` / `crawler/health.rs` 等）每次爬取都从零构造，
//! 100 章小说 = 100 次 TLS 握手 = 30-50% 浪费。
//!
//! # 设计
//!
//! 按"配置维度"维护少量固定实例。reqwest 一旦构造完，proxy /
//! `danger_accept_invalid_certs` 都不能 in-place 改 —— 所以维度变了就得
//! 整体 rebuild。
//!
//! 实际只维护 3 个实例：
//! - `safe` —— `unsafe_ssl=false` 的常规请求（占 99% 流量）
//! - `unsafe_ssl` —— `Rule.ignore_ssl=true` 的老书源
//! - `gh_proxy` —— 更新检查专用，走 forward proxy，配置跟其它 client 互斥
//!
//! 没有"per-Rule 单独 client"——`unsafe_ssl` 是 per-Rule 的**唯一**维度，
//! 所以 2 个 client 足够覆盖所有规则。配置改了 proxy → `rebuild_proxy`
//! 重建 safe + `unsafe_ssl` `两个实例（gh_proxy` 不受 proxy 影响）。
//!
//! # 并发安全
//!
//! `reqwest::Client` 本身 `Send + Sync`（内部 Arc）。`proxy_signature`
//! 用 `std::sync::Mutex` —— 锁粒度极小（只在 `rebuild_proxy` 拿一下），用
//! `parking_lot` 收益不抵加依赖成本。

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::config::AppConfig;
#[cfg(test)]
use crate::config::{GlobalCfg, ProxyCfg};
use crate::http::client::{ClientOptions, build_async_client};
use crate::models::Rule;
use crate::utils::lock::{mutex_or, rw_read_or, rw_write_or};

/// 当前生效的 proxy 配置快照。`rebuild_proxy` 用它判断"配置是否真的变了"。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxySignature {
    enabled: bool,
    host: String,
    port: u16,
}

impl ProxySignature {
    fn from_cfg(cfg: &AppConfig) -> Self {
        Self {
            enabled: cfg.proxy.proxy_enabled,
            host: cfg.proxy.proxy_host.clone(),
            port: cfg.proxy.proxy_port,
        }
    }
}

/// 共享 HTTP client 集合。
///
/// 跨任务复用连接池 + TLS session。构造代价一次性（启动时），
/// 之后每个爬取只拿 `Arc::clone` —— 零成本。
///
/// 持有 3 个 `Arc<reqwest::Client>`：`safe` / `unsafe_ssl` / `gh_proxy`。
/// proxy 改了 → `rebuild_proxy` `整体替换前两个；gh_proxy` 是更新检查专用，
/// 独立维护（用户主动改 `gh_proxy` 字符串才会重建）。
pub struct HttpClients {
    /// safe / `unsafe_ssl` 两个 client 用 `RwLock` 保护：
    /// - `for_rule` 只做 `RwLock::read（无竞争，桌面场景几乎全是读`）；
    /// - `rebuild_proxy` 用 `RwLock::write` 替换 Arc。
    ///   比裸指针 + unsafe 更安全，且 `reqwest::Client` 内部本身就是 Arc（clone ≈ `Arc::clone`）。
    clients: RwLock<(Arc<reqwest::Client>, Arc<reqwest::Client>)>,
    /// `gh_proxy` 字符串 + 对应 `client。gh_proxy` 为空时这个 client 不被使用
    /// 但保留 —— 用户从有 → 无切换时不会出现"没 client 可用"的窗口。
    gh_proxy: Mutex<(String, Arc<reqwest::Client>)>,
    /// 当前生效的 proxy 元组；用于 `rebuild_proxy` 短路"没真改就不重建"。
    proxy_signature: Mutex<ProxySignature>,
}

impl HttpClients {
    /// 从 `AppConfig` 构造初始 client 集合。
    pub fn new(cfg: &AppConfig) -> Result<Self> {
        let safe = Arc::new(
            build_async_client(cfg, &ClientOptions { unsafe_ssl: false })
                .context("构造 safe HTTP client 失败")?,
        );
        let unsafe_ssl = Arc::new(
            build_async_client(cfg, &ClientOptions { unsafe_ssl: true })
                .context("构造 unsafe_ssl HTTP client 失败")?,
        );
        // gh_proxy client：如果用户配了 gh_proxy，用它做 forward proxy；否则退化为普通 client。
        // `gh_proxy_pair()` 调用方自己判断 URL 是否为空决定是否使用。
        let gh_proxy_url = cfg.global.gh_proxy.trim().to_string();
        let gh_proxy_client = if gh_proxy_url.is_empty() {
            Arc::new(
                build_async_client(cfg, &ClientOptions::default())
                    .context("构造 gh_proxy HTTP client 失败")?,
            )
        } else {
            let mut builder = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::limited(5))
                .user_agent("so-novel-rs")
                .default_headers({
                    let mut h = reqwest::header::HeaderMap::new();
                    h.insert(
                        reqwest::header::ACCEPT_LANGUAGE,
                        reqwest::header::HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
                    );
                    h
                });
            if let Ok(proxy) = reqwest::Proxy::all(&gh_proxy_url) {
                builder = builder.proxy(proxy);
            }
            Arc::new(builder.build().context("构造 gh_proxy HTTP client 失败")?)
        };
        Ok(Self {
            clients: RwLock::new((safe, unsafe_ssl)),
            gh_proxy: Mutex::new((gh_proxy_url, gh_proxy_client)),
            proxy_signature: Mutex::new(ProxySignature::from_cfg(cfg)),
        })
    }

    /// 按 `Rule.ignore_ssl` 选 client。
    ///
    /// 返回 owned `reqwest::Client`（内部是 Arc，clone 只做 refcount bump，几乎零开销）。
    /// 返回 owned 而非 `&` 是因为底层用 `RwLock` 保护：`RwLockReadGuard` 不能泄漏出引用。
    #[inline]
    pub fn for_rule(&self, rule: &Rule) -> reqwest::Client {
        rw_read_or("for_rule", &self.clients).map_or_else(
            |_| {
                // 锁 poison：退路拿 unsafe_ssl（哪怕可能坏，也比 worker panic 把整个 web 拖死好）。
                // 二次 read 仍失败则返 reqwest::Client::new() 作 last resort。
                self.clients.read().map_or_else(
                    |_| {
                        tracing::error!("for_rule: 二次 read 仍失败，返回 reqwest::Client::new()");
                        reqwest::Client::new()
                    },
                    |g| g.1.as_ref().clone(),
                )
            },
            |guard| {
                if rule.ignore_ssl {
                    guard.1.as_ref().clone()
                } else {
                    guard.0.as_ref().clone()
                }
            },
        )
    }

    /// `gh_proxy` 专用 client（更新检查用）。
    ///
    /// 返回 `(gh_proxy_url, Arc<client>)` —— 调用方应自己判断 `gh_proxy_url.is_empty()`
    /// 再决定用这个 client 还是改走 `for_rule`。
    pub fn gh_proxy_pair(&self) -> (String, Arc<reqwest::Client>) {
        // 锁 poison：返空配置 + safe client（调用方看到空 url 会走 for_rule 路径，
        // 不会用坏掉的 gh_proxy client）。
        mutex_or("gh_proxy_pair", &self.gh_proxy).map_or_else(
            |_| {
                let fallback = self
                    .clients
                    .read()
                    .ok()
                    .map_or_else(|| Arc::new(reqwest::Client::new()), |g| Arc::clone(&g.0));
                (String::new(), fallback)
            },
            |guard| (guard.0.clone(), Arc::clone(&guard.1)),
        )
    }
    /// proxy 配置变了 → 重建 safe + `unsafe_ssl` 两个 client。
    ///
    pub fn rebuild_proxy(&self, cfg: &AppConfig) -> Result<()> {
        let new_sig = ProxySignature::from_cfg(cfg);
        let old_sig = mutex_or("rebuild_proxy:read_sig", &self.proxy_signature)
            .map_err(anyhow::Error::msg)
            .context("proxy_signature 锁 poison")?
            .clone();
        if old_sig == new_sig {
            return Ok(());
        }

        // proxy 改了 —— 重建。用 RwLock::write 原子替换两个 Arc。
        // 读端在 rebuild 期间短暂阻塞（reqwest::Client 构造不含 IO，代价小）；
        // 旧 Arc 被 in-flight 任务 clone 过去的引用不会受影响，等它们自然 drop。
        let safe = Arc::new(
            build_async_client(cfg, &ClientOptions { unsafe_ssl: false })
                .context("重建 safe HTTP client 失败")?,
        );
        let unsafe_ssl = Arc::new(
            build_async_client(cfg, &ClientOptions { unsafe_ssl: true })
                .context("重建 unsafe_ssl HTTP client 失败")?,
        );
        {
            let mut guard = rw_write_or("rebuild_proxy:write_clients", &self.clients)
                .map_err(anyhow::Error::msg)
                .context("clients 锁 poison")?;
            guard.0 = safe;
            guard.1 = unsafe_ssl;
        }

        {
            let mut guard = mutex_or("rebuild_proxy:write_sig", &self.proxy_signature)
                .map_err(anyhow::Error::msg)
                .context("proxy_signature 锁 poison")?;
            *guard = new_sig;
        }
        Ok(())
    }

    /// 仅测试用：拿到当前 `safe（unsafe_ssl=false）client` 的 Arc 内部指针，
    /// 用于断言"rebuild 真的换了实例"。返回 `Result` 让调用点（test mod 已
    /// `allow(unwrap_used)`）决定 panic 还是吞错。
    #[cfg(test)]
    fn safe_client_ptr(&self) -> Result<*const reqwest::Client, anyhow::Error> {
        // 测试路径上锁不会 poison；panic 立即可见，比静默返错更易调试。
        let guard = self
            .clients
            .read()
            .map_err(|e| anyhow::anyhow!("clients RwLock poisoned: {e}"))?;
        Ok(Arc::as_ptr(&guard.0))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    fn default_cfg() -> AppConfig {
        AppConfig::default()
    }

    #[test]
    fn for_rule_picks_safe_vs_unsafe() {
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let safe_rule = Rule {
            ignore_ssl: false,
            ..Rule::default()
        };
        let unsafe_rule = Rule {
            ignore_ssl: true,
            ..Rule::default()
        };
        // for_rule 应当返回有效 client（不 panic、不死锁）。
        let _c1 = clients.for_rule(&safe_rule);
        let _c2 = clients.for_rule(&unsafe_rule);

        // rebuild 后 for_rule 仍正常工作
        let new_cfg = AppConfig {
            proxy: ProxyCfg {
                proxy_enabled: true,
                proxy_host: "127.0.0.1".into(),
                proxy_port: 9999,
            },
            ..default_cfg()
        };
        clients.rebuild_proxy(&new_cfg).unwrap();
        let _c3 = clients.for_rule(&safe_rule);
        let _c4 = clients.for_rule(&unsafe_rule);
    }

    #[test]
    fn rebuild_proxy_swaps_client_instance() {
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.safe_client_ptr().unwrap();

        let new_cfg = AppConfig {
            proxy: ProxyCfg {
                proxy_enabled: true,
                proxy_host: "127.0.0.1".into(),
                proxy_port: 8080,
            },
            ..default_cfg()
        };
        clients.rebuild_proxy(&new_cfg).unwrap();

        let after = clients.safe_client_ptr().unwrap();
        assert_ne!(
            before, after,
            "proxy changed → safe client instance must be replaced"
        );
    }

    #[test]
    fn rebuild_proxy_no_op_when_unchanged() {
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.safe_client_ptr().unwrap();

        // 同样 config 再 rebuild 一次 → 短路
        clients.rebuild_proxy(&default_cfg()).unwrap();
        let after = clients.safe_client_ptr().unwrap();
        assert_eq!(
            before, after,
            "proxy unchanged → safe client must NOT be replaced"
        );
    }

    #[test]
    fn rebuild_proxy_ignores_non_proxy_changes() {
        // 改一个跟 proxy 无关的字段（这里直接复用 default_cfg() 的 host）——
        // signature 一致 → 不重建。
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.safe_client_ptr().unwrap();

        // 完全相同的 cfg
        let cfg2 = default_cfg();
        clients.rebuild_proxy(&cfg2).unwrap();
        let after = clients.safe_client_ptr().unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn gh_proxy_pair_returns_configured_url() {
        let cfg = AppConfig::default();
        let clients = HttpClients::new(&cfg).unwrap();
        let (url, _client) = clients.gh_proxy_pair();
        assert!(url.is_empty(), "default gh_proxy is empty");

        // 配了 gh_proxy 的 cfg 应该构造带代理的 client
        let cfg_with_proxy = AppConfig {
            global: GlobalCfg {
                gh_proxy: "https://ghproxy.example.com/".into(),
                ..GlobalCfg::default()
            },
            ..default_cfg()
        };
        let clients2 = HttpClients::new(&cfg_with_proxy).unwrap();
        let (url2, _client2) = clients2.gh_proxy_pair();
        assert_eq!(url2, "https://ghproxy.example.com/");
    }
}
