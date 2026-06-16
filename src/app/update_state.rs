//! 版本更新检查状态。

use tokio::sync::mpsc;

#[derive(Default)]
pub struct UpdateState {
    /// 是否正在检查。
    pub checking: bool,
    /// 最新版本号（GitHub release tag_name）。
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
                self.latest_version = result.latest_version.clone();
                self.error = result.error.clone();
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
}

/// 把后台回报的 [`UpdateCheckResult`] 分类成 [`UpdateOutcome`]。
///
/// 优先级：error > latest_version（后台 contract 保证两者不会同时为 Some）。
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
pub async fn check_github_latest_release(gh_proxy: &str) -> UpdateCheckResult {
    let url = "https://api.github.com/repos/AhJxs/so-novel-rs/releases/latest";
    let client = reqwest::Client::builder()
        .user_agent("so-novel-rs")
        .build();
    let client = match client {
        Ok(c) => c,
        Err(e) => {
            return UpdateCheckResult {
                latest_version: None,
                error: Some(format!("构建 HTTP 客户端失败: {e}")),
            }
        }
    };

    let result = if gh_proxy.is_empty() {
        client.get(url).send().await
    } else {
        let proxy = reqwest::Proxy::all(gh_proxy).ok();
        if let Some(proxy) = proxy {
            match reqwest::Client::builder()
                .user_agent("so-novel-rs")
                .proxy(proxy)
                .build()
            {
                Ok(proxied) => proxied.get(url).send().await,
                Err(e) => {
                    return UpdateCheckResult {
                        latest_version: None,
                        error: Some(format!("构建代理客户端失败: {e}")),
                    }
                }
            }
        } else {
            client.get(url).send().await
        }
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
                    // 从 JSON 中提取 "tag_name" 字段
                    let tag = body
                        .lines()
                        .find(|l| l.trim().starts_with("\"tag_name\""))
                        .and_then(|l| {
                            l.split(':')
                                .nth(1)
                                .map(|v| v.trim().trim_matches('"').trim_matches(',').to_string())
                        });
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
}
