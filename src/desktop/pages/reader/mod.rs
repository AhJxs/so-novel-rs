//! 应用内阅读器：覆盖式视图，不作为第 7 个导航页。
//!
//! 注意区分：本模块是阅读器的桌面 UI 外壳，数据层仍是 [`crate::reader`]。

pub mod reader_page;
pub mod theme;

pub use reader_page::{CloseReaderFn, ReaderPage};
// `pages` 是私有模块，`LoadState` 目前只有 reader_page 内部匹配、模块外暂无消费者；
// 保留 re-export 作为阅读器状态机的对外入口（Task 18/19 会用到）。
#[allow(unused_imports)]
pub use reader_page::LoadState;
pub use theme::reader_colors;
