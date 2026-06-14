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

impl UpdateState {
    /// 排空通道；返回是否产生过事件。
    pub fn drain(&mut self) -> bool {
        let Some(rx) = self.rx.as_mut() else {
            return false;
        };
        match rx.try_recv() {
            Ok(result) => {
                self.checking = false;
                self.latest_version = result.latest_version;
                self.error = result.error;
                self.rx = None;
                true
            }
            Err(mpsc::error::TryRecvError::Empty) => false,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                self.checking = false;
                self.rx = None;
                false
            }
        }
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
