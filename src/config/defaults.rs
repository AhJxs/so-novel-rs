//! 默认配置与下载路径发现。与 `toml_io.rs` 拆开：这里是"应用知道、但 TOML 序列化无关"
//! 的数据（下载路径依赖 OS，默认模板是一段手写 TOML 字符串），不参与 `load_config` 流程。

use toml_edit::DocumentMut;

/// 默认下载目录：系统 Documents 下的 `Novel/`（Windows `%USERPROFILE%\Documents`、
/// macOS `~/Documents`、Linux XDG `XDG_DOCUMENTS_DIR`；`directories` 拿真实位置）。
///
/// 取不到（极端环境无 home，如 Docker 里没有 `HOME`/`USERPROFILE`）时回落到 cwd 相对
/// 路径 `./downloads`；带 `./` 是为了在设置页 / 日志 / 存储值三处展示一致。
/// 返回 `String` 而非 `PathBuf`：`AppConfig.download_path` 就是 String，可直接编辑与序列化。
pub fn default_download_path() -> String {
    use directories::UserDirs;
    if let Some(user_dirs) = UserDirs::new()
        && let Some(docs) = user_dirs.document_dir()
    {
        return docs.join("Novel").to_string_lossy().into_owned();
    }
    tracing::warn!("无法定位系统 Documents 目录，下载路径回落到 ./downloads");
    "./downloads".to_string()
}

/// 第一次启动 / 模板 / 文件被破坏时使用的默认 TOML 文档。模板是源码内 `&'static str`
/// 字面量，解析失败只可能是源码写错 → `panic!` 把程序员错误尽早暴露在启动期，避免
/// 后续读到半残 `DocumentMut` 引发更难诊断的二次失败。
#[allow(
    clippy::panic,
    reason = "static template literal must parse; failure = programmer error"
)]
pub fn default_template_doc() -> DocumentMut {
    let template = r#"# So Novel 配置文件
[global]
# 主题偏好：
#   theme-kind = "dynamic"（默认）或 "static"
#     - dynamic：theme-light / theme-dark 各选一个主题，按 theme-dyn-mode（system/light/dark）切换
#     - static  ：固定用 theme-name 这一个主题，不随明暗变化
#   主题名与 `src/desktop/themes/*.json` 里变体的 name 一致（如 "Catppuccin Latte"），
#   留空 = 用 gpui-kit 组件库内置默认主题。
theme-kind = "dynamic"
theme-name = ""
theme-dyn-mode = "system"
theme-light = ""
theme-dark = ""

# language = 应用语言（Sidebar placeholder / Select / Dialog 等所有 gpui-kit 组件库
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
# 代理模式：none = 直连；manual = 用下面的 host / port；system = 读系统代理
# （Windows 读注册表 WinINET，即 Clash / v2ray 的「系统代理」开关；其它平台读 HTTPS_PROXY 环境变量）
mode = "none"
host = "127.0.0.1"
port = 7890
    "#;
    match template.parse() {
        Ok(doc) => doc,
        Err(e) => panic!("default template must parse: {e}"),
    }
}
