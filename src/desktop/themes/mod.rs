//! 主题加载 + 应用 + 列表
//!
//! ## 策略
//!
//! 21 个 JSON **直接 `include_str!` 进二进制** (编译期嵌入, 见 [`embedded`])。
//! 启动时 [`init`] 把 embed 字节同步到用户主目录下的 `~/.sonovel/themes/`, 然后调
//! `ThemeRegistry::watch_dir(themes_dir, cx, _)` 让 gpui-component 扫目录、
//! parse 为 `ThemeSet`、把每个变体注册到 global `HashMap<SharedString, Rc<ThemeConfig>>`。
//!
//! ## 同步规则 (见 [`user_dir::ensure_user_themes_dir`])
//!
//! - 目录不存在 → 创建 + 写入全部 21 个 embed 主题
//! - 目录存在 → 只补缺失的 embed 文件 (app 升级加新主题时自动加进来),
//!   **不覆盖**已有文件 —— 用户可能改过
//! - 用户也可手动放自定义 *.json 进去, gpui-component 的 file watcher 会自动 reload
//!
//! ## 业务层 API
//!
//! - [`init`] — 启动时调一次
//! - [`apply::apply_theme_pref`] — 把 [`crate::config::ThemePref`] 装到 `Theme::global_mut`;
//!   找不到的主题名 → fallback 默认主题
//! - [`apply::apply_font_size`] — 全局字号 (与装主题解耦, 装主题之后再调)
//! - [`apply::list_theme_names`] / [`apply::list_theme_names_by_mode`] — 设置页 Select 用
//!
//! ## 不做的事
//!
//! - 不在 dev / release 之间区分路径 (统一 embed)
//! - 不依赖 CWD / exe 同目录
//! - 不删用户已有的 themes (即使看着像 embed 默认)

//! ## 子模块
//!
//! - [`embedded`] — 21 个主题 JSON 编译期嵌入
//! - [`user_dir`] — 用户目录同步 (创建 / 补缺失 / 不覆盖)
//! - [`apply`] — 主题应用 (`theme_pref` / `font_size` / list)
//! - [`init`] — 启动入口, 串起 `user_dir` + apply

pub mod apply;
pub mod embedded;
pub mod init;
pub mod user_dir;

pub use apply::{apply_font_size, apply_theme_pref, list_theme_names, list_theme_names_by_mode};
pub use embedded::{FONT_SIZE_DEFAULT, FONT_SIZE_MAX, FONT_SIZE_MIN};
pub use init::init;
