//! `AppModel` 书源连通性检测方法 (PR #17 拆分, 2026-07-08).
//!
//! 1 个方法: spawn_health_check.
//! 通过 `crawler::health::check_sources_health` 对所有书源并发发 HEAD 请求。

use std::sync::Arc;

use crate::i18n::ts;

use super::{AppModel, ops};

impl AppModel {
    /// 派一个连通性检测任务。
    pub fn spawn_health_check(&mut self) {
        if self.rules.is_empty() {
            self.push_warning(ts("Toasts.no_sources_detected"));
            return;
        }
        ops::spawn_health_check(
            &self.rules,
            Arc::clone(&self.http),
            self.runtime,
            &mut self.sources_state,
        );
    }
}
