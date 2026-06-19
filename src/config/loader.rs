//! `config.toml` 读写。
//!
//! 设计目标：
//! - 配置文件就在项目根目录（`./config.toml`）— 与 Java 时代的 bundle/config.ini 不同；
//! - 用 `toml_edit` 保留注释 + 字段顺序，UI 设置页改完写回不会洗掉用户注释；
//! - 字段语义沿用旧版（`extname` / `min-interval` 等保留 kebab-case）；
//! - 旧 INI 默认 `1` / `0` 表示布尔，TOML 用真正的 bool；
//! - 旧版 `-1` 占位"未指定"的整数，TOML 一律用键缺失（`Option`）。
//!
//! `source-id` / `search-limit` / `concurrency` 在 TOML 里如果不写就视为未指定。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, value};

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
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
            LangType::ZhCn => "zh_CN",
            LangType::ZhTw => "zh_TW",
            LangType::ZhHant => "zh_Hant",
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
#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Language {
    /// 简体中文
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: String,

    // [global]
    pub theme_pref: ThemePref,
    pub language: Language,
    pub gh_proxy: String,
    pub cf_bypass: String,
    /// 左侧 Sidebar 是否折叠。重启后保持上次状态。
    pub sidebar_collapsed: bool,
    /// UI 字号（px）。gpui-component 默认 16；`Root::render` 每帧用它设 rem 基准，
    /// 组件全用 `rems(...)` 缩放，改这一个字段 = 全局缩放。
    /// 范围由 `themes::FONT_SIZE_MIN/MAX` 钳制（12–24），超出部分渲染时被夹住。
    pub font_size: f32,

    // [download]
    pub download_path: String,
    pub ext_name: ExportFormat,
    pub txt_encoding: String,
    pub preserve_chapter_cache: bool,
    pub enable_progressbar: bool,

    // [source]
    /// `None` 表示未指定（旧 INI `-1`）。
    pub source_id: Option<i32>,
    pub search_limit: Option<i32>,
    pub search_filter: bool,

    // [crawl]
    pub concurrency: Option<i32>,
    pub min_interval: u32,
    pub max_interval: u32,
    pub enable_retry: bool,
    pub max_retries: u32,
    pub retry_min_interval: u32,
    pub retry_max_interval: u32,

    // [cookie]
    pub qidian_cookie: String,

    // [proxy]
    pub proxy_enabled: bool,
    pub proxy_host: String,
    pub proxy_port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),

            theme_pref: ThemePref::default(),
            // 默认 = Dynamic + System + 空名（gpui-component 默认浅/深主题，跟 OS 走）。
            language: Language::SimplifiedChinese,
            gh_proxy: String::new(),
            cf_bypass: String::new(),
            sidebar_collapsed: false,
            // 与 themes::FONT_SIZE_DEFAULT 一致（16px）。
            font_size: 16.0,

            download_path: default_download_path(),
            ext_name: ExportFormat::Epub,
            txt_encoding: "UTF-8".to_string(),
            preserve_chapter_cache: false,
            enable_progressbar: true,

            source_id: None,
            search_limit: None,
            search_filter: true,

            concurrency: None,
            min_interval: 200,
            max_interval: 400,
            enable_retry: true,
            // README 写 3，Java 代码默认值是 5；以代码为准。
            max_retries: 5,
            retry_min_interval: 2000,
            retry_max_interval: 4000,

            qidian_cookie: String::new(),

            proxy_enabled: false,
            proxy_host: "127.0.0.1".to_string(),
            proxy_port: 7890,
        }
    }
}

// ---------- TOML 工具 ----------

fn t_str(doc: &DocumentMut, table: &str, key: &str) -> Option<String> {
    doc.get(table)
        .and_then(|t| t.as_table())
        .and_then(|t| t.get(key))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

fn t_bool(doc: &DocumentMut, table: &str, key: &str) -> Option<bool> {
    doc.get(table)
        .and_then(|t| t.as_table())
        .and_then(|t| t.get(key))
        .and_then(|v| v.as_bool())
}

fn t_int(doc: &DocumentMut, table: &str, key: &str) -> Option<i64> {
    doc.get(table)
        .and_then(|t| t.as_table())
        .and_then(|t| t.get(key))
        .and_then(|v| v.as_integer())
}

/// 读浮点（兼容 TOML 里写成整数 `16` 或浮点 `16.0` 两种形式）。
fn t_float(doc: &DocumentMut, table: &str, key: &str) -> Option<f32> {
    let v = doc
        .get(table)
        .and_then(|t| t.as_table())
        .and_then(|t| t.get(key))?;
    v.as_float()
        .map(|f| f as f32)
        .or_else(|| v.as_integer().map(|i| i as f32))
}

fn sat_i32(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn sat_u32(v: i64) -> u32 {
    v.max(0) as u32
}

fn sat_u16(v: i64) -> u16 {
    v.clamp(0, u16::MAX as i64) as u16
}

/// 加载配置。文件不存在时返回 `Default::default()`。
pub fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("读取 config.toml 失败: {}", path.display()))?;
    let doc: DocumentMut = content
        .parse()
        .map_err(|e| anyhow::anyhow!("解析 config.toml 失败: {e}"))?;

    let mut cfg = AppConfig::default();

    // [global] —— 主题偏好。
    //
    // 新键：theme-kind / theme-name / theme-dyn-mode / theme-light / theme-dark。
    // 旧键 `[global].theme = "X"`（单一主题名）向后兼容：非空 → 静态模式 + 该名字；
    // 空或缺省 → 默认（Dynamic + System）。
    if let Some(v) = t_str(&doc, "global", "theme-kind") {
        cfg.theme_pref.kind = ThemeKind::parse(&v);
    }
    if let Some(v) = t_str(&doc, "global", "theme-name") {
        cfg.theme_pref.static_name = v;
    }
    if let Some(v) = t_str(&doc, "global", "theme-dyn-mode") {
        cfg.theme_pref.dyn_mode = ThemeDynMode::parse(&v);
    }
    if let Some(v) = t_str(&doc, "global", "theme-light") {
        cfg.theme_pref.dyn_light = v;
    }
    if let Some(v) = t_str(&doc, "global", "theme-dark") {
        cfg.theme_pref.dyn_dark = v;
    }
    // 旧单主题键迁移：仅当新键 theme-kind 没写、且旧 theme 非空时，按静态模式接管。
    if t_str(&doc, "global", "theme-kind").is_none() {
        if let Some(v) = t_str(&doc, "global", "theme") {
            cfg.theme_pref = ThemePref {
                kind: ThemeKind::Static,
                static_name: v,
                ..ThemePref::default()
            };
        }
    }
    // 新 TOML 键 `[global].language`；旧键 `[global].app-lang` 兼容 —— 老用户配置无需手动迁移。
    if let Some(v) = t_str(&doc, "global", "language") {
        if let Some(parsed) = Language::parse(&v) {
            cfg.language = parsed;
        }
    }
    if let Some(v) = t_str(&doc, "global", "gh-proxy") {
        cfg.gh_proxy = v;
    }
    if let Some(v) = t_str(&doc, "global", "cf-bypass") {
        cfg.cf_bypass = v;
    }
    if let Some(v) = t_bool(&doc, "global", "sidebar-collapsed") {
        cfg.sidebar_collapsed = v;
    }
    if let Some(v) = t_float(&doc, "global", "font-size") {
        cfg.font_size = v;
    }

    // [download]
    if let Some(v) = t_str(&doc, "download", "download-path") {
        cfg.download_path = v;
    }
    if let Some(v) = t_str(&doc, "download", "extname") {
        cfg.ext_name = ExportFormat::parse(&v);
    }
    if let Some(v) = t_str(&doc, "download", "txt-encoding") {
        cfg.txt_encoding = v;
    }
    if let Some(v) = t_bool(&doc, "download", "preserve-chapter-cache") {
        cfg.preserve_chapter_cache = v;
    }
    if let Some(v) = t_bool(&doc, "download", "enable-progressbar") {
        cfg.enable_progressbar = v;
    }

    // [source]
    cfg.source_id = t_int(&doc, "source", "source-id").map(sat_i32);
    cfg.search_limit = t_int(&doc, "source", "search-limit").map(sat_i32);
    if let Some(v) = t_bool(&doc, "source", "search-filter") {
        cfg.search_filter = v;
    }

    // [crawl]
    cfg.concurrency = t_int(&doc, "crawl", "concurrency").map(sat_i32);
    if let Some(v) = t_int(&doc, "crawl", "min-interval") {
        cfg.min_interval = sat_u32(v);
    }
    if let Some(v) = t_int(&doc, "crawl", "max-interval") {
        cfg.max_interval = sat_u32(v);
    }
    if let Some(v) = t_bool(&doc, "crawl", "enable-retry") {
        cfg.enable_retry = v;
    }
    if let Some(v) = t_int(&doc, "crawl", "max-retries") {
        cfg.max_retries = sat_u32(v);
    }
    if let Some(v) = t_int(&doc, "crawl", "retry-min-interval") {
        cfg.retry_min_interval = sat_u32(v);
    }
    if let Some(v) = t_int(&doc, "crawl", "retry-max-interval") {
        cfg.retry_max_interval = sat_u32(v);
    }

    // [cookie]
    if let Some(v) = t_str(&doc, "cookie", "qidian-cookie") {
        cfg.qidian_cookie = v;
    }

    // [proxy]
    if let Some(v) = t_bool(&doc, "proxy", "enabled") {
        cfg.proxy_enabled = v;
    }
    if let Some(v) = t_str(&doc, "proxy", "host") {
        cfg.proxy_host = v;
    }
    if let Some(v) = t_int(&doc, "proxy", "port") {
        cfg.proxy_port = sat_u16(v);
    }

    Ok(cfg)
}

/// 把 AppConfig 写回 TOML。如果原文件存在，就在它上面 in-place 改字段（保留注释）；
/// 不存在则用统一模板生成。
pub fn save_config(path: &Path, cfg: &AppConfig) -> Result<()> {
    let mut doc: DocumentMut = if path.exists() {
        std::fs::read_to_string(path)
            .with_context(|| format!("读取 {}", path.display()))?
            .parse()
            .unwrap_or_else(|_| default_template_doc())
    } else {
        default_template_doc()
    };

    // 写一行 (table, key, value)。`value()` 自动处理 toml 类型。
    fn set_str(doc: &mut DocumentMut, table: &str, key: &str, v: &str) {
        let t = doc.entry(table).or_insert(Item::Table(Default::default()));
        if let Some(t) = t.as_table_mut() {
            t[key] = value(v);
        }
    }
    fn set_bool(doc: &mut DocumentMut, table: &str, key: &str, v: bool) {
        let t = doc.entry(table).or_insert(Item::Table(Default::default()));
        if let Some(t) = t.as_table_mut() {
            t[key] = value(v);
        }
    }
    fn set_int(doc: &mut DocumentMut, table: &str, key: &str, v: i64) {
        let t = doc.entry(table).or_insert(Item::Table(Default::default()));
        if let Some(t) = t.as_table_mut() {
            t[key] = value(v);
        }
    }
    fn set_float(doc: &mut DocumentMut, table: &str, key: &str, v: f64) {
        let t = doc.entry(table).or_insert(Item::Table(Default::default()));
        if let Some(t) = t.as_table_mut() {
            t[key] = value(v);
        }
    }
    fn unset(doc: &mut DocumentMut, table: &str, key: &str) {
        if let Some(t) = doc.get_mut(table).and_then(|t| t.as_table_mut()) {
            t.remove(key);
        }
    }
    // set
    set_str(
        &mut doc,
        "global",
        "theme-kind",
        cfg.theme_pref.kind.as_str(),
    );
    set_str(
        &mut doc,
        "global",
        "theme-name",
        &cfg.theme_pref.static_name,
    );
    set_str(
        &mut doc,
        "global",
        "theme-dyn-mode",
        cfg.theme_pref.dyn_mode.as_str(),
    );
    set_str(&mut doc, "global", "theme-light", &cfg.theme_pref.dyn_light);
    set_str(&mut doc, "global", "theme-dark", &cfg.theme_pref.dyn_dark);
    set_str(&mut doc, "global", "language", cfg.language.as_str());
    set_str(&mut doc, "global", "gh-proxy", &cfg.gh_proxy);
    set_str(&mut doc, "global", "cf-bypass", &cfg.cf_bypass);
    set_bool(
        &mut doc,
        "global",
        "sidebar-collapsed",
        cfg.sidebar_collapsed,
    );
    set_float(&mut doc, "global", "font-size", cfg.font_size as f64);

    // [download]
    set_str(&mut doc, "download", "download-path", &cfg.download_path);
    set_str(&mut doc, "download", "extname", cfg.ext_name.as_lower());
    set_str(&mut doc, "download", "txt-encoding", &cfg.txt_encoding);
    set_bool(
        &mut doc,
        "download",
        "preserve-chapter-cache",
        cfg.preserve_chapter_cache,
    );
    set_bool(
        &mut doc,
        "download",
        "enable-progressbar",
        cfg.enable_progressbar,
    );

    // [source]
    match cfg.source_id {
        Some(v) => set_int(&mut doc, "source", "source-id", v as i64),
        None => unset(&mut doc, "source", "source-id"),
    }
    match cfg.search_limit {
        Some(v) => set_int(&mut doc, "source", "search-limit", v as i64),
        None => unset(&mut doc, "source", "search-limit"),
    }
    set_bool(&mut doc, "source", "search-filter", cfg.search_filter);

    // [crawl]
    match cfg.concurrency {
        Some(v) => set_int(&mut doc, "crawl", "concurrency", v as i64),
        None => unset(&mut doc, "crawl", "concurrency"),
    }
    set_int(&mut doc, "crawl", "min-interval", cfg.min_interval as i64);
    set_int(&mut doc, "crawl", "max-interval", cfg.max_interval as i64);
    set_bool(&mut doc, "crawl", "enable-retry", cfg.enable_retry);
    set_int(&mut doc, "crawl", "max-retries", cfg.max_retries as i64);
    set_int(
        &mut doc,
        "crawl",
        "retry-min-interval",
        cfg.retry_min_interval as i64,
    );
    set_int(
        &mut doc,
        "crawl",
        "retry-max-interval",
        cfg.retry_max_interval as i64,
    );

    // [cookie]
    set_str(&mut doc, "cookie", "qidian_cookie", &cfg.qidian_cookie);

    // [proxy]
    set_bool(&mut doc, "proxy", "enabled", cfg.proxy_enabled);
    set_str(&mut doc, "proxy", "host", &cfg.proxy_host);
    set_int(&mut doc, "proxy", "port", cfg.proxy_port as i64);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, doc.to_string()).with_context(|| format!("写入 {}", path.display()))?;
    Ok(())
}

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
fn default_download_path() -> String {
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
/// 字段顺序与默认值与 Java 端 `bundle/config.ini` 对齐，方便老用户对照。
fn default_template_doc() -> DocumentMut {
    let template = r#"# So Novel 配置文件
                        # 字段语义与旧版 config.ini 一致；规则与下载任务记录已迁到根目录的 sonovel.db。

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
                        # 占位写空串，save_config 会按当前 cfg.download_path 覆盖此处的值。
                        download-path = ""
                        extname = "epub"
                        txt-encoding = "UTF-8"
                        preserve-chapter-cache = false
                        enable-progressbar = true

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

/// 程序启动时关心的几条路径。
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    /// `config.toml` 路径。
    pub config_file: PathBuf,
    /// SQLite 数据库文件 `sonovel.db`：装下载任务 + 书源 + 用户覆写。
    pub db_file: PathBuf,
    /// 主题目录 `~/.sonovel/themes/`：首次启动写入 21 个 embed 主题，
    /// 之后 watcher 监听这个目录，用户可手动放自定义 *.json 进去热加载。
    pub themes_dir: PathBuf,
    /// 日志目录 `~/.sonovel/logs/`：tracing 文件 appender 按天滚动输出 `so-novel-rs.YYYY-MM-DD.log`。
    /// 启动时清理超过 30 天的旧文件（保留策略跟 tracing_appender 解耦，自己做）。
    pub log_dir: PathBuf,
}

impl ConfigPaths {
    /// 路径约定：
    /// - `config.toml` + `sonovel.db` + `themes/` + `logs/` 统一存放在用户主目录下的 `~/.sonovel/`；
    /// - 首次启动时各目录/文件不存在，`save_config` / `Db::open` / `themes::init` / 日志 appender 会自动创建；
    /// - 如果无法获取主目录（极端情况），回落到当前工作目录。
    pub fn discover() -> Self {
        let base = home_dir().join(".sonovel");
        Self {
            config_file: base.join("config.toml"),
            db_file: base.join("sonovel.db"),
            themes_dir: base.join("themes"),
            log_dir: base.join("logs"),
        }
    }
}

/// 获取用户主目录，回落到当前工作目录。
fn home_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_default_when_missing() {
        let cfg = load_config(&PathBuf::from("/definitely/does/not/exist.toml")).unwrap();
        assert_eq!(cfg.min_interval, 200);
        assert_eq!(cfg.max_interval, 400);
        assert!(cfg.enable_retry);
        assert!(cfg.search_filter);
        assert_eq!(cfg.ext_name, ExportFormat::Epub);
    }

    #[test]
    fn font_size_accepts_int_and_float_literal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // 整数形式（模板默认写法）
        std::fs::write(&path, "[global]\nfont-size = 18\n").unwrap();
        assert_eq!(load_config(&path).unwrap().font_size, 18.0);
        // 浮点形式
        std::fs::write(&path, "[global]\nfont-size = 20.5\n").unwrap();
        assert_eq!(load_config(&path).unwrap().font_size, 20.5);
    }

    #[test]
    fn round_trip_through_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let cfg = AppConfig {
            download_path: "/tmp/sn-novels".to_string(),
            ext_name: ExportFormat::Txt,
            txt_encoding: "GBK".to_string(),
            preserve_chapter_cache: true,
            search_limit: Some(50),
            concurrency: Some(8),
            proxy_enabled: true,
            proxy_host: "10.0.0.1".to_string(),
            proxy_port: 1080,
            qidian_cookie: "".to_string(),
            language: Language::English,
            theme_pref: ThemePref {
                kind: ThemeKind::Dynamic,
                dyn_mode: ThemeDynMode::Dark,
                dyn_light: "Catppuccin Latte".to_string(),
                dyn_dark: "Catppuccin Mocha".to_string(),
                ..ThemePref::default()
            },
            sidebar_collapsed: true,
            font_size: 20.0,
            ..AppConfig::default()
        };

        save_config(&path, &cfg).unwrap();
        let loaded = load_config(&path).unwrap();

        assert_eq!(loaded.download_path, cfg.download_path);
        assert_eq!(loaded.ext_name, cfg.ext_name);
        assert_eq!(loaded.txt_encoding, cfg.txt_encoding);
        assert_eq!(loaded.preserve_chapter_cache, cfg.preserve_chapter_cache);
        assert_eq!(loaded.search_limit, cfg.search_limit);
        assert_eq!(loaded.concurrency, cfg.concurrency);
        assert_eq!(loaded.proxy_enabled, cfg.proxy_enabled);
        assert_eq!(loaded.proxy_host, cfg.proxy_host);
        assert_eq!(loaded.proxy_port, cfg.proxy_port);
        assert_eq!(loaded.qidian_cookie, cfg.qidian_cookie);
        assert_eq!(loaded.language, Language::English);
        assert_eq!(loaded.theme_pref, cfg.theme_pref);
        assert!(loaded.sidebar_collapsed);
        assert_eq!(loaded.font_size, 20.0);
    }

    #[test]
    fn save_preserves_user_comments_in_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"# 我的自定义注释
                [global]
                auto-update = false
                gh-proxy = "https://my-proxy.example/"
            "#,
        )
        .unwrap();

        let mut cfg = load_config(&path).unwrap();
        assert_eq!(cfg.gh_proxy, "https://my-proxy.example/");
        cfg.gh_proxy = "https://changed.example/".to_string();

        save_config(&path, &cfg).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.contains("# 我的自定义注释"),
            "注释应保留: {written}"
        );
        assert!(
            written.contains("https://changed.example/"),
            "新值应写入: {written}"
        );
    }

    #[test]
    fn missing_optional_int_keys_become_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[source]
                search-filter = true
            "#,
        )
        .unwrap();

        let cfg = load_config(&path).unwrap();
        // source-id / search-limit / concurrency 都没填，应当是 None
        assert!(cfg.source_id.is_none());
        assert!(cfg.search_limit.is_none());
        assert!(cfg.concurrency.is_none());
    }

    #[test]
    fn language_maps_to_book_target_lang() {
        // 合并设置后，UI 语言的繁体选项 → 下载目标语言为 ZhTw（不是 ZhHant）。
        assert_eq!(
            Language::SimplifiedChinese.to_book_target_lang(),
            LangType::ZhCn
        );
        assert_eq!(
            Language::TraditionalChinese.to_book_target_lang(),
            LangType::ZhTw
        );
        // 英文 / 其它 → 回落简体（用户要求）。
        assert_eq!(Language::English.to_book_target_lang(), LangType::ZhCn);
    }

    #[test]
    fn load_ignores_orphan_source_language_key() {
        // 老用户配置文件里可能还留着 `[source].language = "..."`，加载时必须容忍。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[source]
                search-filter = true
            "#,
        )
        .unwrap();

        let cfg = load_config(&path).unwrap();
        assert!(cfg.search_filter);
    }
}
