//! Library 页面：本地书库（下载目录里的电子书文件）。
//!
//! - 进入页面时 `library.scanned_dir` 与 `config.download.download_path` 不一致 → 自动扫一次。
//! - 工具栏：文件名过滤输入 + 文件类型按钮组（不持 State —— 切语言即时更新）。
//! - 列表：`List`（虚拟滚动）+ `LibraryDelegate`，每页 30 条；分页页脚走 `components::Pagination`，
//!   其可见性由组件自判（不足一页渲染 `Empty`）。
//! - **没有文件 watcher**：只在「首次进入 / 下载目录变化」自动扫，其余靠「刷新」按钮。
//! - 删除走 `WindowExt::open_dialog` 二次确认 → `model.delete_library_entry` → bump
//!   `entries_version` 让 `ListCache` 立即失效。

mod delegate;
mod library_page;
mod row;
mod toolbar;

pub use library_page::LibraryPage;
