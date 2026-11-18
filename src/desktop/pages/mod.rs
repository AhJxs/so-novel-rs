//! 6 个一级导航页面的渲染入口。
//!
//! 每个 page 是独立 entity，由 `RootView` 在 `current_page` 切换时挂载。
//! 这种"每页一个 entity"模式便于：
//! - 页面状态（输入框内容 / 滚动位置 / 上次选择）跨切换保持
//! - 复杂页面（Search）的弹窗 / 子视图独立管理
//!
//! `reader` 例外：它是覆盖式视图，由 `RootView` 在打开书籍时挂载、阅读器占满内容区，
//! 不是第 7 个导航页。

pub mod library;
pub mod reader;
pub mod search;
pub mod settings;
pub mod sources;
pub mod tasks;
pub mod tools;

pub use library::LibraryPage;
pub use reader::ReaderPage;
pub use search::SearchPage;
pub use settings::SettingsPage;
pub use sources::SourcesPage;
pub use tasks::TasksPage;
pub use tools::ToolsPage;
