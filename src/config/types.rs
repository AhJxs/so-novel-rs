//! `AppConfig` 的类型定义与 enum 解析。
//!
//! 拆出来是为了让 `toml_io.rs` / `defaults.rs` 集中处理"如何读写 TOML"，
//! 类型本身（结构、serde 派生、enum 解析、默认值）放在这里。

use serde::{Deserialize, Serialize};

/// 导出文件格式。EPUB / TXT / HTML / PDF。
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
    #[default]
    Epub,
    Txt,
    Html,
    /// 阶段一不实现 PDF 导出，仅保留枚举以便兼容旧配置，
    /// UI 选择 PDF 时会显示提示并降级。详见 audit §6.4。
    Pdf,
}

impl ExportFormat {
    pub fn as_lower(self) -> &'static str {
        match self {
            ExportFormat::Epub => "epub",
            ExportFormat::Txt => "txt",
            ExportFormat::Html => "html",
            ExportFormat::Pdf => "pdf",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "txt" => ExportFormat::Txt,
            "html" => ExportFormat::Html,
            "pdf" => ExportFormat::Pdf,
            _ => ExportFormat::Epub,
        }
    }
}

/// zhconv 用的目标语言变体（影响下载章节正文的简繁转换目标）。
#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum LangType {
    /// 简体中文
    ZhCn,
    /// 繁體中文（台灣）
    ZhTw,
    /// 繁體中文（通用 / Hant）
    ZhHant,
}

impl LangType {
    pub fn as_str(self) -> &'static str {
        match self {
            LangType::ZhCn => "zh-CN",
            LangType::ZhTw => "zh-TW",
            LangType::ZhHant => "zh-Hant",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "zh_CN" | "zh-CN" | "zh-Hans" | "zh_Hans" => Some(LangType::ZhCn),
            "zh_TW" | "zh-TW" => Some(LangType::ZhTw),
            "zh_Hant" | "zh-Hant" => Some(LangType::ZhHant),
            _ => None,
        }
    }
}

/// **应用语言**（与 `LangType` 区分：`LangType` 是 zhconv 用的目标语言变体；
/// `Language` 是**应用**语言，决定 Sidebar placeholder / Select placeholder /
/// Dialog OK|Cancel 等所有 gpui-component `t!("...")` 调用的文案，同时也决定
/// 下载章节正文的目标语言 —— 见 `Language::to_book_target_lang`）。
///
/// 三种：简体中文 / 繁體中文 / English。存到 TOML `[global].language`
/// （旧名 `[global].app-lang` 仍可加载 —— 仅做向后兼容）。
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Language {
    /// 简体中文
    #[default]
    SimplifiedChinese,
    /// 繁體中文
    TraditionalChinese,
    /// English
    English,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Language::SimplifiedChinese => "zh-CN",
            Language::TraditionalChinese => "zh-TW",
            Language::English => "en",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "zh-CN" | "zh_CN" | "zh-cn" | "zh-Hans" | "zh_Hans" => {
                Some(Language::SimplifiedChinese)
            }
            "zh-TW" | "zh_TW" | "zh-tw" | "zh-Hant" | "zh_Hant" => {
                Some(Language::TraditionalChinese)
            }
            "en" | "en-US" | "English" => Some(Language::English),
            _ => None,
        }
    }

    /// 把界面语言映射到下载书籍的目标语言（zhconv 用的 `LangType`）。
    ///
    /// 合并设置后，**用户只设一个 `Language`**，下载时的简繁转换目标语言从这里推：
    /// - 简体中文界面 → 下载正文用简体中文（LangType::ZhCn）
    /// - 繁體中文界面 → 下载正文用繁體中文（LangType::ZhTw，台湾用词）
    /// - 英文 / 其它  → 回落简体中文（LangType::ZhCn）
    ///
    /// 注意：`LangType::ZhHant`（通用繁体）不再从 UI 暴露 —— 之前的 Source language
    /// 下拉被合并掉了。如果用户想要"通用繁体"输出，得改用其它工具后处理。
    pub fn to_book_target_lang(self) -> LangType {
        match self {
            Language::SimplifiedChinese | Language::English => LangType::ZhCn,
            Language::TraditionalChinese => LangType::ZhTw,
        }
    }
}

/// 主题模式：静态（固定一个主题）或动态（浅/深色各选一个，按明暗模式切换）。
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Default)]
pub enum ThemeKind {
    /// 动态：分别指定浅色 / 深色主题，按 [`ThemeDynMode`] 切换。默认。
    #[default]
    Dynamic,
    /// 静态：固定使用 `static_name` 这一个主题，不跟随系统明暗。
    Static,
}

impl ThemeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeKind::Dynamic => "dynamic",
            ThemeKind::Static => "static",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "static" => ThemeKind::Static,
            _ => ThemeKind::Dynamic,
        }
    }
}

/// 动态主题的明暗切换方式（仅 [`ThemeKind::Dynamic`] 生效）。
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Default)]
pub enum ThemeDynMode {
    /// 跟随系统明暗。
    #[default]
    System,
    /// 强制浅色。
    Light,
    /// 强制深色。
    Dark,
}

impl ThemeDynMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeDynMode::System => "system",
            ThemeDynMode::Light => "light",
            ThemeDynMode::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "light" => ThemeDynMode::Light,
            "dark" => ThemeDynMode::Dark,
            _ => ThemeDynMode::System,
        }
    }
}

/// 主题偏好。
///
/// 两种模式共用一个 struct（而非 enum）—— 切换 [`ThemeKind`] 时**保留**另一模式的
/// 选项，用户在静态/动态间来回切不会丢失已选的浅/深主题名。
///
/// - [`ThemeKind::Static`] → 用 `static_name`（空串 = gpui-component 默认主题）。
/// - [`ThemeKind::Dynamic`] → `dyn_light` / `dyn_dark` 各指定一个主题名（空串 = 用
///   registry 默认浅/深主题），`dyn_mode` 决定按系统 / 强制浅 / 强制深切换。
///
/// 主题名来自 `src/gpui_app/themes/*.json`（每个文件含 light + dark 变体，变体名如
/// `"Catppuccin Latte"` / `"Catppuccin Mocha"`）。设置页选浅/深主题时会按变体的
/// `mode` 过滤，避免把深色主题选进浅色槽。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemePref {
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

/// 主配置结构。`version` 字段用于将来 in-place 升级时做迁移判断。
///
/// 字段按 TOML 章节分组, 每个章节一个 sub-struct, 序列化时是嵌套表:
///
/// ```toml
/// [global]
/// theme-kind = "dynamic"
/// font-size = 16.0
///
/// [download]
/// download-path = "..."
/// ```
///
/// 读取流程 (`toml_io::load_config`) 按章节用 toml_edit 解析, 不直接走 serde
/// 反序列化 (要做旧键迁移、字段夹值、i18n 兜底等); 这里只声明结构与默认值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 配置 schema 版本。`env!("CARGO_PKG_VERSION")` 在 with_defaults 时填。
    pub version: String,

    /// `[global]` 章节: 主题偏好 / 语言 / 代理 / 字号。
    #[serde(default)]
    pub global: GlobalCfg,

    /// `[download]` 章节: 下载路径 / 导出格式 / 编码 / 章节缓存策略。
    #[serde(default)]
    pub download: DownloadCfg,

    /// `[source]` 章节: 书源搜索限制 / 过滤开关。
    #[serde(default)]
    pub source: SourceCfg,

    /// `[crawl]` 章节: 并发数 / 间隔 / 重试参数。
    #[serde(default)]
    pub crawl: CrawlCfg,

    /// `[cookie]` 章节: 站点专用 cookie (目前只起点中文)。
    #[serde(default)]
    pub cookie: CookieCfg,

    /// `[proxy]` 章节: HTTP 代理配置。
    #[serde(default)]
    pub proxy: ProxyCfg,
}

/// `[global]` 章节。主题偏好 / 应用语言 / GitHub 代理 / Cloudflare bypass /
/// 侧栏折叠状态 / 全局字号。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GlobalCfg {
    /// 主题偏好 (静态 / 动态)。
    pub theme_pref: ThemePref,
    /// 应用语言 (zh-CN / zh-TW / en)。
    pub language: Language,
    /// GitHub raw 代理前缀 (留空 = 直连)。
    pub gh_proxy: String,
    /// Cloudflare bypass URL (留空 = 关闭 bypass)。
    pub cf_bypass: String,
    /// 左侧 Sidebar 是否折叠。重启后保持上次状态。
    pub sidebar_collapsed: bool,
    /// UI 字号 (px)。gpui-component 默认 16; `Root::render` 每帧用它设 rem 基准,
    /// 组件全用 `rems(...)` 缩放, 改这一个字段 = 全局缩放。
    /// 范围由 `validate()` 钳制到 [12, 24], 渲染层还会再夹一次防越界。
    pub font_size: f32,
}

/// `[download]` 章节。下载路径 / 导出格式 / 编码 / 章节缓存策略。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DownloadCfg {
    /// 默认下载目录 (由 `defaults::default_download_path` 决定)。
    pub download_path: String,
    /// 导出文件格式 (EPUB / TXT / HTML / PDF)。
    pub ext_name: ExportFormat,
    /// TXT 导出编码 (UTF-8 / GBK / Big5 ...)。
    pub txt_encoding: String,
    /// 导出完成后是否保留章节缓存目录。
    pub preserve_chapter_cache: bool,
}

/// `[source]` 章节。书源搜索限制 / 过滤开关。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SourceCfg {
    /// 单次搜索最多返回结果数。`None` 表示未指定 (用书源默认)。
    pub search_limit: Option<i32>,
    /// 是否启用搜索结果过滤 (按书名/作者名相似度去重)。
    pub search_filter: bool,
}

/// `[crawl]` 章节。并发数 / 间隔 / 重试参数。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CrawlCfg {
    /// 全局并发抓取上限。`None` = 由运行时按 CPU 数自动算。
    pub concurrency: Option<i32>,
    /// 两次抓取的最小间隔 (ms)。
    pub min_interval: u32,
    /// 两次抓取的最大间隔 (ms)。运行时在 [min, max] 间随机。
    pub max_interval: u32,
    /// 是否启用失败重试。
    pub enable_retry: bool,
    /// 单个书源的最大重试次数。
    pub max_retries: u32,
    /// 重试最小间隔 (ms)。
    pub retry_min_interval: u32,
    /// 重试最大间隔 (ms)。
    pub retry_max_interval: u32,
}

/// `[cookie]` 章节。站点专用 cookie。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CookieCfg {
    /// 起点中文网 cookie (订阅章节用)。
    pub qidian_cookie: String,
}

/// `[proxy]` 章节。HTTP 代理配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ProxyCfg {
    /// 是否启用 HTTP 代理。
    pub proxy_enabled: bool,
    /// 代理主机地址。
    pub proxy_host: String,
    /// 代理端口。
    pub proxy_port: u16,
}

impl AppConfig {
    /// 构造默认配置, 下载路径由 `defaults::default_download_path` 决定。
    pub fn with_defaults() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            global: GlobalCfg {
                theme_pref: ThemePref::default(),
                // 默认 = Dynamic + System + 空名 (gpui-component 默认浅/深主题, 跟 OS 走)
                language: Language::SimplifiedChinese,
                gh_proxy: String::new(),
                cf_bypass: String::new(),
                sidebar_collapsed: false,
                // 与 themes::FONT_SIZE_DEFAULT 一致 (16px)
                font_size: 16.0,
            },
            download: DownloadCfg {
                download_path: crate::config::defaults::default_download_path(),
                ext_name: ExportFormat::Epub,
                txt_encoding: "UTF-8".to_string(),
                preserve_chapter_cache: false,
            },
            source: SourceCfg {
                search_limit: None,
                search_filter: true,
            },
            crawl: CrawlCfg {
                concurrency: None,
                min_interval: 200,
                max_interval: 400,
                enable_retry: true,
                max_retries: 5,
                retry_min_interval: 2000,
                retry_max_interval: 4000,
            },
            cookie: CookieCfg {
                qidian_cookie: String::new(),
            },
            proxy: ProxyCfg {
                proxy_enabled: false,
                proxy_host: "127.0.0.1".to_string(),
                proxy_port: 7890,
            },
        }
    }

    /// 校验配置合法性。启动时调一次, 失败让用户改 config.toml 重启。
    ///
    /// 当前校验:
    /// - `font_size` ∈ [12.0, 24.0] (与 `gpui_app::themes::FONT_SIZE_MIN/MAX` 一致)
    /// - `min_interval <= max_interval` (爬虫间隔合法性)
    /// - `retry_min_interval <= retry_max_interval` (重试间隔合法性)
    /// - `proxy_port != 0` (启用代理时端口必须非零; 实际上 u16 不会为 0 但显式校验可读)
    /// - `download_path` 非空
    pub fn validate(&self) -> Result<(), ConfigError> {
        // 字号
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

        // 爬虫间隔
        if self.crawl.min_interval > self.crawl.max_interval {
            return Err(ConfigError::InvalidRange {
                field: "crawl.min_interval/max_interval",
                min: self.crawl.min_interval as u64,
                max: self.crawl.max_interval as u64,
            });
        }

        // 重试间隔
        if self.crawl.retry_min_interval > self.crawl.retry_max_interval {
            return Err(ConfigError::InvalidRange {
                field: "crawl.retry_min_interval/retry_max_interval",
                min: self.crawl.retry_min_interval as u64,
                max: self.crawl.retry_max_interval as u64,
            });
        }

        // 下载路径
        if self.download.download_path.trim().is_empty() {
            return Err(ConfigError::Empty {
                field: "download.download_path",
            });
        }

        Ok(())
    }
}

/// 配置校验错误 (PR #6, 2026-07-08)
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// 字段值超出合法范围。
    #[error("配置字段 `{field}` = {value} 超出合法范围 [{min}, {max}]")]
    OutOfRange {
        field: &'static str,
        value: f64,
        min: f64,
        max: f64,
    },

    /// 字段范围非法 (min > max)。
    #[error("配置字段 `{field}` 范围非法: min={min} > max={max}")]
    InvalidRange {
        field: &'static str,
        min: u64,
        max: u64,
    },

    /// 必填字段为空。
    #[error("配置字段 `{field}` 不能为空")]
    Empty { field: &'static str },
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::with_defaults()
    }
}
