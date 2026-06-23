//! 跨多个状态结构的业务方法（`AppModel` 调用）。
//!
//! 这些方法比纯状态内部方法（如 `SearchState::spawn_cover_download`）高一层：
//! 需要同时访问多个 state（如 rules / config / tasks），仍以 `&mut AppModel`
//! 为接收者，避免引入 callback 机制。

mod download;
mod library;
mod search;
mod settings;
mod sources;
mod update;

pub use download::*;
pub use library::*;
pub use search::*;
pub use settings::*;
pub use sources::*;
pub use update::*;
