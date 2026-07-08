//! 版本更新检查状态。

use tokio::sync::mpsc;

use crate::http::HttpClients;
use crate::models::Rule;

#[derive(Default)]
pub struct UpdateState {
    /// 是否正在检查。
    pub checking: bool,
    /// 最新版本号（GitHub release `tag_name`）。
    pub latest_version: Option<String>,
    /// 检查失败的错误信息。
    pub error: Option<String>,
    /// 后台推送的接收端。
    pub rx: Option<mpsc::UnboundedReceiver<UpdateCheckResult>>,
}

/// 后台更新检查的结果。
pub struct UpdateCheckResult {
    pub latest_version: Option<String>,
    pub error: Option<String>,
}

/// `UpdateState::drain` 报告的语义化跃迁。
///
/// 把"通道刚返回了一条 result"翻译成"用户应该听到什么" ——
/// 调用方拿到这个 enum 后只管 push notification，不再自己 `if let` 串字段。
#[derive(Debug)]
pub enum UpdateOutcome {
    /// 当前版本 = GitHub 最新版本。
    UpToDate,
    /// 有新版本可用，附版本号字符串（已去掉开头的 `v`）。
    NewVersion(String),
    /// 检查失败，附错误信息。
    Failed(String),
}

impl UpdateState {
    /// 排空通道；只在状态刚刚跃迁到终态时返回 [`Some(UpdateOutcome)`]，
    /// 中间状态（无事件 / 通道断开但无 result）返回 `None`。
    pub fn drain(&mut self) -> Option<UpdateOutcome> {
        let rx = self.rx.as_mut()?;
        match rx.try_recv() {
            Ok(result) => {
                self.checking = false;
                self.latest_version.clone_from(&result.latest_version);
                self.error.clone_from(&result.error);
                self.rx = None;
                Some(classify(&result))
            }
            Err(mpsc::error::TryRecvError::Empty) => None,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                self.checking = false;
                self.rx = None;
                None
            }
        }
    }

    /// 检查完成后 `latest_version` 与当前版本不同时为 true —— Settings 页据此把
    /// "检查更新"按钮切换成"下载新版"。`v` 前缀按 `classify` 同款方式剥除。
    pub fn is_new_version_available(&self) -> bool {
        self.latest_version
            .as_ref()
            .is_some_and(|latest| latest.trim_start_matches('v') != env!("CARGO_PKG_VERSION"))
    }
}

/// 把后台回报的 [`UpdateCheckResult`] 分类成 [`UpdateOutcome`]。
///
/// 优先级：error > `latest_version（后台` contract 保证两者不会同时为 Some）。
/// `env!("CARGO_PKG_VERSION")` 是 build-time 常量，比较无副作用。
fn classify(result: &UpdateCheckResult) -> UpdateOutcome {
    if let Some(err) = &result.error {
        return UpdateOutcome::Failed(err.clone());
    }
    match &result.latest_version {
        Some(latest) if latest.trim_start_matches('v') == env!("CARGO_PKG_VERSION") => {
            UpdateOutcome::UpToDate
        }
        Some(latest) => UpdateOutcome::NewVersion(latest.clone()),
        // 后台应保证 at least one of (latest_version, error) is Some；
        // 兜底成 Failed 是为了不静默丢失事件。
        None => UpdateOutcome::Failed("(empty result)".into()),
    }
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
pub async fn check_github_latest_release(http: &HttpClients) -> UpdateCheckResult {
    let url = "https://api.github.com/repos/AhJxs/so-novel-rs/releases/latest";

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
            .get(url)
            .header(reqwest::header::USER_AGENT, "so-novel-rs")
            .send()
            .await
    } else {
        gh_proxy_client.get(url).send().await
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

    #[test]
    fn classify_failed_takes_precedence_over_latest() {
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
    fn classify_new_version_when_differs() {
        let r = UpdateCheckResult {
            latest_version: Some("999.0.0".into()),
            error: None,
        };
        assert!(matches!(classify(&r), UpdateOutcome::NewVersion(ref v) if v == "999.0.0"));
    }

    #[test]
    fn classify_empty_result_becomes_failed() {
        let r = UpdateCheckResult {
            latest_version: None,
            error: None,
        };
        assert!(matches!(classify(&r), UpdateOutcome::Failed(_)));
    }

    #[test]
    fn drain_returns_none_without_rx() {
        let mut s = UpdateState::default();
        assert!(s.drain().is_none());
    }

    #[test]
    fn is_new_version_available_none() {
        let s = UpdateState::default();
        assert!(!s.is_new_version_available());
    }

    #[test]
    fn is_new_version_available_same() {
        let s = UpdateState {
            latest_version: Some(format!("v{}", env!("CARGO_PKG_VERSION"))),
            ..Default::default()
        };
        assert!(!s.is_new_version_available());
    }

    #[test]
    fn is_new_version_available_differs() {
        let s = UpdateState {
            latest_version: Some("v999.0.0".into()),
            ..Default::default()
        };
        assert!(s.is_new_version_available());
    }

    /// 压缩 JSON（GitHub API 实际响应格式）能被正确解析 `tag_name`。
    /// 旧实现按行匹配 pretty-print，找不到会误报 "(empty result)"。
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
