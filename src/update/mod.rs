//! 应用自更新引擎：按平台挑 release 产物 → 流式下载 → 解出主程序 → 替换运行中的二进制并重启。
//!
//! 与 [`crate::core::update`] 分工：那边是 GUI 无关的 release 查询契约，这里是需要网络 +
//! 文件系统的执行层；`desktop` 只调用本模块的公开函数，不含平台细节。
//!
//! 安装只替换主程序，不动 `~/.sonovel` 下的用户数据与随包 `rules/`。

pub mod archive;
pub mod asset;
pub mod download;
pub mod install;

pub use archive::extract_binary;
pub use asset::{current_asset_name, pick_asset};
pub use download::{DownloadEvent, DownloadRequest, download_to_file};
pub use install::{cleanup_after_update, install_binary, spawn_restarted};

/// 发布包内主程序文件名。
pub(crate) const BINARY_NAME: &str = if cfg!(windows) {
    "so-novel-rs.exe"
} else {
    "so-novel-rs"
};
