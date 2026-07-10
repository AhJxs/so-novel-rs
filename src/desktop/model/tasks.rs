//! `AppModel` 任务管理方法
//!
//! 2 个方法: `clear_finished_tasks` / `delete_task`.
//! 操作内存 `self.tasks` Vec, 改完 fire-and-forget 走
//! `self.runtime.spawn_blocking(... crate::db::save_with_trim ...)` 落盘。

use crate::i18n::ts_fmt;

use super::{AppModel, ops};

impl AppModel {
    /// 清掉所有已结束的任务。
    pub fn clear_finished_tasks(&mut self) {
        let before = self.tasks.len();
        ops::clear_finished_tasks(&mut self.tasks);
        let removed = before - self.tasks.len();
        if removed > 0 {
            let path = self.paths.tasks_file.clone();
            let tasks = self.tasks.clone();
            self.runtime.spawn_blocking(move || {
                if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
                    tracing::warn!("保存任务到文件失败: {e:#}");
                }
            });
            self.push_success(ts_fmt(
                "Toasts.clear_tasks_ok",
                &[("n", &removed.to_string())],
            ));
        }
    }

    /// 删除单条任务记录 (仅已结束的, 运行中跳过)。
    ///
    /// 内存 `tasks` retain 移除 + 文件保存。运行中的任务不能删 (会留下孤儿后台
    /// 任务 + cancel token 丢失), 调用方 (UI) 本就只对已结束任务显示删除按钮, 这里再兜底。
    /// 返回是否真的删了 (false = 任务还在跑或不存在)。
    pub fn delete_task(&mut self, id: u64) -> bool {
        // 兜底: 运行中的不删; 不存在的也跳过。
        let Some(task) = self.tasks.iter().find(|t| t.id == id) else {
            return false;
        };
        if task.is_running() {
            return false;
        }
        self.tasks.retain(|t| t.id != id);
        let path = self.paths.tasks_file.clone();
        let tasks = self.tasks.clone();
        self.runtime.spawn_blocking(move || {
            if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
                tracing::warn!("保存任务到文件失败: {e:#}");
            }
        });
        true
    }
}
