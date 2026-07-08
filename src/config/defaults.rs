//! 默认配置与下载路径发现。
//!
//! 跟 `toml_io.rs` 拆开是因为这些是"应用知道、但 TOML 序列化无关"的数据：
//! 下载路径依赖 OS（`directories` crate），默认模板是一段手写 TOML 字符串。
//! 它们与 schema 同寿命，但不参与 `load_config` 的字段读取流程。

use toml_edit::DocumentMut;

/// 默认下载目录：系统 Documents 文件夹下的 `Novel/` 子目录。
///
/// - Windows：`%USERPROFILE%\Documents\Novel`（或被用户改过的位置 — `directories`
///   底层走 `SHGetKnownFolderPath(FOLDERID_Documents)`，会拿到真实位置）
/// - macOS：`~/Documents/Novel`
/// - Linux：XDG `XDG_DOCUMENTS_DIR`，未设置时一般是 `~/Documents`
///
/// 取不到（极端环境无 home）时回落到相对路径 `downloads`，与历史行为保持一致 —
/// 至少程序还能跑，写到 cwd 下的 `downloads/`。
///
/// 返回字符串而非 PathBuf：`AppConfig.download_path` 字段就是 String，
/// 字符串能被设置页直接放到 TextEdit 里编辑，也能直接序列化进 TOML。
pub fn default_download_path() -> String {
    use directories::UserDirs;
    if let Some(user_dirs) = UserDirs::new() {
        if let Some(docs) = user_dirs.document_dir() {
            return docs.join("Novel").to_string_lossy().into_owned();
        }
    }
    tracing::warn!("无法定位系统 Documents 目录，下载路径回落到 ./downloads");
    "downloads".to_string()
}

/// 第一次启动 / 模板 / 文件被破坏时使用的默认 TOML 文档。
pub fn default_template_doc() -> DocumentMut {
    let template = r#"# So Novel 配置文件
[global]
# 主题偏好：
#   theme-kind = "dynamic"（默认）或 "static"
#     - dynamic：theme-light / theme-dark 各选一个主题，按 theme-dyn-mode（system/light/dark）切换
#     - static  ：固定用 theme-name 这一个主题，不随明暗变化
#   主题名与 `src/gpui_app/themes/*.json` 里变体的 name 一致（如 "Catppuccin Latte"），
#   留空 = 用 gpui-component 内置默认主题。
theme-kind = "dynamic"
theme-name = ""
theme-dyn-mode = "system"
theme-light = ""
theme-dark = ""

# language = 应用语言（Sidebar placeholder / Select / Dialog 等所有 gpui-component
# 内部 `t!("...")` 文案的语言，同时决定下载章节正文的目标语言 —— 见
# `Language::to_book_target_lang`）。三选一：zh-CN / zh-TW / en。
language = "zh-CN"
gh-proxy = ""
cf-bypass = ""
# 左侧 Sidebar 是否折叠。重启后保持上次状态。
sidebar-collapsed = false
# UI 字号（px），范围 12–24，默认 16。整个 app 按 rem 等比缩放。
font-size = 16

[download]
# download-path 默认为系统 Documents/Novel/（由 AppConfig::default() 注入）。
# 占位写空串，save_config 会按当前 cfg.download.download_path 覆盖此处的值。
download-path = ""
extname = "epub"
txt-encoding = "UTF-8"
preserve-chapter-cache = false

[source]
search-limit = 30
search-filter = true

[crawl]
min-interval = 200
max-interval = 400
enable-retry = true
max-retries = 5
retry-min-interval = 2000
retry-max-interval = 4000

[cookie]
qidian-cookie = ""

[proxy]
enabled = false
host = "127.0.0.1"
port = 7890
    "#;
    template.parse().expect("default template must parse")
}
