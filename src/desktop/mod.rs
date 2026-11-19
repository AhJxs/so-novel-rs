//! GUI 栈（gpui-kit）。
//!
//! - `RootView` 是应用内容视图（`TitleBar` + Sidebar + 内容区）; dialog / sheet /
//!   notification 覆盖层由 `gpui_kit::open_window` 包出的 `Root` 上挂的 `RootPlugin` 渲染。
//! - 页面在 `pages/`, 共享组件在 `components/`, 后台通道 → UI 重绘走 `drain_loop`。
//!
//! 本模块仅依赖 gpui-kit + 业务模块（`crate::desktop::model`）。

// `AppModel` 持有运行数据存储（`surrealkv::Tree` 自带自定义 `Drop`），clippy 会顺着
// `Entity<T>` 的类型参数把整条实体链判成 significant drop，于是 UI 里每个 `Entity`
// clone / `read` guard 的临时值都被要求提前 `drop`。这些类型只是引用计数句柄，
// 提前 `drop` 不释放任何资源，建议在这里没有意义 —— 整个 UI 层关闭这条 lint。
// （实测：把 `AppModel.state` 换成无 `Drop` 的桩类型后，9 条告警全部消失。）
#![allow(clippy::significant_drop_tightening)]

mod actions;
pub mod components;
mod drain_loop;
mod logo;
pub mod model;
mod nav;
mod notifications;
mod pages;
mod root;
mod run;
pub mod themes;

pub use actions::{
    NextPage, PrevPage, Quit, ShowLibrary, ShowSearch, ShowSettings, ShowSources, ShowTasks,
    ShowTools, ToggleSidebar,
};
pub use nav::{NavPage, register_key_bindings};
pub use pages::LibraryPage;
pub use root::RootView;
pub use run::run;
