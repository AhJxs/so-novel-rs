//! 共享 HTTP client 集合。
//!
//! `reqwest::Client` 持有连接池 + TLS session cache，每次 `build()` 都是全新实例 =
//! 重置 keep-alive + 重做 TLS 握手。故按"配置维度"维护固定实例：`safe` /
//! `unsafe_ssl`（`Rule.ignore_ssl=true` 的老书源）/ `gh_proxy`（更新检查专用）。
//!
//! proxy / `danger_accept_invalid_certs` 构造后**不能** in-place 改，维度变了只能整体 rebuild；
//! `proxy_signature` 短路"没真改就不重建"（用 `std::sync::Mutex`，不值得换 `parking_lot`）。

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

/// 共享 HTTP client 集合。构造代价一次性（启动时），之后每个爬取只 `Arc::clone`。
pub struct HttpClients {
    /// safe / `unsafe_ssl` 两个 client：`for_rule` 只 read，`rebuild_proxy` 用 write 换 Arc。
    /// clone ≈ `Arc::clone`（`reqwest::Client` 内部本就是 Arc）。
    clients: RwLock<(Arc<reqwest::Client>, Arc<reqwest::Client>)>,
    /// `gh_proxy` 字符串 + 对应 `client。gh_proxy` 为空时不用它但保留 ——
    /// 用户从有 → 无切换时不会出现"没 client 可用"的窗口。
    gh_proxy: Mutex<(String, Arc<reqwest::Client>)>,
    /// 当前生效的 proxy 元组；用于短路"没真改就不重建"。
    proxy_signature: Mutex<ProxySignature>,
}

impl HttpClients {
    /// 兜底空集：两个 client 都是 `reqwest::Client::new()`，`gh_proxy` 为空串。
    ///
    /// `core::bootstrap::load_context` 在 proxy strip 后仍失败时最后兜底用 ——
    /// 比 panic 友好（前端仍能进 UI，下载页报网络错即可）。日常路径都走 [`Self::new`]。
    pub fn empty() -> Self {
        let bare = Arc::new(reqwest::Client::new());
        Self {
            clients: RwLock::new((Arc::clone(&bare), Arc::clone(&bare))),
            gh_proxy: Mutex::new((String::new(), bare)),
            proxy_signature: Mutex::new(ProxySignature {
                enabled: false,
                host: String::new(),
                port: 0,
            }),
        }
    }

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
        // gh_proxy client：配了就用它做 forward proxy，否则退化为普通 client。
        // 调用方自己判断 URL 是否为空决定是否使用。
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
    /// 返回 owned 而非 `&`：底层是 `RwLock`，`RwLockReadGuard` 不能泄漏出引用；
    /// clone 只做 refcount bump，几乎零开销。
    #[inline]
    pub fn for_rule(&self, rule: &Rule) -> reqwest::Client {
        rw_read_or("for_rule", &self.clients).map_or_else(
            |_| {
                // 锁 poison：退路拿 unsafe_ssl（哪怕可能坏，也比 worker panic 拖死整个 web 好）。
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

    /// `gh_proxy` 专用 client（更新检查用）。返回 `(gh_proxy_url, Arc<client>)`，
    /// 调用方需自行判断 `gh_proxy_url.is_empty()` 决定是否改走 `for_rule`。
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
    /// proxy 配置变了 → 重建 safe + `unsafe_ssl` 两个 client（`gh_proxy` 不受 proxy 影响）。
    pub fn rebuild_proxy(&self, cfg: &AppConfig) -> Result<()> {
        let new_sig = ProxySignature::from_cfg(cfg);
        let old_sig = mutex_or("rebuild_proxy:read_sig", &self.proxy_signature)
            .map_err(anyhow::Error::msg)
            .context("proxy_signature 锁 poison")?
            .clone();
        if old_sig == new_sig {
            return Ok(());
        }

        // proxy 改了 —— 用 RwLock::write 原子替换两个 Arc。读端在 rebuild 期间短暂阻塞
        // （构造 client 不含 IO）；已 clone 到 in-flight 任务的旧 Arc 不受影响，自然 drop。
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

    /// 仅测试用：拿 `safe` client 的 Arc 内部指针，用于断言"rebuild 真的换了实例"。
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
        // 与 proxy 无关的字段变了 → signature 一致 → 不重建。
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
