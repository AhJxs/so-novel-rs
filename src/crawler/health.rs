//! 书源连通性检测。对应 Java
//! `util.SourceUtils#getActivatedSourcesWithAvailabilityCheck`。
//!
//! 行为：每源发一个 `HEAD` 请求带 5s 超时；记录 (延迟 ms, http_status, error)；
//! 通过 mpsc 把单源结果实时推回 UI（先返回的源先点亮）。

use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, USER_AGENT};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

use crate::http::client::{ClientOptions, build_async_client};
use crate::http::ua::random_ua;
use crate::models::Rule;

/// 单源探测结果。
#[derive(Debug, Clone)]
pub struct SourceHealth {
    pub source_id: i32,
    pub source_name: String,
    /// HTTP 响应状态码。请求失败时为 None。
    pub http_status: Option<u16>,
    /// 完整往返延迟（毫秒）。请求失败时仍记录已耗费时间。
    pub delay_ms: u64,
    /// 错误信息（请求失败时填）。
    pub error: Option<String>,
}

/// 单源健康判定（domain-level，UI 自行映射到主题色 / StatusKind）。
///
/// 跟 GUI 层的 `gpui_app::components::StatusKind` 解耦——`crawler` 不应依赖 `gpui_app`。
/// `sources` page 拿 `HealthStatus` 后再做 1 行 `match` 转成 `StatusKind`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// 2xx：探活成功。
    Ok,
    /// 3xx：源在跳转，浏览器能跟、爬虫可能不能跟。给用户一个"看似 OK 但要警惕"的状态。
    Redirect,
    /// 4xx/5xx：源回了一个非成功状态码。
    BadResponse,
    /// 探活函数本身报告了 error（如 Client 构建失败）。
    ProbeError,
    /// 探活失败且没拿到 HTTP 响应（DNS / 超时 / TLS 等网络层问题）。
    NetworkError,
}

impl SourceHealth {
    /// 把探测结果归类成 5 个语义状态之一。
    pub fn classify(&self) -> HealthStatus {
        if self.error.is_some() {
            return HealthStatus::ProbeError;
        }
        match self.http_status {
            None => HealthStatus::NetworkError,
            Some(s) if (200..300).contains(&s) => HealthStatus::Ok,
            Some(s) if (300..400).contains(&s) => HealthStatus::Redirect,
            Some(_) => HealthStatus::BadResponse,
        }
    }

    /// 本地化显示文本。延迟 2xx 只显示 ms；4xx/5xx 同时显示状态码 + 延迟；
    /// 探活失败 / 无响应 各走独立 i18n key。
    pub fn label(&self) -> String {
        if self.error.is_some() {
            return crate::i18n::ts("Sources.health.error").to_string();
        }
        match self.http_status {
            // 2xx —— 测速最关心的是延迟，状态码冗余。
            Some(s) if (200..300).contains(&s) => crate::i18n::ts_fmt(
                "Sources.health.latency",
                &[("ms", &self.delay_ms.to_string())],
            )
            .to_string(),
            // 3xx/4xx/5xx —— 状态码 + 延迟并存，方便区分「慢但通（3xx 跳转）」和
            // 「真的失败（4xx/5xx）」。`ts_fmt` 替换 2 个占位符（不能直接 `format!`
            // 拼字符串，否则切语言后占位符翻译也跟着拼，顺序会乱）。
            Some(s) => crate::i18n::ts_fmt(
                "Sources.health.http_status",
                &[
                    ("status", &s.to_string()),
                    ("ms", &self.delay_ms.to_string()),
                ],
            )
            .to_string(),
            // 源错误但没 HTTP 响应（DNS 失败 / 超时 等）—— 调试输出太长塞不进 StatusBadge，
            // 用一句"网络错误"代替。原来的 `format!("{:?}", h.error)` 会把 anyhow 内部
            // chain 全部展开，超长且对用户没意义。
            None => crate::i18n::ts("Sources.health.network_error").to_string(),
        }
    }
}

/// 并发探测一组规则；每源结果通过 `tx` 实时回推（顺序不保证，UI 用 source_id 关联）。
///
/// 完成后通道关闭（tx drop），UI 端 try_recv 看到 Disconnected 即知道全部跑完。
pub async fn check_sources_health(
    cfg: Arc<crate::config::AppConfig>,
    rules: Vec<Rule>,
    tx: mpsc::UnboundedSender<SourceHealth>,
) {
    let mut set: JoinSet<()> = JoinSet::new();

    for rule in rules {
        let cfg = Arc::clone(&cfg);
        let tx = tx.clone();
        set.spawn(async move {
            // 直接在 tokio task 里 async 探测。不用 spawn_blocking + blocking client ——
            // 后者会在工作线程 drop Client，触发 reqwest::blocking 已知 panic
            // （见 http/client.rs 的反模式警告 + search_state.rs 的 ignore 回归测试）。
            let result = probe_one(&cfg, &rule).await;
            let _ = tx.send(result);
        });
    }

    while set.join_next().await.is_some() {}
    // tx drop（与 set 同生命周期），UI 端通道收尾。
}

async fn probe_one(cfg: &crate::config::AppConfig, rule: &Rule) -> SourceHealth {
    let started = Instant::now();
    let opts = ClientOptions {
        unsafe_ssl: rule.ignore_ssl,
    };
    let client = match build_async_client(cfg, &opts) {
        Ok(c) => c,
        Err(e) => {
            return SourceHealth {
                source_id: rule.id,
                source_name: rule.name.clone(),
                http_status: None,
                delay_ms: started.elapsed().as_millis() as u64,
                error: Some(format!("client: {e:#}")),
            };
        }
    };

    let result = client
        .head(&rule.url)
        .timeout(Duration::from_secs(5))
        .header(USER_AGENT, random_ua())
        .header(
            ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .await;

    let delay_ms = started.elapsed().as_millis() as u64;
    match result {
        Ok(resp) => SourceHealth {
            source_id: rule.id,
            source_name: rule.name.clone(),
            http_status: Some(resp.status().as_u16()),
            delay_ms,
            error: None,
        },
        Err(e) => SourceHealth {
            source_id: rule.id,
            source_name: rule.name.clone(),
            http_status: None,
            delay_ms,
            error: Some(short_err(&e)),
        },
    }
}

fn short_err(e: &reqwest::Error) -> String {
    let s = format!("{e}");
    // 取第一行避免堆叠多层包装错误把 UI 撑得太长
    s.lines()
        .next()
        .unwrap_or("(空错误)")
        .chars()
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::models::Rule;

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_rules_finishes_immediately() {
        let cfg = Arc::new(AppConfig::default());
        let (tx, mut rx) = mpsc::unbounded_channel::<SourceHealth>();
        check_sources_health(cfg, Vec::new(), tx).await;
        assert!(rx.try_recv().is_err(), "no rules → no events");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unreachable_host_yields_error_with_id() {
        // 用一个保留地址，远端肯定不通；验证流程把错误封装到 SourceHealth。
        let cfg = Arc::new(AppConfig::default());
        let rule = Rule {
            id: 42,
            name: "test".to_string(),
            // 保留地址段 192.0.2.0/24（RFC 5737 文档示例，不会路由）
            url: "http://192.0.2.1/".to_string(),
            ..Rule::default()
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<SourceHealth>();
        check_sources_health(cfg, vec![rule], tx).await;

        let h = rx.try_recv().expect("should yield exactly one event");
        assert_eq!(h.source_id, 42);
        assert_eq!(h.source_name, "test");
        // 任何超时 / 网络错误都行；只断言"有错误"
        assert!(h.error.is_some(), "expected error; got {h:?}");
        assert!(h.http_status.is_none());
        // 通道关闭
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn classify_maps_status_codes() {
        let mk = |status: Option<u16>, error: Option<&str>| SourceHealth {
            source_id: 1,
            source_name: "x".to_string(),
            http_status: status,
            delay_ms: 100,
            error: error.map(String::from),
        };
        // 2xx → Ok
        assert_eq!(mk(Some(200), None).classify(), HealthStatus::Ok);
        assert_eq!(mk(Some(204), None).classify(), HealthStatus::Ok);
        assert_eq!(mk(Some(299), None).classify(), HealthStatus::Ok);
        // 3xx → Redirect
        assert_eq!(mk(Some(301), None).classify(), HealthStatus::Redirect);
        assert_eq!(mk(Some(304), None).classify(), HealthStatus::Redirect);
        // 4xx/5xx → BadResponse
        assert_eq!(mk(Some(404), None).classify(), HealthStatus::BadResponse);
        assert_eq!(mk(Some(500), None).classify(), HealthStatus::BadResponse);
        // 没响应 → NetworkError
        assert_eq!(mk(None, None).classify(), HealthStatus::NetworkError);
        // 有 error（任何 status / 无 status）→ ProbeError 优先
        assert_eq!(
            mk(Some(200), Some("oops")).classify(),
            HealthStatus::ProbeError
        );
        assert_eq!(
            mk(None, Some("timeout")).classify(),
            HealthStatus::ProbeError
        );
    }

    #[test]
    fn label_uses_distinct_keys_per_branch() {
        let mk = |status: Option<u16>, delay_ms: u64, error: Option<&str>| SourceHealth {
            source_id: 1,
            source_name: "x".to_string(),
            http_status: status,
            delay_ms,
            error: error.map(String::from),
        };
        // 2xx → 延迟模板，状态码不出现在文案里
        let ok_label = mk(Some(200), 123, None).label();
        assert!(
            ok_label.contains("123"),
            "2xx label should embed ms: {ok_label}"
        );
        // 4xx/5xx → 状态码 + 延迟双占位符
        let bad_label = mk(Some(503), 456, None).label();
        assert!(
            bad_label.contains("503"),
            "bad label should embed status: {bad_label}"
        );
        assert!(
            bad_label.contains("456"),
            "bad label should embed ms: {bad_label}"
        );
        // ProbeError vs NetworkError 走不同 key，长度 / 内容至少有一个不同
        let probe = mk(Some(200), 0, Some("oops")).label();
        let net = mk(None, 0, None).label();
        assert_ne!(probe, net, "probe error and network error should differ");
    }
}
