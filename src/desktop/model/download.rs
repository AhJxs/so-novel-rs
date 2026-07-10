//! `AppModel` 下载相关方法
//!
//! 全部 thin delegator, 实际逻辑在 `crate::desktop::model::ops::download`.
//! 拆这里只是按职责分类, 不重复实现。

use crate::models::{Book, Chapter, SearchResult};

use super::{AppModel, ops};

impl AppModel {
    /// 派一个新的下载任务。返回新任务 id。
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let id = model.spawn_download(search_result);
    /// model.list_cache.clear();  // 列表缓存需要重渲染
    /// ```
    pub fn spawn_download(&mut self, target: SearchResult) -> u64 {
        let ctx = self.ops_ctx();
        let (id, task) = ops::spawn_download(&ctx, self.next_task_id, target);
        self.next_task_id += 1;
        self.tasks.push(task);
        // Fire-and-forget 落盘: UI 线程不能同步等 fsync, 派到 blocking pool。
        // 失败仅 warn (best-effort), 下次保存会覆盖。
        let path = self.paths.tasks_file.clone();
        let tasks = self.tasks.clone();
        self.runtime.spawn_blocking(move || {
            if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
                tracing::warn!("保存任务到文件失败: {e:#}");
            }
        });
        id
    }

    /// 派一个 TOC 预取任务 (获取元数据 + 章节列表, 不开始下载)。
    pub fn spawn_resolve_toc(&mut self, target: &SearchResult) {
        let ctx = self.ops_ctx();
        let rx = ops::spawn_resolve_toc(&ctx, target);
        self.search.toc_rx = Some(rx);
    }

    /// 派一个指定章节范围的下载任务。跳过 resolve 阶段, 直接进入下载。
    /// 返回新任务 id。
    #[allow(clippy::needless_pass_by_value)]
    pub fn spawn_download_range(
        &mut self,
        target: SearchResult,
        book: Book,
        chapters: Vec<Chapter>,
    ) -> u64 {
        let ctx = self.ops_ctx();
        let (id, task) = ops::spawn_download_range(&ctx, self.next_task_id, target, book, chapters);
        self.next_task_id += 1;
        self.tasks.push(task);
        // 同 spawn_download: fire-and-forget 落盘。
        let path = self.paths.tasks_file.clone();
        let tasks = self.tasks.clone();
        self.runtime.spawn_blocking(move || {
            if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
                tracing::warn!("保存任务到文件失败: {e:#}");
            }
        });
        id
    }
}
