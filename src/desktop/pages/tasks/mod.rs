//! Tasks 页面：下载任务管理（进度 / 取消 / 重试 / 打开 / 位置 / 删除单条）。
//!
//! `PageHeader` + 5 个带计数后缀的过滤 Button + `List` 虚拟滚动列表；已结束的任务可删除
//! （弹 confirm Dialog → `AppModel::delete_task`）。
//! 子模块：`summary`（`TaskSummary` + 过滤/排序 helper）、`toolbar`、`delegate`、`row`。

mod delegate;
mod row;
mod summary;
mod tasks_page;
mod toolbar;

pub use tasks_page::TasksPage;

pub use self::summary::TaskSummary;
