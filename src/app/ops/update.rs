//! 版本更新检查业务方法。

use std::sync::Arc;

use super::super::update_state::UpdateState;
use crate::config::AppConfig;
use crate::http::HttpClients;

/// 手动检查 GitHub release 是否有新版本。
///
/// `http` 共享 client 集合：`gh_proxy` 为空时复用 `http.safe`（HTTP CONNECT
/// 代理路径与搜索/详情共用）；非空时仍走 raw builder（forward proxy 与
/// HTTP CONNECT 互斥）。
pub fn spawn_update_check(
    config: &AppConfig,
    http: Arc<HttpClients>,
    runtime: &tokio::runtime::Runtime,
    update_state: &mut UpdateState,
) {
    if update_state.checking {
        return;
    }
    update_state.checking = true;
    update_state.latest_version = None;
    update_state.error = None;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    update_state.rx = Some(rx);

    let gh_proxy = config.gh_proxy.clone();
    let config = config.clone();
    let http = Arc::clone(&http);
    runtime.spawn(async move {
        let result =
            super::super::update_state::check_github_latest_release(&config, &http, &gh_proxy)
                .await;
        let _ = tx.send(result);
    });
}
