//! 主题加载 + 应用 + 列表。
//!
//! 主题 JSON 在编译期 `include_str!` 进二进制 (见 [`embedded`]); 启动时 [`init`] 把 embed
//! 同步到 `~/.sonovel/themes/`, 再 `ThemeRegistry::watch_dir` 让组件库扫目录并注册到全局 registry。

//! 同步规则 (见 [`user_dir::ensure_user_themes_dir`]): 目录不存在则全量写入, 已存在只补缺失的
//! embed 文件 —— **不覆盖**用户改过的文件; 用户自放的 *.json 由 file watcher 自动 reload。
//!
//! 路径统一走 embed, 不依赖 CWD / exe 同目录, 不删除用户已有主题。

pub mod apply;
pub mod embedded;
pub mod init;
pub mod user_dir;

pub use apply::{apply_font_size, apply_theme_pref, list_theme_names, list_theme_names_by_mode};
pub use embedded::{FONT_SIZE_DEFAULT, FONT_SIZE_MAX, FONT_SIZE_MIN};
pub use init::init;
