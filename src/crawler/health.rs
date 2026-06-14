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

use crate::http::client::{build_blocking_client, ClientOptions};
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
            let result = tokio::task::spawn_blocking(move || probe_one(&cfg, &rule))
                .await
                .unwrap_or_else(|join_err| SourceHealth {
                    source_id: -1,
                    source_name: format!("(join failed: {join_err})"),
                    http_status: None,
                    delay_ms: 0,
                    error: Some(format!("spawn_blocking: {join_err}")),
                });
            let _ = tx.send(result);
        });
    }

    while set.join_next().await.is_some() {}
    // tx drop（与 set 同生命周期），UI 端通道收尾。
}

fn probe_one(cfg: &crate::config::AppConfig, rule: &Rule) -> SourceHealth {
    let started = Instant::now();
    let opts = ClientOptions {
        unsafe_ssl: rule.ignore_ssl,
    };
    let client = match build_blocking_client(cfg, &opts) {
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
        .send();

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
}
