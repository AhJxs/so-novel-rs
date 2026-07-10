//! `AppModel` 版本更新方法
//!
//! 1 个方法: `spawn_update_check`.
//! 手动触发 GitHub release 检查, 跟启动期自动检查走同一 `ops::spawn_update_check` 路径。

use std::sync::Arc;

use super::{AppModel, ops};

impl AppModel {
    /// 手动检查 GitHub release 是否有新版本。
    pub fn spawn_update_check(&mut self) {
        ops::spawn_update_check(
            &self.config,
            Arc::clone(&self.http),
            self.runtime,
            &self.wakeup,
            &mut self.update_state,
        );
    }
}
