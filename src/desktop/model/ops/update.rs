//! 版本更新检查业务方法。

use super::super::events::WakeupHandle;
use super::super::update_state::UpdateState;
use crate::config::AppConfig;
use crate::http::HttpClients;
use crate::models::Rule;

/// 手动检查 GitHub release 是否有新版本。
///
/// 代理逻辑集中在 `HttpClients.gh_proxy_pair()`：
/// `gh_proxy` 非空时走预构建的前向代理 client，否则复用共享 safe client。
pub fn spawn_update_check(
    _config: &AppConfig,
    http: &HttpClients,
    runtime: &tokio::runtime::Runtime,
    wakeup: &WakeupHandle,
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

    // 任务不借用 HttpClients：spawn 前把两个 client 取成 owned 值。
    let gh_proxy = http.gh_proxy_pair();
    let safe_client = http.for_rule(&Rule {
        ignore_ssl: false,
        ..Rule::default()
    });
    let wakeup = wakeup.clone();
    runtime.spawn(async move {
        let result =
            super::super::update_state::check_github_latest_release(gh_proxy, safe_client).await;
        let _ = tx.send(result);
        wakeup.notify();
    });
}
