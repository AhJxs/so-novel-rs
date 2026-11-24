//! `AppModel` 版本更新方法：检查、下载安装包、取消下载、安装阶段切换。
//!
//! 手动触发 GitHub release 检查, 跟启动期自动检查走同一 `ops::spawn_update_check` 路径；
//! 下载与安装同样只改模型状态, UI 从 `update_state` 读进度。

use std::path::PathBuf;

use super::update_state::DownloadPhase;
use super::{AppModel, ops};

impl AppModel {
    /// 手动检查 GitHub release 是否有新版本。
    pub fn spawn_update_check(&mut self) {
        ops::spawn_update_check(
            &self.config,
            &self.http,
            self.runtime,
            &self.wakeup,
            &mut self.update_state,
        );
    }

    /// 下载并解出当前平台的新主程序；失败只在后台通道回报。
    pub fn spawn_update_download(&mut self) {
        if let Err(e) = ops::spawn_update_download(
            &self.http,
            self.runtime,
            &self.wakeup,
            &mut self.update_state,
        ) {
            self.push_error(e.to_string());
        }
    }

    /// 请求取消正在进行的下载。
    pub fn cancel_update_download(&mut self) {
        ops::cancel_update_download(&mut self.update_state);
    }

    /// 进入安装阶段并取出解好的新主程序；未就绪时返回 `None`。
    pub fn begin_update_install(&mut self) -> Option<PathBuf> {
        if self.update_state.download.phase != DownloadPhase::Ready {
            return None;
        }
        let staged = self.update_state.download.staged_binary.clone()?;
        self.update_state.download.phase = DownloadPhase::Installing;
        Some(staged)
    }

    /// 安装失败：记录状态并提示，应用保持运行、旧版本不受影响。
    pub fn fail_update_install(&mut self, err: &str) {
        self.update_state.download.phase = DownloadPhase::Failed(err.to_string());
        self.push_error(rust_i18n::t!("Toasts.update_install_failed", err = err).to_string());
    }
}
