//! `AppModel` 搜索相关方法
//!
//! 全部 thin delegator, 实际逻辑在 `crate::app::ops::search`.

use std::sync::Arc;

use super::{AppModel, ops};

impl AppModel {
    /// 派聚合搜索任务。返回是否成功派发。
    ///
    /// # Examples
    ///
    /// ```ignore
    /// if model.spawn_search() {
    ///     // 搜索已启动, 等事件流回填
    /// }
    /// ```
    pub fn spawn_search(&mut self) -> bool {
        ops::spawn_search(
            &self.rules,
            &self.config,
            Arc::clone(&self.http),
            self.runtime,
            &self.wakeup,
            &mut self.search,
        )
    }

    /// 选中某条搜索结果; 如果之前没拉过详情就 spawn 一次。
    pub fn select_search_result(&mut self, idx: usize) {
        ops::select_search_result(
            &self.rules,
            &self.config,
            Arc::clone(&self.http),
            self.runtime,
            &self.wakeup,
            &mut self.search,
            idx,
        );
    }
}
