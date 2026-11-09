//! `AppConfig` 类型定义与 enum 解析：结构 / serde 派生 / 默认值 / 校验。
//! TOML 读写流程在 `toml_io.rs`，默认路径与模板在 `defaults.rs`。

use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// 配置字符串无法解析成对应枚举时的错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseConfigValueError;

impl std::fmt::Display for ParseConfigValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("无效的配置值")
    }
}

impl std::error::Error for ParseConfigValueError {}

/// 导出文件格式。EPUB / TXT / HTML / PDF / Markdown。
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// EPUB 电子书，带章节与内嵌资源。
    #[default]
    Epub,
    /// 纯文本单文件。
    Txt,
    /// HTML 单文件。
    Html,
    /// PDF 导出（DocumentBuilder 直接构建，嵌入 CJK 字体）。
    Pdf,
    /// Markdown 单文件输出（`.md`），UTF-8 only。详见 docs/superpowers/specs/2026-07-11-markdown-export-design.md。
    Markdown,
}

impl ExportFormat {
    /// 小写名字，用作导出文件扩展名与 TOML 取值。
    pub const fn as_lower(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Txt => "txt",
            Self::Html => "html",
            Self::Pdf => "pdf",
            Self::Markdown => "markdown",
        }
    }

    /// 解析 TOML 里的格式名；无法识别时回落到 [`Self::Epub`]。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "txt" => Self::Txt,
            "html" => Self::Html,
            "pdf" => Self::Pdf,
            "markdown" => Self::Markdown,
            _ => Self::Epub,
        }
    }
}

impl FromStr for ExportFormat {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

/// zhconv 用的目标语言变体（影响下载章节正文的简繁转换目标）：
/// `ZhCn` 简体中文 / `ZhTw` 繁體中文（台灣）/ `ZhHant` 繁體中文（通用 / Hant）。
#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum LangType {
    /// 简体中文（`zh-CN`）。
    ZhCn,
    /// 台灣繁體（`zh-TW`）。
    ZhTw,
    /// 通用繁體（`zh-Hant`）。
    ZhHant,
}

impl LangType {
    /// zhconv 接受的语言标签。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ZhCn => "zh-CN",
            Self::ZhTw => "zh-TW",
            Self::ZhHant => "zh-Hant",
        }
    }

    /// 从语言标签解析；接受下划线写法与简体/繁体别名，未知标签返回 `None`。
    pub fn parse(s: &str) -> Option<Self> {
        s.parse().ok()
    }
}

impl FromStr for LangType {
    type Err = ParseConfigValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "zh_CN" | "zh-CN" | "zh-Hans" | "zh_Hans" => Ok(Self::ZhCn),
            "zh_TW" | "zh-TW" => Ok(Self::ZhTw),
            "zh_Hant" | "zh-Hant" => Ok(Self::ZhHant),
            _ => Err(ParseConfigValueError),
        }
    }
}

/// **应用语言**：简体中文 / 繁體中文 / English。存 TOML `[global].language`
/// （旧名 `[global].app-lang` 仍可加载，仅做向后兼容）。
///
/// 与 [`LangType`] 区分：`LangType` 是 zhconv 的目标语言变体；`Language` 是**应用**
/// 语言，决定 Sidebar placeholder / Dialog OK|Cancel 等所有 `gpui_kit::component`
/// 内部 `t!("...")` 文案，同时也决定下载章节正文的目标语言（见 [`Self::to_book_target_lang`]）。
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Language {
    /// 简体中文界面，下载正文转为简体。
    #[default]
    SimplifiedChinese,
    /// 繁體中文界面，下载正文转为台灣繁體。
    TraditionalChinese,
    /// English 界面，下载正文保持简体。
    English,
}

impl Language {
    /// 存进 TOML / 系统语言 API 的 BCP-47 标签。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SimplifiedChinese => "zh-CN",
            Self::TraditionalChinese => "zh-TW",
            Self::English => "en",
        }
    }

    /// 从语言标签解析界面语言；未知标签返回 `None`。
    pub fn parse(s: &str) -> Option<Self> {
        s.parse().ok()
    }

    /// 把界面语言映射到下载书籍的目标语言（zhconv 用的 [`LangType`]）：简体 / 英文 → `ZhCn`，
    /// 繁體 → `ZhTw`。
    ///
    /// `LangType::ZhHant`（通用繁体）不再从 UI 暴露 —— 原来的 Source language 下拉已被合并掉，
    /// 需要"通用繁体"输出得用其它工具后处理。
    pub const fn to_book_target_lang(self) -> LangType {
        match self {
            Self::SimplifiedChinese | Self::English => LangType::ZhCn,
            Self::TraditionalChinese => LangType::ZhTw,
        }
    }
}

impl FromStr for Language {
    type Err = ParseConfigValueError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "zh-CN" | "zh_CN" | "zh-cn" | "zh-Hans" | "zh_Hans" => Ok(Self::SimplifiedChinese),
            "zh-TW" | "zh_TW" | "zh-tw" | "zh-Hant" | "zh_Hant" => Ok(Self::TraditionalChinese),
            "en" | "en-US" | "English" => Ok(Self::English),
            _ => Err(ParseConfigValueError),
        }
    }
}

/// 主题模式：`Dynamic` 按明暗各选一个主题、跟随 [`ThemeDynMode`] 切换（默认）；
/// `Static` 固定用 `static_name` 一个主题，不跟随系统明暗。
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Default)]
pub enum ThemeKind {
    /// 明暗两套主题随系统切换。
    #[default]
    Dynamic,
    /// 固定使用单一主题。
    Static,
}

impl ThemeKind {
    /// 存进 TOML 的模式名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dynamic => "dynamic",
            Self::Static => "static",
        }
    }

    /// 解析 TOML 里的模式名；无法识别时回落到 [`Self::Dynamic`]。
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "static" => Self::Static,
            _ => Self::Dynamic,
        }
    }
}

impl FromStr for ThemeKind {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

/// 动态主题的明暗切换方式：跟随系统 / 强制浅色 / 强制深色（仅 [`ThemeKind::Dynamic`] 生效）。
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Default)]
pub enum ThemeDynMode {
    /// 跟随操作系统的明暗设置。
    #[default]
    System,
    /// 始终使用动态模式里选定的浅色主题。
    Light,
    /// 始终使用动态模式里选定的深色主题。
    Dark,
}

impl ThemeDynMode {
    /// 存进 TOML 的模式名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// 解析 TOML 里的模式名；无法识别时回落到 [`Self::System`]。
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

impl FromStr for ThemeDynMode {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

/// 主题偏好。两种模式共用一个 struct（而非 enum）—— 切换 [`ThemeKind`] 时**保留**另一模式
/// 的选项，用户在静态 / 动态间来回切不会丢失已选主题名（空串 = 用组件库 / registry 默认）。
///
/// 主题名来自 `src/desktop/themes/*.json`（每个文件含 light + dark 变体，名如
/// `"Catppuccin Latte"` / `"Catppuccin Mocha"`）；设置页按变体 `mode` 过滤，避免
/// 把深色主题选进浅色槽。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemePref {
    /// 当前生效的主题模式。
    pub kind: ThemeKind,
    /// 静态模式用的主题变体名。
    pub static_name: String,
    /// 动态模式的明暗切换方式。
    pub dyn_mode: ThemeDynMode,
    /// 动态模式 — 浅色主题变体名（空 = 默认浅色）。
    pub dyn_light: String,
    /// 动态模式 — 深色主题变体名（空 = 默认深色）。
    pub dyn_dark: String,
}

/// 主配置结构。字段按 TOML 章节分组，每个章节一个 sub-struct（序列化成嵌套表）；
/// `version` 用于将来 in-place 升级时做迁移判断。
///
/// 读取走 `toml_io::load_config`（`toml_edit` 逐字段解析，做旧键迁移 / 夹值 / i18n 兜底），
/// 不直接走 serde 反序列化；这里只声明结构与默认值。模板见 `defaults::default_template_doc`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 配置 schema 版本。`env!("CARGO_PKG_VERSION")` 在 `new` 时填。
    pub version: String,

    /// 界面与外观（`[global]`）。
    #[serde(default)]
    pub global: GlobalConfig,

    /// 下载与导出（`[download]`）。
    #[serde(default)]
    pub download: DownloadConfig,

    /// 书源搜索（`[source]`）。
    #[serde(default)]
    pub source: SourceConfig,

    /// 抓取节奏（`[crawl]`）。
    #[serde(default)]
    pub crawl: CrawlConfig,

    /// 站点 Cookie（`[cookie]`）。
    #[serde(default)]
    pub cookie: CookieConfig,

    /// 网络代理（`[proxy]`）。
    #[serde(default)]
    pub proxy: ProxyConfig,
}

/// `[global]` 章节。主题偏好 / 应用语言 / GitHub 代理 / Cloudflare bypass / 侧栏折叠 / 字号。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GlobalConfig {
    /// 主题偏好，含静态/动态两种模式的选项。
    pub theme_pref: ThemePref,
    /// 界面语言，同时决定下载正文的简繁目标。
    pub language: Language,
    /// GitHub raw 代理前缀 (留空 = 直连)。
    pub gh_proxy: String,
    /// Cloudflare bypass URL (留空 = 关闭 bypass)。
    pub cf_bypass: String,
    /// 左侧 Sidebar 是否折叠。重启后保持上次状态。
    pub sidebar_collapsed: bool,
    /// UI 字号 (px)。组件全用 `rems(...)`，`Root::render` 每帧用它设 rem 基准，
    /// 改这一个字段 = 全局缩放。范围由 `validate()` 钳到 [12, 24]，渲染层再夹一次。
    pub font_size: f32,
}

/// `[download]` 章节。下载路径 / 导出格式 / 编码 / 章节缓存策略。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadConfig {
    /// 默认下载目录 (由 `defaults::default_download_path` 决定)。
    pub download_path: String,
    /// 导出文件格式。
    pub ext_name: ExportFormat,
    /// TXT 导出编码 (UTF-8 / GBK / Big5 ...)。
    pub txt_encoding: String,
    /// 导出完成后是否保留章节缓存目录。
    pub preserve_chapter_cache: bool,
}

/// `[source]` 章节。书源搜索限制 / 过滤开关。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceConfig {
    /// 单次搜索最多返回结果数。`None` 表示未指定 (用书源默认)。
    pub search_limit: Option<i32>,
    /// 是否启用搜索结果过滤 (按书名/作者名相似度去重)。
    pub search_filter: bool,
}

/// `[crawl]` 章节。并发数 / 间隔 / 重试参数。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrawlConfig {
    /// 全局并发抓取上限。`None` = 由运行时按 CPU 数自动算。
    pub concurrency: Option<i32>,
    /// 两次抓取的最小间隔 (ms)。
    pub min_interval: u32,
    /// 两次抓取的最大间隔 (ms)。运行时在 [min, max] 间随机。
    pub max_interval: u32,
    /// 章节抓取失败后是否自动重试。
    pub enable_retry: bool,
    /// 单个书源的最大重试次数。
    pub max_retries: u32,
    /// 重试最小间隔 (ms)。
    pub retry_min_interval: u32,
    /// 重试最大间隔 (ms)。
    pub retry_max_interval: u32,
}

/// `[cookie]` 章节。站点专用 cookie。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CookieConfig {
    /// 起点中文网 cookie (订阅章节用)。
    pub qidian_cookie: String,
}

/// 代理模式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// 直连，不用代理。
    #[default]
    None,
    /// 用下面的 `proxy_host` / `proxy_port` 手动配置的 HTTP 代理。
    Manual,
    /// 读操作系统代理设置：Windows 读 `WinINET` 注册表（Clash / v2ray 的「系统代理」
    /// 开关写的就是它），其它平台读 `HTTPS_PROXY` / `HTTP_PROXY` 环境变量。
    System,
}

impl ProxyMode {
    /// 存进 TOML 的模式名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Manual => "manual",
            Self::System => "system",
        }
    }

    /// 解析 TOML 里的字符串。无法识别（含空串）→ [`Self::None`]，与 `ThemeKind::parse` 风格一致。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "manual" => Self::Manual,
            "system" => Self::System,
            _ => Self::None,
        }
    }
}

impl FromStr for ProxyMode {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

/// `[proxy]` 章节。代理模式 + 手动代理地址。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyConfig {
    /// 代理模式。决定 `crate::http::resolve_proxy_url` 的行为。
    pub proxy_mode: ProxyMode,
    /// 仅 `Manual` 模式使用的代理主机地址。
    pub proxy_host: String,
    /// 仅 `Manual` 模式使用的代理端口。
    pub proxy_port: u16,
}

impl AppConfig {
    /// 构造默认配置, 下载路径由 `defaults::default_download_path` 决定。
    pub fn new() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            global: GlobalConfig {
                theme_pref: ThemePref::default(),
                // 默认 = Dynamic + System + 空名 (gpui-kit 组件库默认浅/深主题, 跟 OS 走)
                language: Language::SimplifiedChinese,
                gh_proxy: String::new(),
                cf_bypass: String::new(),
                sidebar_collapsed: false,
                // 与 themes::FONT_SIZE_DEFAULT 一致 (16px)
                font_size: 16.0,
            },
            download: DownloadConfig {
                download_path: crate::config::defaults::default_download_path(),
                ext_name: ExportFormat::Epub,
                txt_encoding: "UTF-8".to_string(),
                preserve_chapter_cache: false,
            },
            source: SourceConfig {
                search_limit: None,
                search_filter: true,
            },
            crawl: CrawlConfig {
                concurrency: None,
                min_interval: 200,
                max_interval: 400,
                enable_retry: true,
                max_retries: 5,
                retry_min_interval: 2000,
                retry_max_interval: 4000,
            },
            cookie: CookieConfig {
                qidian_cookie: String::new(),
            },
            proxy: ProxyConfig {
                proxy_mode: ProxyMode::None,
                proxy_host: "127.0.0.1".to_string(),
                proxy_port: 7890,
            },
        }
    }

    /// 校验配置合法性（启动时调一次，失败让用户改 config.toml 重启）：
    /// `font_size` ∈ [12.0, 24.0]（与 `desktop::themes::FONT_SIZE_MIN/MAX` 一致）、
    /// `min_interval <= max_interval`、`retry_min_interval <= retry_max_interval`、`download_path` 非空。
    pub fn validate(&self) -> Result<(), ConfigError> {
        const FONT_MIN: f32 = 12.0;
        const FONT_MAX: f32 = 24.0;
        if !(FONT_MIN..=FONT_MAX).contains(&self.global.font_size) {
            return Err(ConfigError::OutOfRange {
                field: "global.font_size",
                value: self.global.font_size as f64,
                min: FONT_MIN as f64,
                max: FONT_MAX as f64,
            });
        }

        if self.crawl.min_interval > self.crawl.max_interval {
            return Err(ConfigError::InvalidRange {
                field: "crawl.min_interval/max_interval",
                min: self.crawl.min_interval as u64,
                max: self.crawl.max_interval as u64,
            });
        }

        if self.crawl.retry_min_interval > self.crawl.retry_max_interval {
            return Err(ConfigError::InvalidRange {
                field: "crawl.retry_min_interval/retry_max_interval",
                min: self.crawl.retry_min_interval as u64,
                max: self.crawl.retry_max_interval as u64,
            });
        }

        if self.download.download_path.trim().is_empty() {
            return Err(ConfigError::Empty {
                field: "download.download_path",
            });
        }

        Ok(())
    }
}

/// 配置校验错误
#[derive(Debug, thiserror::Error, Copy, Clone, PartialEq)]
pub enum ConfigError {
    /// 某个配置值落在允许区间之外。
    #[error("配置字段 `{field}` = {value} 超出合法范围 [{min}, {max}]")]
    OutOfRange {
        /// 出问题的配置键路径（如 `global.font_size`）。
        field: &'static str,
        /// 实际读到的值。
        value: f64,
        /// 允许的下界。
        min: f64,
        /// 允许的上界。
        max: f64,
    },

    /// 区间上下界本身倒置（`min > max`）。
    #[error("配置字段 `{field}` 范围非法: min={min} > max={max}")]
    InvalidRange {
        /// 出问题的配置键路径。
        field: &'static str,
        /// 声明的下界。
        min: u64,
        /// 声明的上界。
        max: u64,
    },

    /// 必填配置为空串。
    #[error("配置字段 `{field}` 不能为空")]
    Empty {
        /// 出问题的配置键路径。
        field: &'static str,
    },
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::new()
    }
}
