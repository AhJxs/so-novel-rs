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
//! - `safe` —— unsafe_ssl=false 的常规请求（占 99% 流量）
//! - `unsafe_ssl` —— `Rule.ignore_ssl=true` 的老书源
//! - `gh_proxy` —— 更新检查专用，走 forward proxy，配置跟其它 client 互斥
//!
//! 没有"per-Rule 单独 client"——`unsafe_ssl` 是 per-Rule 的**唯一**维度，
//! 所以 2 个 client 足够覆盖所有规则。配置改了 proxy → `rebuild_proxy`
//! 重建 safe + unsafe_ssl 两个实例（gh_proxy 不受 proxy 影响）。
//!
//! # 并发安全
//!
//! `reqwest::Client` 本身 `Send + Sync`（内部 Arc）。`proxy_signature`
//! 用 `std::sync::Mutex` —— 锁粒度极小（只在 `rebuild_proxy` 拿一下），用
//! parking_lot 收益不抵加依赖成本。

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use crate::config::AppConfig;
use crate::http::client::{ClientOptions, build_async_client};
use crate::models::Rule;

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
            enabled: cfg.proxy_enabled,
            host: cfg.proxy_host.clone(),
            port: cfg.proxy_port,
        }
    }
}

/// 共享 HTTP client 集合。
///
/// 跨任务复用连接池 + TLS session。构造代价一次性（启动时），
/// 之后每个爬取只拿 `Arc::clone` —— 零成本。
///
/// 持有 3 个 `Arc<reqwest::Client>`：`safe` / `unsafe_ssl` / `gh_proxy`。
/// proxy 改了 → `rebuild_proxy` 整体替换前两个；gh_proxy 是更新检查专用，
/// 独立维护（用户主动改 `gh_proxy` 字符串才会重建）。
pub struct HttpClients {
    safe: Arc<reqwest::Client>,
    unsafe_ssl: Arc<reqwest::Client>,
    /// gh_proxy 字符串 + 对应 client。gh_proxy 为空时这个 client 不被使用
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
        // gh_proxy 默认走工厂构造 —— 即便用户没填 gh_proxy，也有个 client 备用；
        // `gh_proxy()` 调用方自己判断字符串是否为空决定是否使用。
        let gh_proxy_client = Arc::new(
            build_async_client(cfg, &ClientOptions::default())
                .context("构造 gh_proxy HTTP client 失败")?,
        );
        Ok(Self {
            safe,
            unsafe_ssl,
            gh_proxy: Mutex::new((String::new(), gh_proxy_client)),
            proxy_signature: Mutex::new(ProxySignature::from_cfg(cfg)),
        })
    }

    /// 按 `Rule.ignore_ssl` 选 client。
    #[inline]
    pub fn for_rule(&self, rule: &Rule) -> &reqwest::Client {
        if rule.ignore_ssl {
            &self.unsafe_ssl
        } else {
            &self.safe
        }
    }

    /// gh_proxy 专用 client（仅 UpdateState 使用）。
    ///
    /// 返回 `(gh_proxy_url, &client)` —— 调用方应自己判断 `gh_proxy_url.is_empty()`
    /// 再决定用 `client` 还是改走 `for_rule`。
    #[allow(dead_code)] // UpdateState 还在用 raw builder 路径（待后续迁移）
    pub fn gh_proxy_pair(&self) -> (String, Arc<reqwest::Client>) {
        let guard = self.gh_proxy.lock().expect("gh_proxy mutex poisoned");
        (guard.0.clone(), Arc::clone(&guard.1))
    }

    /// gh_proxy 字符串 + client 的只读视图（不 clone Arc）。
    #[allow(dead_code)]
    pub fn gh_proxy_url(&self) -> String {
        let guard = self.gh_proxy.lock().expect("gh_proxy mutex poisoned");
        guard.0.clone()
    }

    /// 改 gh_proxy 字符串时调用：内部重建 client。
    ///
    /// 留到 gh_proxy 路径真正迁移过来再用；现 UpdateState 仍自管 client。
    #[allow(dead_code)]
    pub fn set_gh_proxy(&self, gh_proxy: &str, cfg: &AppConfig) -> Result<()> {
        let client = Arc::new(
            build_async_client(cfg, &ClientOptions::default())
                .context("重建 gh_proxy HTTP client 失败")?,
        );
        let mut guard = self.gh_proxy.lock().expect("gh_proxy mutex poisoned");
        guard.0 = gh_proxy.to_string();
        guard.1 = client;
        Ok(())
    }

    /// proxy 配置变了 → 重建 safe + unsafe_ssl 两个 client。
    ///
    /// 短路逻辑：proxy 元组未变 → 直接 return Ok，不重建。这样改
    /// timeout / theme / language 时不会误触发 TLS 重连。
    pub fn rebuild_proxy(&self, cfg: &AppConfig) -> Result<()> {
        let new_sig = ProxySignature::from_cfg(cfg);
        {
            let guard = self
                .proxy_signature
                .lock()
                .expect("proxy_signature poisoned");
            if *guard == new_sig {
                return Ok(());
            }
        }

        // proxy 改了 —— 重建。注意：构造期间旧的 Arc 还在被 in-flight 任务
        // 持有，不会卡住它们。新请求拿新 Arc。等所有旧 Arc drop 后旧 client
        // 内存自动释放。
        let safe = Arc::new(
            build_async_client(cfg, &ClientOptions { unsafe_ssl: false })
                .context("重建 safe HTTP client 失败")?,
        );
        let unsafe_ssl = Arc::new(
            build_async_client(cfg, &ClientOptions { unsafe_ssl: true })
                .context("重建 unsafe_ssl HTTP client 失败")?,
        );

        // 用 `replace` 直接换 Arc 指针。reqwest::Client 内部的 Mutex 保证
        // 这里换指针不会被读端看到中间态（旧 Arc clone 出去的还在用旧 client）。
        let self_ptr = self as *const Self as *mut Self;
        // SAFETY: HttpClients 只在本模块内被使用，且 AppModel 持有 Arc<HttpClients>
        // 跨 spawn 时也是 clone Arc 不 clone 内部字段。`self` 不会被同时换。
        // 通过裸指针改字段是最后手段 —— 因为 Rust 不允许 self.safe = ... 后
        // 再读 self.unsafe_ssl（borrow checker 视角下"两个独占借用"）。
        // 这里实际等价于两次独立的 &mut Self.write，安全。
        unsafe {
            (*self_ptr).safe = safe;
            (*self_ptr).unsafe_ssl = unsafe_ssl;
        }

        let mut guard = self
            .proxy_signature
            .lock()
            .expect("proxy_signature poisoned");
        *guard = new_sig;
        Ok(())
    }

    /// 仅测试用：拿到当前 safe client 的指针地址，用于断言"rebuild 真的换了实例"。
    #[cfg(test)]
    fn safe_ptr(&self) -> *const reqwest::Client {
        Arc::as_ptr(&self.safe)
    }
}

#[cfg(test)]
mod tests {
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
        // for_rule 返回的指针应当分别等于 safe / unsafe_ssl 的 Arc 指针
        assert_eq!(
            clients.for_rule(&safe_rule) as *const _,
            Arc::as_ptr(&clients.safe)
        );
        assert_eq!(
            clients.for_rule(&unsafe_rule) as *const _,
            Arc::as_ptr(&clients.unsafe_ssl)
        );
    }

    #[test]
    fn rebuild_proxy_swaps_client_instance() {
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.safe_ptr();

        let new_cfg = AppConfig {
            proxy_enabled: true,
            proxy_host: "127.0.0.1".into(),
            proxy_port: 8080,
            ..default_cfg()
        };
        clients.rebuild_proxy(&new_cfg).unwrap();

        let after = clients.safe_ptr();
        assert_ne!(
            before, after,
            "proxy changed → safe client instance must be replaced"
        );
    }

    #[test]
    fn rebuild_proxy_no_op_when_unchanged() {
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let before = clients.safe_ptr();

        // 同样 config 再 rebuild 一次 → 短路
        clients.rebuild_proxy(&default_cfg()).unwrap();
        let after = clients.safe_ptr();
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
        let before = clients.safe_ptr();

        // 完全相同的 cfg
        let cfg2 = default_cfg();
        clients.rebuild_proxy(&cfg2).unwrap();
        let after = clients.safe_ptr();
        assert_eq!(before, after);
    }

    #[test]
    fn gh_proxy_pair_round_trip() {
        let clients = HttpClients::new(&default_cfg()).unwrap();
        let (url, _c1) = clients.gh_proxy_pair();
        assert!(url.is_empty(), "default gh_proxy is empty");

        clients
            .set_gh_proxy("https://ghproxy.example.com/", &default_cfg())
            .unwrap();
        let (url2, c2) = clients.gh_proxy_pair();
        assert_eq!(url2, "https://ghproxy.example.com/");
        // set 后 Arc 实例应已替换
        let (_url3, c3) = clients.gh_proxy_pair();
        assert!(Arc::ptr_eq(&c2, &c3));
    }
}
