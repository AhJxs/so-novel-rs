//! GUI 栈（gpui-kit）。
//!
//! - `RootView` 是应用内容视图（`TitleBar` + Sidebar + 内容区）; dialog / sheet /
//!   notification 覆盖层由 `gpui_kit::open_window` 包出的 `Root` 上挂的 `RootPlugin` 渲染。
//! - 页面在 `pages/`, 共享组件在 `components/`, 后台通道 → UI 重绘走 `drain_loop`。
//!
//! 本模块仅依赖 gpui-kit + 业务模块（`crate::desktop::model`）。

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
    ToggleSidebar,
};
pub use nav::{NavPage, register_key_bindings};
pub use root::RootView;
pub use run::run;
