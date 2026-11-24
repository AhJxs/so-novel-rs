//! 版本更新业务方法：检查 release、下载安装包并解出主程序。

use super::super::events::WakeupHandle;
use super::super::update_state::{DownloadPhase, UpdateState};
use crate::config::{AppConfig, ConfigPaths};
use crate::core::update::UpdateError;
use crate::http::HttpClients;
use crate::models::Rule;
use crate::update::{DownloadEvent, DownloadRequest, download_to_file, extract_binary};

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

/// 下载当前平台匹配到的发布包并解出主程序，进度经 `update_state.download.rx` 回报。
///
/// 只在 `Idle` / `Failed` 阶段可再次触发；下载文件落在 `~/.sonovel/updates/<version>/`。
///
/// # Errors
///
/// 还没有可下载的产物、暂存目录创建失败时返回 [`UpdateError`]（此时不启动后台任务）。
pub fn spawn_update_download(
    http: &HttpClients,
    runtime: &tokio::runtime::Runtime,
    wakeup: &WakeupHandle,
    update_state: &mut UpdateState,
) -> Result<(), UpdateError> {
    if !matches!(
        update_state.download.phase,
        DownloadPhase::Idle | DownloadPhase::Failed(_)
    ) {
        return Ok(());
    }
    let Some(asset) = update_state.asset.clone() else {
        return Err(UpdateError("当前平台没有可下载的安装包".into()));
    };
    let version = update_state
        .latest_version
        .as_deref()
        .map(|v| v.trim_start_matches('v').to_string())
        .ok_or_else(|| UpdateError("还不知道最新版本号".into()))?;

    let dir = ConfigPaths::discover().updates_dir.join(&version);
    std::fs::create_dir_all(&dir).map_err(|e| UpdateError(format!("创建更新暂存目录失败: {e}")))?;
    let archive = dir.join(&asset.name);

    let gh_proxy = http.gh_proxy_pair();
    let client = if gh_proxy.url.is_empty() {
        http.for_rule(&Rule {
            ignore_ssl: false,
            ..Rule::default()
        })
    } else {
        gh_proxy.client
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = crate::crawler::download_options::CancelToken::new();
    update_state.download = super::super::update_state::DownloadState {
        phase: DownloadPhase::Downloading,
        received: 0,
        total: asset.size,
        cancelling: false,
        staged_binary: None,
        cancel: Some(cancel.clone()),
        rx: Some(rx),
    };

    let wakeup = wakeup.clone();
    runtime.spawn(async move {
        let progress_wakeup = wakeup.clone();
        let progress_tx = tx.clone();
        let request = DownloadRequest {
            url: &asset.url,
            expected_size: asset.size,
            dest: archive.clone(),
        };
        let downloaded = download_to_file(&client, request, &cancel, &|received, total| {
            let _ = progress_tx.send(DownloadEvent::Progress { received, total });
            progress_wakeup.notify();
        })
        .await;

        let event = match downloaded {
            Err(_) if cancel.is_cancelled() => DownloadEvent::Cancelled,
            Err(e) => DownloadEvent::Failed(e.to_string()),
            Ok(path) => {
                // 解包是同步 IO，挪到阻塞线程，别占 runtime worker。
                let staged_dir = dir.clone();
                let archive_path = archive.clone();
                let extracted = tokio::task::spawn_blocking(move || {
                    let result = extract_binary(&path, &staged_dir);
                    // 不管成功失败都不留几十 MB 的安装包；失败时用户重试会重新下载。
                    let _ = std::fs::remove_file(&archive_path);
                    result
                })
                .await;
                match extracted {
                    Ok(Ok(binary)) => DownloadEvent::Finished { path: binary },
                    Ok(Err(e)) => DownloadEvent::Failed(e.to_string()),
                    Err(e) => DownloadEvent::Failed(format!("解包任务异常: {e}")),
                }
            }
        };
        let _ = tx.send(event);
        wakeup.notify();
    });

    Ok(())
}

/// 请求取消正在进行的下载；后台任务收敛后回 `Cancelled` 事件。
pub fn cancel_update_download(update_state: &mut UpdateState) {
    if update_state.download.phase != DownloadPhase::Downloading {
        return;
    }
    if let Some(cancel) = update_state.download.cancel.take() {
        cancel.cancel();
        update_state.download.cancelling = true;
    }
}
