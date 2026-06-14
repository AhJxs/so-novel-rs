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
use toml_edit::{value, DocumentMut, Item};

use crate::util::lang::detect_system_lang;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub version: String,

    // [global]
    pub auto_update: bool,
    pub gh_proxy: String,
    pub cf_bypass: String,

    // [download]
    pub download_path: String,
    pub ext_name: ExportFormat,
    pub txt_encoding: String,
    pub preserve_chapter_cache: bool,
    pub enable_progressbar: bool,

    // [source]
    pub language: LangType,
    /// 兼容字段：旧 INI 用文件名标记当前规则集（如 `main.json`）。
    /// 规则迁到 SQLite 后，这个字段仅作为"标签"保留：UI 上仍可看到，
    /// 也可被未来的"规则集分组"功能利用，但运行时不再据此找文件。
    pub active_rules: String,
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

    // [web]
    pub web_enabled: bool,
    pub web_port: u16,

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

            auto_update: false,
            gh_proxy: String::new(),
            cf_bypass: String::new(),

            download_path: default_download_path(),
            ext_name: ExportFormat::Epub,
            txt_encoding: "UTF-8".to_string(),
            preserve_chapter_cache: false,
            enable_progressbar: true,

            language: detect_system_lang(),
            active_rules: "main.json".to_string(),
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

            web_enabled: false,
            web_port: 7765,

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

    // [global]
    if let Some(v) = t_bool(&doc, "global", "auto-update") {
        cfg.auto_update = v;
    }
    if let Some(v) = t_str(&doc, "global", "gh-proxy") {
        cfg.gh_proxy = v;
    }
    if let Some(v) = t_str(&doc, "global", "cf-bypass") {
        cfg.cf_bypass = v;
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
    if let Some(v) = t_str(&doc, "source", "language") {
        if let Some(parsed) = LangType::parse(&v) {
            cfg.language = parsed;
        }
    }
    if let Some(v) = t_str(&doc, "source", "active-rules") {
        cfg.active_rules = v;
    }
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

    // [web]
    if let Some(v) = t_bool(&doc, "web", "enabled") {
        cfg.web_enabled = v;
    }
    if let Some(v) = t_int(&doc, "web", "port") {
        cfg.web_port = sat_u16(v);
    }

    // [cookie]
    if let Some(v) = t_str(&doc, "cookie", "qidian") {
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
    fn unset(doc: &mut DocumentMut, table: &str, key: &str) {
        if let Some(t) = doc.get_mut(table).and_then(|t| t.as_table_mut()) {
            t.remove(key);
        }
    }

    // [global]
    set_bool(&mut doc, "global", "auto-update", cfg.auto_update);
    set_str(&mut doc, "global", "gh-proxy", &cfg.gh_proxy);
    set_str(&mut doc, "global", "cf-bypass", &cfg.cf_bypass);

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
    set_str(&mut doc, "source", "language", cfg.language.as_str());
    set_str(&mut doc, "source", "active-rules", &cfg.active_rules);
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

    // [web]
    set_bool(&mut doc, "web", "enabled", cfg.web_enabled);
    set_int(&mut doc, "web", "port", cfg.web_port as i64);

    // [cookie]
    set_str(&mut doc, "cookie", "qidian", &cfg.qidian_cookie);

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
auto-update = false
gh-proxy = ""
cf-bypass = ""

[download]
# download-path 默认为系统 Documents/Novel/（由 AppConfig::default() 注入）。
# 占位写空串，save_config 会按当前 cfg.download_path 覆盖此处的值。
download-path = ""
extname = "epub"
txt-encoding = "UTF-8"
preserve-chapter-cache = false
enable-progressbar = true

[source]
language = "zh_CN"
# active-rules 已不再用于定位文件（规则进 DB），保留为标签字段，方便未来分组。
active-rules = "main.json"
search-limit = 30
search-filter = true

[crawl]
min-interval = 200
max-interval = 400
enable-retry = true
max-retries = 5
retry-min-interval = 2000
retry-max-interval = 4000

[web]
enabled = false
port = 7765

[cookie]
qidian = ""

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
}

impl ConfigPaths {
    /// 路径约定：
    /// - 优先使用项目根（`current_dir`，开发态 `cargo run` 时就是仓库根）下的
    ///   `config.toml` + `sonovel.db`；
    /// - 这两个文件首次启动时会自动创建。
    ///
    /// 老版的 `bundle/config.ini` / `bundle/rules/` / `source-overrides.json` /
    /// `bundle/downloads.db` 不再被加载（文件可保留，仅作历史参考）。
    pub fn discover() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            config_file: cwd.join("config.toml"),
            db_file: cwd.join("sonovel.db"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_default_when_missing() {
        let cfg = load_config(&PathBuf::from("/definitely/does/not/exist.toml")).unwrap();
        assert_eq!(cfg.active_rules, "main.json");
        assert_eq!(cfg.min_interval, 200);
        assert_eq!(cfg.max_interval, 400);
        assert!(cfg.enable_retry);
        assert!(cfg.search_filter);
        assert_eq!(cfg.web_port, 7765);
        assert_eq!(cfg.ext_name, ExportFormat::Epub);
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
            qidian_cookie: "w_tsfp=demo".to_string(),
            language: LangType::ZhTw,
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
        assert_eq!(loaded.language, cfg.language);
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
}
