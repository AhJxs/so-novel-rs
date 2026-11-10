//! 共享 HTTP client 集合。
//!
//! `Client` 持有连接池 + TLS session cache，每次 `build()` 都是全新实例 =
//! 重置 keep-alive + 重做 TLS 握手。故按"配置维度"维护固定实例：`safe` /
//! `unsafe_ssl`（`Rule.ignore_ssl=true` 的老书源）/ `gh_proxy`（更新检查专用）。
//!
//! proxy / `danger_accept_invalid_certs` 构造后**不能** in-place 改，维度变了只能整体 rebuild；
//! `proxy_signature` 短路"没真改就不重建"（用 `std::sync::Mutex`，不值得换 `parking_lot`）。

#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Client;

use crate::config::AppConfig;
#[cfg(test)]
use crate::config::{GlobalConfig, ProxyConfig, ProxyMode};
use crate::http::client::{ClientOptions, build_async_client};
use crate::models::Rule;
use crate::utils::lock::{mutex_or, rw_read_or, rw_write_or};

/// 当前生效的 proxy 快照。`rebuild_proxy` 用它判断"配置是否真的变了"。
///
/// 存**解析后的 URL** 而不是原始字段：`System` 模式下 host / port 字段根本没动，
/// 但注册表里的值可能刚被 Clash 改过 —— 只比对字段会漏掉这种真实变化。
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProxySignature {
    resolved: Option<String>,
}

impl ProxySignature {
    fn from_config(cfg: &AppConfig) -> Self {
        Self {
            resolved: crate::http::system_proxy::resolve_proxy_url(&cfg.proxy),
        }
    }
}

impl From<&AppConfig> for ProxySignature {
    fn from(cfg: &AppConfig) -> Self {
        Self::from_config(cfg)
    }
}

/// `gh_proxy` 专用 URL + client（更新检查用）。
pub struct GhProxy {
    /// 配置的 GitHub 代理前缀；空串表示未配置。
    pub url: String,
    /// 与该前缀配套的 `reqwest::Client`。
    pub client: reqwest::Client,
}

/// 共享 HTTP client 集合。构造代价一次性（启动时）；后台任务在 `spawn` 前用
/// [`Self::for_rule`] 取出 owned `reqwest::Client`（clone 只是 refcount bump）。
pub struct HttpClients {
    /// safe / `unsafe_ssl` 两个 client：`for_rule` 只 read，`rebuild_proxy` 用 write 换实例。
    /// 返回的 clone 只是 refcount bump（`Client` 内部本就是 Arc）。
    clients: RwLock<(Client, Client)>,
    /// `gh_proxy` 字符串 + 对应 `client。gh_proxy` 为空时不用它但保留 ——
    /// 用户从有 → 无切换时不会出现"没 client 可用"的窗口。
    gh_proxy: Mutex<(String, Client)>,
    /// 当前生效的 proxy 元组；用于短路"没真改就不重建"。
    proxy_signature: Mutex<ProxySignature>,
    /// 测试用：每次成功替换 client 实例后自增，替代原先比对 `Arc` 指针。
    #[cfg(test)]
    generation: AtomicU64,
}

impl HttpClients {
    /// 兜底空集：两个 client 都是 `Client::new()`，`gh_proxy` 为空串。
    ///
    /// `core::bootstrap::load_context` 在 proxy strip 后仍失败时最后兜底用 ——
    /// 比 panic 友好（UI 仍能进，下载时报网络错即可）。日常路径都走 [`Self::new`]。
    pub fn empty() -> Self {
        let bare = Client::new();
        Self {
            clients: RwLock::new((bare.clone(), bare.clone())),
            gh_proxy: Mutex::new((String::new(), bare)),
            proxy_signature: Mutex::new(ProxySignature { resolved: None }),
            #[cfg(test)]
            generation: AtomicU64::new(0),
        }
    }

    /// 从 `AppConfig` 构造初始 client 集合。
    pub fn new(cfg: &AppConfig) -> Result<Self> {
        let safe = build_async_client(cfg, &ClientOptions { unsafe_ssl: false })
            .context("构造 safe HTTP client 失败")?;
        let unsafe_ssl = build_async_client(cfg, &ClientOptions { unsafe_ssl: true })
            .context("构造 unsafe_ssl HTTP client 失败")?;
        // gh_proxy client：配了就用它做 forward proxy，否则退化为普通 client。
        // 调用方自己判断 URL 是否为空决定是否使用。
        let gh_proxy_url = cfg.global.gh_proxy.trim().to_string();
        let gh_proxy_client = if gh_proxy_url.is_empty() {
            build_async_client(cfg, &ClientOptions::default())
                .context("构造 gh_proxy HTTP client 失败")?
        } else {
            let mut builder = Client::builder()
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
            builder.build().context("构造 gh_proxy HTTP client 失败")?
        };
        Ok(Self {
            clients: RwLock::new((safe, unsafe_ssl)),
            gh_proxy: Mutex::new((gh_proxy_url, gh_proxy_client)),
            proxy_signature: Mutex::new(ProxySignature::from_config(cfg)),
            #[cfg(test)]
            generation: AtomicU64::new(0),
        })
    }

    /// 按 `Rule.ignore_ssl` 选 client。
    ///
    /// 返回 owned 而非 `&`：底层是 `RwLock`，`RwLockReadGuard` 不能泄漏出引用；
    /// clone 只做 refcount bump，几乎零开销。
    #[inline]
    pub fn for_rule(&self, rule: &Rule) -> Client {
        rw_read_or("for_rule", &self.clients).map_or_else(
            |_| {
                // 锁 poison：退路拿 unsafe_ssl（哪怕可能坏，也比 worker panic 拖垮整个进程好）。
                // 二次 read 仍失败则返 Client::new() 作 last resort。
                self.clients.read().map_or_else(
                    |_| {
                        tracing::error!("for_rule: 二次 read 仍失败，返回 Client::new()");
                        Client::new()
                    },
                    |g| g.1.clone(),
                )
            },
            |guard| {
                if rule.ignore_ssl {
                    guard.1.clone()
                } else {
                    guard.0.clone()
                }
            },
        )
    }

    /// `gh_proxy` 专用 URL + client（更新检查用）。调用方需自行判断
    /// [`GhProxy::url`] 是否为空决定是否改走 [`Self::for_rule`]。
    pub fn gh_proxy_pair(&self) -> GhProxy {
        // 锁 poison：返空配置 + safe client（调用方看到空 url 会走 for_rule 路径，
        // 不会用坏掉的 gh_proxy client）。
        mutex_or("gh_proxy_pair", &self.gh_proxy).map_or_else(
            |_| {
                let fallback = self
                    .clients
                    .read()
                    .ok()
                    .map_or_else(Client::new, |g| g.0.clone());
                GhProxy {
                    url: String::new(),
                    client: fallback,
                }
            },
            |guard| GhProxy {
                url: guard.0.clone(),
                client: guard.1.clone(),
            },
        )
    }
    /// proxy 配置变了 → 重建 safe + `unsafe_ssl` 两个 client（`gh_proxy` 不受 proxy 影响）。
    pub fn rebuild_proxy(&self, cfg: &AppConfig) -> Result<()> {
        let new_sig = ProxySignature::from_config(cfg);
        let old_sig = mutex_or("rebuild_proxy:read_sig", &self.proxy_signature)
            .map_err(anyhow::Error::msg)
            .context("proxy_signature 锁 poison")?
            .clone();
        if old_sig == new_sig {
            return Ok(());
        }

        // proxy 改了 —— 用 RwLock::write 原子替换两个 client。读端在 rebuild 期间短暂阻塞
        // （构造 client 不含 IO）；已 clone 到 in-flight 任务的旧 client 不受影响，自然 drop。
        let safe = build_async_client(cfg, &ClientOptions { unsafe_ssl: false })
            .context("重建 safe HTTP client 失败")?;
        let unsafe_ssl = build_async_client(cfg, &ClientOptions { unsafe_ssl: true })
            .context("重建 unsafe_ssl HTTP client 失败")?;
        {
            let mut guard = rw_write_or("rebuild_proxy:write_clients", &self.clients)
                .map_err(anyhow::Error::msg)
                .context("clients 锁 poison")?;
            guard.0 = safe;
            guard.1 = unsafe_ssl;
        }
        // 替换成功才自增；测试据此断言"真的换了实例"。
        #[cfg(test)]
        self.generation.fetch_add(1, Ordering::Relaxed);

        {
            let mut guard = mutex_or("rebuild_proxy:write_sig", &self.proxy_signature)
                .map_err(anyhow::Error::msg)
                .context("proxy_signature 锁 poison")?;
            *guard = new_sig;
        }
        Ok(())
    }

    /// 仅测试用：client 实例替换次数，用于断言"rebuild 真的换了实例"。
    #[cfg(test)]
    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
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
            proxy: ProxyConfig {
                proxy_mode: ProxyMode::Manual,
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
    fn rebuild_proxy_advances_generation_on_change() {
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.generation();

        let new_cfg = AppConfig {
            proxy: ProxyConfig {
                proxy_mode: ProxyMode::Manual,
                proxy_host: "127.0.0.1".into(),
                proxy_port: 8080,
            },
            ..default_cfg()
        };
        clients.rebuild_proxy(&new_cfg).unwrap();

        let after = clients.generation();
        assert_eq!(
            after,
            before + 1,
            "proxy changed → client instances must be replaced exactly once"
        );
    }

    #[test]
    fn rebuild_proxy_no_op_when_unchanged() {
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.generation();

        // 同样 config 再 rebuild 一次 → 短路
        clients.rebuild_proxy(&default_cfg()).unwrap();
        let after = clients.generation();
        assert_eq!(
            before, after,
            "proxy unchanged → clients must NOT be replaced"
        );
    }

    #[test]
    fn rebuild_proxy_ignores_non_proxy_changes() {
        // 与 proxy 无关的字段变了 → signature 一致 → 不重建。
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.generation();

        // 完全相同的 cfg
        let cfg2 = default_cfg();
        clients.rebuild_proxy(&cfg2).unwrap();
        let after = clients.generation();
        assert_eq!(before, after);
    }

    #[test]
    fn gh_proxy_pair_returns_configured_url() {
        let cfg = AppConfig::default();
        let clients = HttpClients::new(&cfg).unwrap();
        let gh_proxy = clients.gh_proxy_pair();
        assert!(gh_proxy.url.is_empty(), "default gh_proxy is empty");

        // 配了 gh_proxy 的 cfg 应该构造带代理的 client
        let cfg_with_proxy = AppConfig {
            global: GlobalConfig {
                gh_proxy: "https://ghproxy.example.com/".into(),
                ..GlobalConfig::default()
            },
            ..default_cfg()
        };
        let clients2 = HttpClients::new(&cfg_with_proxy).unwrap();
        let gh_proxy2 = clients2.gh_proxy_pair();
        assert_eq!(gh_proxy2.url, "https://ghproxy.example.com/");
    }

    #[test]
    fn rebuild_proxy_from_mode_none_to_blank_host_manual_is_noop() {
        // None 模式与「Manual + 空 host」解析结果都是 None —— signature 相等，
        // 不该白重建 client（这也是 `empty()` 之后第一次 rebuild 不再重建的原因）。
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.generation();

        let cfg = AppConfig {
            proxy: ProxyConfig {
                proxy_mode: ProxyMode::Manual,
                proxy_host: String::new(),
                proxy_port: 7890,
            },
            ..default_cfg()
        };
        clients.rebuild_proxy(&cfg).unwrap();
        assert_eq!(before, clients.generation());
    }
}
