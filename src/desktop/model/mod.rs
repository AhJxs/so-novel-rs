//! 应用状态、状态结构体、业务方法集合。
//!
//! - `*_state` — 各页面状态结构体; `ops/*` — 跨状态业务方法; `events` — 后台通道排空。
//! - 入口 `AppModel` 持有所有状态 struct 实例, **UI 中立**（不依赖任何 GUI 框架）。

mod app_model;
mod cover;
mod download;
pub(crate) mod events;
mod health;
mod library;
mod library_state;
mod list_cache;
mod persistence;
mod runtime;
mod search;
mod search_state;
mod sources;
mod sources_state;
pub use app_model::AppModel;
pub use sources_state::SourcesFilterStatus;
pub(crate) mod tasks;
pub(crate) mod trace;
mod ui_event;
mod update;
mod update_state;

pub(crate) mod ops;

pub use cover::{CoverEntry, hash_short};
pub use library_state::{LibraryEntry, LibraryState, scan_library_dir};
pub use list_cache::{ListCache, ListCacheKey, PageKind, filter_signature};
pub use runtime::build_shared_runtime;
pub use search_state::{
    CoverEvent, DetailEvent, DetailState, SearchState, SourceSearchEvent, SourceStatus, TocEvent,
    TocState,
};
pub use sources_state::SourcesState;
pub use ui_event::UIEvent;
pub use update_state::{
    UpdateCheckResult, UpdateError, UpdateOutcome, UpdateState, check_github_latest_release,
};
