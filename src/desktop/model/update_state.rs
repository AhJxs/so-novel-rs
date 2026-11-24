//! 版本更新状态：release 检查缓存 + 自更新下载/安装进度。
//!
//! `check_latest_release` / `classify` 的实现已搬到 [`crate::core::update`]; 这里只剩
//! [`UpdateState`]（mpsc receiver + `drain` 转换 + 缓存）+ re-export。

use std::path::PathBuf;

use tokio::sync::mpsc;

// 保留旧 re-export, 免得 `desktop::model::*` 的外部 user 受影响。
pub use crate::core::update::{
    ReleaseAsset, UpdateCheckResult, UpdateError, UpdateOutcome,
    check_latest_release as check_github_latest_release, classify, is_new_version_available,
};
use crate::core::{DrainOutcome, try_drain_all};
use crate::crawler::download_options::CancelToken;
use crate::update::{DownloadEvent, current_asset_name, pick_asset};

/// 下载 / 安装所处的阶段，UI 按它切换按钮与进度条。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum DownloadPhase {
    /// 未开始（或有新版本还没点下载）。
    #[default]
    Idle,
    /// 正在下载安装包。
    Downloading,
    /// 安装包已解好，等待用户确认重启。
    Ready,
    /// 正在替换主程序并重启。
    Installing,
    /// 下载或安装失败。
    Failed(String),
}

/// 自更新（下载 + 解包）的进度与通道。
#[derive(Default)]
pub struct DownloadState {
    pub phase: DownloadPhase,
    /// 已接收字节数。
    pub received: u64,
    /// 总字节数（0 表示未知）。
    pub total: u64,
    /// 已请求取消，等后台线程收敛。
    pub cancelling: bool,
    /// 解好的新主程序。
    pub staged_binary: Option<PathBuf>,
    /// 取消令牌（spawn 时放入）。
    pub cancel: Option<CancelToken>,
    /// 后台推送的接收端。
    pub rx: Option<mpsc::UnboundedReceiver<DownloadEvent>>,
}

impl DownloadState {
    fn apply(&mut self, ev: DownloadEvent) -> DownloadOutcome {
        match ev {
            DownloadEvent::Progress { received, total } => {
                self.received = received;
                if total > 0 {
                    self.total = total;
                }
                DownloadOutcome::Progress
            }
            DownloadEvent::Finished { path } => {
                self.phase = DownloadPhase::Ready;
                self.cancelling = false;
                self.received = self.total;
                self.staged_binary = Some(path);
                DownloadOutcome::Ready
            }
            DownloadEvent::Cancelled => {
                self.reset();
                DownloadOutcome::Cancelled
            }
            DownloadEvent::Failed(err) => {
                self.phase = DownloadPhase::Failed(err.clone());
                self.cancelling = false;
                self.staged_binary = None;
                DownloadOutcome::Failed(err)
            }
        }
    }

    /// 回到未开始状态；不删暂存文件（清理交给启动时的 `cleanup_after_update`）。
    pub fn reset(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.cancel();
        }
        self.phase = DownloadPhase::Idle;
        self.received = 0;
        self.total = 0;
        self.cancelling = false;
        self.staged_binary = None;
        self.rx = None;
    }
}

/// `UpdateState::drain_download` 的语义化结果：`Progress` 只需重绘，其余推 toast。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadOutcome {
    /// 进度推进（不弹通知）。
    Progress,
    /// 下载并解包完成，可以确认重启安装。
    Ready,
    /// 用户取消。
    Cancelled,
    /// 失败原因。
    Failed(String),
}

/// 当前进程内的"更新检查 / 下载安装"状态聚合。
///
/// 后台任务把检查结果与下载事件推回 channel, `drain` / `drain_download` 落到字段并
/// 翻译成语义化结果供 UI 推 notification。
#[derive(Default)]
pub struct UpdateState {
    /// 是否正在检查。
    pub checking: bool,
    /// 最新版本号（GitHub release `tag_name`，保留 `v` 前缀）。
    pub latest_version: Option<String>,
    /// 检查失败的错误信息。
    pub error: Option<String>,
    /// 检查任务的接收端。
    pub rx: Option<mpsc::UnboundedReceiver<UpdateCheckResult>>,
    /// 当前平台匹配到的发布产物；`None` 表示该平台没有可自更新的包。
    pub asset: Option<ReleaseAsset>,
    /// 自更新下载进度。
    pub download: DownloadState,
}

impl UpdateState {
    /// 排空检查通道; 只在状态刚跃迁到终态时返回 `Some(UpdateOutcome)`, 否则 `None`。
    pub fn drain(&mut self) -> Option<UpdateOutcome> {
        let mut outcome = None;
        let result = try_drain_all(&mut self.rx, |res: UpdateCheckResult| {
            self.checking = false;
            match &res {
                Ok(Some(info)) => {
                    if self.latest_version.as_deref() != Some(info.tag.as_str()) {
                        // 版本变了：丢弃旧版本残留的下载 / 就绪状态。
                        self.download.reset();
                    }
                    let version = info.tag.trim_start_matches('v');
                    self.asset = pick_asset(&info.assets, version).cloned();
                    self.latest_version = Some(info.tag.clone());
                    self.error = None;
                }
                Ok(None) => {
                    self.latest_version = None;
                    self.asset = None;
                    self.error = None;
                }
                Err(e) => {
                    self.latest_version = None;
                    self.asset = None;
                    self.error = Some(e.to_string());
                }
            }
            outcome = Some(classify(&res));
        });
        // sender 已 drop（用户中断 / 异常退出）→ 收敛 `checking` 让 spinner 消失。
        if matches!(result, DrainOutcome::Disconnected) {
            self.checking = false;
        }
        outcome
    }

    /// 排空下载通道；`Progress` 事件也会返回，调用方据此重绘进度条。
    pub fn drain_download(&mut self) -> Option<DownloadOutcome> {
        let mut outcome = None;
        // 先 mem::take rx：闭包里要写 `self.download` 字段，不能再借它的 rx。
        let mut rx = self.download.rx.take();
        let result = try_drain_all(&mut rx, |ev: DownloadEvent| {
            outcome = Some(self.download.apply(ev));
        });
        if matches!(result, DrainOutcome::Disconnected) {
            // 任务异常结束（panic / runtime 关闭）：别让按钮永远卡在"下载中"。
            self.download.cancelling = false;
            if self.download.phase == DownloadPhase::Downloading {
                self.download.phase = DownloadPhase::Failed("下载中断".into());
                outcome = Some(DownloadOutcome::Failed("下载中断".into()));
            }
        } else {
            self.download.rx = rx;
        }
        outcome
    }

    /// `latest_version` 与当前版本不同时为 true —— Settings 页据此把"检查更新"
    /// 按钮换成"下载新版"。
    pub fn is_new_version_available(&self) -> bool {
        self.latest_version
            .as_deref()
            .is_some_and(is_new_version_available)
    }

    /// 当前平台是否有可自更新的发布包（没有则回退到浏览器下载）。
    #[must_use]
    pub const fn has_downloadable_asset(&self) -> bool {
        self.asset.is_some()
    }

    /// 目标产物名（用于提示 / 排错）；平台无产物时为 `None`。
    #[must_use]
    pub fn asset_name(&self) -> Option<String> {
        let version = self.latest_version.as_deref()?.trim_start_matches('v');
        current_asset_name(version)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use crate::core::update::ReleaseInfo;

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

    fn progress(received: u64, total: u64) -> DownloadEvent {
        DownloadEvent::Progress { received, total }
    }

    #[test]
    fn progress_tracks_received_and_total() {
        let mut d = DownloadState {
            phase: DownloadPhase::Downloading,
            ..Default::default()
        };
        assert_eq!(d.apply(progress(5, 10)), DownloadOutcome::Progress);
        assert_eq!(d.received, 5);
        assert_eq!(d.total, 10);
        // total 未知（0）时不覆盖已知总量。
        let mut d = DownloadState::default();
        d.apply(progress(5, 0));
        assert_eq!(d.received, 5);
        assert_eq!(d.total, 0);
    }

    #[test]
    fn finished_moves_to_ready() {
        let mut d = DownloadState {
            phase: DownloadPhase::Downloading,
            ..Default::default()
        };
        let outcome = d.apply(DownloadEvent::Finished {
            path: PathBuf::from("/tmp/new-exe"),
        });
        assert_eq!(outcome, DownloadOutcome::Ready);
        assert_eq!(d.phase, DownloadPhase::Ready);
        assert_eq!(d.staged_binary, Some(PathBuf::from("/tmp/new-exe")));
    }

    #[test]
    fn failed_keeps_message() {
        let mut d = DownloadState::default();
        let outcome = d.apply(DownloadEvent::Failed("boom".into()));
        assert_eq!(outcome, DownloadOutcome::Failed("boom".into()));
        assert_eq!(d.phase, DownloadPhase::Failed("boom".into()));
    }

    #[test]
    fn cancelled_resets_to_idle() {
        let mut d = DownloadState {
            phase: DownloadPhase::Downloading,
            received: 10,
            total: 100,
            cancelling: true,
            ..Default::default()
        };
        assert_eq!(
            d.apply(DownloadEvent::Cancelled),
            DownloadOutcome::Cancelled
        );
        assert_eq!(d.phase, DownloadPhase::Idle);
        assert_eq!(d.received, 0);
        assert!(d.cancel.is_none());
    }

    #[test]
    fn disconnected_during_download_fails_it() {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut s = UpdateState::default();
        s.download.phase = DownloadPhase::Downloading;
        s.download.rx = Some(rx);
        drop(tx);

        let outcome = s.drain_download();

        assert_eq!(outcome, Some(DownloadOutcome::Failed("下载中断".into())));
        assert_ne!(s.download.phase, DownloadPhase::Downloading);
    }

    #[test]
    fn check_result_selects_current_asset() {
        let expected = current_asset_name("9.9.9").expect("当前平台应有产物");
        let info = ReleaseInfo {
            tag: "v9.9.9".into(),
            assets: vec![ReleaseAsset {
                name: expected.clone(),
                url: "https://example.com/pkg".into(),
                size: 1,
            }],
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let mut s = UpdateState {
            rx: Some(rx),
            ..Default::default()
        };
        tx.send(Ok(Some(info))).unwrap();

        assert_eq!(s.drain(), Some(UpdateOutcome::NewVersion("v9.9.9".into())));
        assert_eq!(s.asset.as_ref().map(|a| a.name.clone()), Some(expected));
        assert_eq!(s.asset_name(), current_asset_name("9.9.9"));
    }
}
