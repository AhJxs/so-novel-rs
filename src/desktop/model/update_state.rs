//! 版本更新检查状态。
//!
//! `check_latest_release` / `classify` 的实现已搬到 [`crate::core::update`]; 这里只剩
//! [`UpdateState`]（mpsc receiver + `drain` 转换 + 缓存）+ re-export。

use tokio::sync::mpsc;

use crate::core::{DrainOutcome, try_drain_all};

// 保留旧 re-export, 免得 `desktop::model::*` 的外部 user 受影响。
pub use crate::core::update::{
    UpdateCheckResult, UpdateOutcome, check_latest_release as check_github_latest_release,
    classify, is_new_version_available,
};

/// 当前进程内的"更新检查中 / 最新版本 / 错误"状态聚合。
///
/// 后台任务把 [`UpdateCheckResult`] 推回 channel, `drain` 落到字段并翻译成
/// [`UpdateOutcome`] 供 UI 推 notification。
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

impl UpdateState {
    /// 排空通道; 只在状态刚跃迁到终态时返回 [`Some(UpdateOutcome)`], 否则 `None`。
    pub fn drain(&mut self) -> Option<UpdateOutcome> {
        let mut outcome = None;
        let result = try_drain_all(&mut self.rx, |res: UpdateCheckResult| {
            self.checking = false;
            self.latest_version.clone_from(&res.latest_version);
            self.error.clone_from(&res.error);
            outcome = Some(classify(&res));
        });
        // sender 已 drop（用户中断 / 异常退出）→ 收敛 `checking` 让 spinner 消失。
        if matches!(result, DrainOutcome::Disconnected) {
            self.checking = false;
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
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

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
}
