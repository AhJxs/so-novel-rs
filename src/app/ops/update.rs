//! 版本更新检查业务方法。

use super::super::update_state::UpdateState;

/// 手动检查 GitHub release 是否有新版本。
pub fn spawn_update_check(
    config: &crate::config::AppConfig,
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
    runtime.spawn(async move {
        let result = super::super::update_state::check_github_latest_release(&gh_proxy).await;
        let _ = tx.send(result);
    });
}
