//! 版本更新检查状态。
//!
//! Phase 3.6：`check_latest_release` / `classify` / `is_new_version_available` 已
//! 搬到 [`crate::core::update`]；这里只剩 [`UpdateState`]（mpsc receiver +
//! `drain` 转换 + 缓存 `latest_version` / `error`）+ 旧版本兼容 re-export。

use tokio::sync::mpsc;

// 保留旧 re-export 以免 `desktop::model::*` 的外部 user 受影响。
// 等 Phase 3.x 清理 desktop 时一并删。
pub use crate::core::update::{
    UpdateCheckResult, UpdateOutcome, check_latest_release as check_github_latest_release,
    classify, is_new_version_available,
};

/// 当前进程内的"更新检查中 / 最新版本 / 错误"状态聚合。
///
/// 桌面把 [`crate::http::HttpClients`] + [`crate::core::update::check_latest_release`]
/// spawn 到后台任务，channel 推 [`UpdateCheckResult`] 回来；`drain` 把 result 落到
/// 字段并翻译成 [`UpdateOutcome`] 供 UI 推 notification。
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
    /// "检查更新"按钮切换成"下载新版"。`v` 前缀按 [`crate::core::update::classify`]
    /// 同款方式剥除。
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
