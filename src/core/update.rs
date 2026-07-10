//! 三端共享的版本更新检查。
//!
//! ## 为什么在 `core`?
//!
//! `check_latest_release` / `classify` / `is_new_version_available` 三个函数都是
//! 纯业务（无 GUI / HTTP 状态语义）。当前只有 desktop 用，但 web 将来想做
//! `GET /api/update` 端点 / CLI 想做 `sonovel update` 子命令时直接复用即可。
//!
//! ## 网络层
//!
//! 通过 [`HttpClients`] 复用同一套共享 client：
//! - `gh_proxy` 配了 → 走前向代理（启动期一次性 build）
//! - 否则走 `safe` client（占位 rule 选 `ignore_ssl=false`）
//!
//! 详见 [`check_latest_release`] 注释里的代理优先级。

use crate::http::HttpClients;
use crate::models::Rule;

/// 后台更新检查的结果。`latest_version` 和 `error` 互斥：成功时前者 Some，
/// 失败时后者 Some —— 由 [`check_latest_release`] 自身保证。
pub struct UpdateCheckResult {
    pub latest_version: Option<String>,
    pub error: Option<String>,
}

/// `classify` / [`UpdateState::drain`] 报告的语义化跃迁。
///
/// 把"通道刚返回了一条 result"翻译成"用户应该听到什么" ——
/// 调用方拿到这个 enum 后只管 push notification，不再自己 `if let` 串字段。
#[derive(Debug)]
pub enum UpdateOutcome {
    /// 当前版本 = GitHub 最新版本。
    UpToDate,
    /// 有新版本可用，附版本号字符串（保留原始 `v` 前缀，由 UI 决定是否剥）。
    NewVersion(String),
    /// 检查失败，附错误信息。
    Failed(String),
}

/// 把后台回报的 [`UpdateCheckResult`] 分类成 [`UpdateOutcome`]。
///
/// **优先级**：error > `latest_version`（后台 contract 保证两者不会同时为 Some）。
/// `env!("CARGO_PKG_VERSION")` 是 build-time 常量，比较无副作用。
///
/// ## Fallback
///
/// `latest_version == None && error == None`（空 result）兜底成 [`UpdateOutcome::Failed`]，
/// 避免静默丢事件 —— 通常意味着 GitHub API 响应 schema 变了 / 反序列化失败但没
/// 显式报错。
pub fn classify(result: &UpdateCheckResult) -> UpdateOutcome {
    if let Some(err) = &result.error {
        return UpdateOutcome::Failed(err.clone());
    }
    match &result.latest_version {
        Some(latest) if latest.trim_start_matches('v') == env!("CARGO_PKG_VERSION") => {
            UpdateOutcome::UpToDate
        }
        Some(latest) => UpdateOutcome::NewVersion(latest.clone()),
        None => UpdateOutcome::Failed("(empty result)".into()),
    }
}

/// 给定 latest release tag（含/不含 `v` 前缀都行），判断是否新于当前 build 版本。
///
/// 内部剥 `v` 前缀后与 `env!("CARGO_PKG_VERSION")` 字符串比较 —— 简单粗暴，
/// 不做 semver 解析（避免引入 `semver` crate 增加依赖图）。
pub fn is_new_version_available(latest: &str) -> bool {
    latest.trim_start_matches('v') != env!("CARGO_PKG_VERSION")
}

/// 向 GitHub API 查询最新 release 版本号。
///
/// **代理策略**（优先级从高到低）：
/// 1. `HttpClients.gh_proxy_pair()` URL 非空 → 走预构建的 GH 镜像前向代理 client
///    （构造时已含 proxy + UA，启动时一次性 build，不重复创建连接池）。
///    `gh_proxy` 检查频率极低（启动一次 + 用户手动），不构成热路径。
/// 2. 否则从共享 [`HttpClients`] 复用 `safe` client（按占位 rule 选）——
///    该 client 已包含 HTTP CONNECT 代理 / TLS session cache / 默认 headers。
/// 3. 两者都空 → `safe` client 直接走，无代理直连。
///
/// ## 失败模式
///
/// - HTTP 非 2xx → `error: Some("GitHub API 返回 HTTP {status}")`
/// - 读 body 失败 → `error: Some("读取响应失败: {err}")`
/// - 请求本身失败 → `error: Some("请求失败: {err}")`
/// - 解析 `tag_name` 失败 → `latest_version: None, error: None` —— 由 [`classify`]
///   兜底成 `Failed("(empty result)")`。
///
/// 网络错误用 `tracing::warn!` 记录（便于调试），但不返回 tracing 字符串到
/// `error` 字段（避免 debug 字符串泄漏到 UI 文案）。
pub async fn check_latest_release(http: &HttpClients) -> UpdateCheckResult {
    const URL: &str = "https://api.github.com/repos/AhJxs/so-novel-rs/releases/latest";

    let (gh_proxy_url, gh_proxy_client) = http.gh_proxy_pair();

    // GitHub API 对无 UA 的请求返回 403；gh_proxy client 构造时已设 UA；
    // safe client 分支需显式带 UA header。
    let result = if gh_proxy_url.is_empty() {
        // 共享 client 分支：复用 `safe` —— 用占位 rule（ignore_ssl=false），
        // 因为查询 GitHub API 不需要忽略证书校验。
        let client = http.for_rule(&Rule {
            ignore_ssl: false,
            ..Rule::default()
        });
        client
            .get(URL)
            .header(reqwest::header::USER_AGENT, "so-novel-rs")
            .send()
            .await
    } else {
        gh_proxy_client.get(URL).send().await
    };

    match result {
        Ok(resp) => {
            if !resp.status().is_success() {
                return UpdateCheckResult {
                    latest_version: None,
                    error: Some(format!("GitHub API 返回 HTTP {}", resp.status())),
                };
            }
            match resp.text().await {
                Ok(body) => {
                    // GitHub API 现在返回压缩 JSON（无换行）；用 serde_json 解析取 tag_name。
                    // 旧实现按行匹配 pretty-print 格式，在压缩响应下永远找不到 → 误报 "(empty result)"。
                    let tag = serde_json::from_str::<serde_json::Value>(&body)
                        .ok()
                        .and_then(|v| v.get("tag_name").and_then(|t| t.as_str()).map(String::from));
                    if tag.is_none() {
                        tracing::warn!("GitHub release response 无 tag_name: {body}");
                    }
                    UpdateCheckResult {
                        latest_version: tag,
                        error: None,
                    }
                }
                Err(e) => UpdateCheckResult {
                    latest_version: None,
                    error: Some(format!("读取响应失败: {e}")),
                },
            }
        }
        Err(e) => UpdateCheckResult {
            latest_version: None,
            error: Some(format!("请求失败: {e}")),
        },
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    // ---- classify ----

    #[test]
    fn classify_failed_takes_precedence_over_latest() {
        // 后台 contract：error / latest_version 互斥。但防御性写：error 优先
        let r = UpdateCheckResult {
            latest_version: Some("9.9.9".into()),
            error: Some("net".into()),
        };
        assert!(matches!(classify(&r), UpdateOutcome::Failed(ref m) if m == "net"));
    }

    #[test]
    fn classify_up_to_date_strips_leading_v() {
        let r = UpdateCheckResult {
            latest_version: Some(format!("v{}", env!("CARGO_PKG_VERSION"))),
            error: None,
        };
        assert!(matches!(classify(&r), UpdateOutcome::UpToDate));
    }

    #[test]
    fn classify_up_to_date_without_v_prefix() {
        // tag 可能没 v 前缀（自己 fork 时常见）
        let r = UpdateCheckResult {
            latest_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            error: None,
        };
        assert!(matches!(classify(&r), UpdateOutcome::UpToDate));
    }

    #[test]
    fn classify_new_version_when_differs() {
        let r = UpdateCheckResult {
            latest_version: Some("999.0.0".into()),
            error: None,
        };
        assert!(matches!(classify(&r), UpdateOutcome::NewVersion(ref v) if v == "999.0.0"));
    }

    #[test]
    fn classify_empty_result_becomes_failed() {
        // 后台应保证 at least one of (latest_version, error) is Some；
        // 兜底成 Failed 是为了不静默丢事件。
        let r = UpdateCheckResult {
            latest_version: None,
            error: None,
        };
        assert!(matches!(classify(&r), UpdateOutcome::Failed(_)));
    }

    // ---- is_new_version_available ----

    #[test]
    fn is_new_version_available_same_no_v() {
        assert!(!is_new_version_available(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn is_new_version_available_same_with_v() {
        assert!(!is_new_version_available(&format!(
            "v{}",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[test]
    fn is_new_version_available_different() {
        assert!(is_new_version_available("v999.0.0"));
        assert!(is_new_version_available("0.0.1"));
    }

    // ---- GitHub response parsing ----

    #[test]
    fn parse_tag_name_from_actual_github_response() {
        // 真实 /releases/latest 响应（压缩格式，无换行）。author 留最小有效结构。
        let body = r#"{"url":"https://api.github.com/repos/AhJxs/so-novel-rs/releases/341144353","html_url":"https://github.com/AhJxs/so-novel-rs/releases/tag/v0.2.3","id":341144353,"author":{"login":"github-actions[bot]","id":41898282},"node_id":"RE_kwDOS6YKu84UVXMh","tag_name":"v0.2.3","target_commitish":"master","name":"v0.2.3","draft":false,"immutable":false,"prerelease":false,"created_at":"2026-06-18T03:12:36Z","updated_at":"2026-06-18T03:28:50Z","published_at":"2026-06-18T03:28:50Z","assets":[],"tarball_url":"x","zipball_url":"y","body":"x"}"#;
        let tag = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("tag_name").and_then(|t| t.as_str()).map(String::from));
        assert_eq!(tag.as_deref(), Some("v0.2.3"));
    }

    #[test]
    fn parse_tag_name_missing_yields_none() {
        let body = r#"{"html_url":"x","name":"v0.2.3"}"#;
        let tag = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v.get("tag_name").and_then(|t| t.as_str()).map(String::from));
        assert!(tag.is_none());
    }
}
