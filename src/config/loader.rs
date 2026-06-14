//! `config.ini` 兼容读写。
//!
//! 设计目标：
//! - 直接读取 `bundle/config.ini`（Java 版的旧配置文件），字段语义保持一致；
//! - 写回时保留段落顺序与字段顺序，UI"设置页"修改后能往原文件写入而不丢失字段；
//! - Java 端用 `1`/`0` 表示布尔，本 Rust 版对外暴露 `bool`，序列化时统一回写 `1`/`0`；
//! - Java 端用 `-1` 表示"未指定"的整数（concurrency / source-id / search-limit），
//!   本 Rust 版对应 `Option<...>`，写回时缺省值发出 `-1` 以保持兼容。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use configparser::ini::Ini;
use serde::{Deserialize, Serialize};

use crate::util::lang::detect_system_lang;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
    Epub,
    Txt,
    Html,
    /// 阶段一不实现 PDF 导出，仅保留枚举以便兼容旧 config.ini，
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
        // 注意：Java 端 `LangUtil.getCurrentLang` 把 zh_TW 与 zh_Hant 视为不同；
        // 我们这里也保留这个区分。
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
    pub active_rules: String,
    /// `-1` 表示未指定（Java 默认值）。
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

            download_path: "downloads".to_string(),
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

/// 读取 INI 中某 section/key 的字符串。空字符串视为"未设置"，返回默认值。
fn ini_str(ini: &Ini, section: &str, key: &str, default: &str) -> String {
    match ini.get(section, key) {
        Some(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => default.to_string(),
    }
}

/// 读取整数（带默认值）。
fn ini_int(ini: &Ini, section: &str, key: &str, default: i64) -> i64 {
    ini.getint(section, key).ok().flatten().unwrap_or(default)
}

/// 读取 0/1 形式的布尔。
fn ini_bool01(ini: &Ini, section: &str, key: &str, default: bool) -> bool {
    match ini.get(section, key) {
        Some(v) => match v.trim() {
            "1" => true,
            "0" => false,
            "" => default,
            other => {
                // 容错：true/false 也接受
                matches!(other.to_ascii_lowercase().as_str(), "true" | "yes" | "on")
            }
        },
        None => default,
    }
}

/// 把 i64 转 i32，溢出时取饱和。
fn sat_i32(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// 加载配置。文件不存在时返回 `Default::default()`，但保留 `path` 信息便于回写。
#[allow(clippy::field_reassign_with_default)] // 字段-逐条赋值与 INI 段落对齐更易读
pub fn load_config(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        // 与 Java 端不同，缺失时不抛异常；UI 第一次启动会写出默认 ini。
        return Ok(AppConfig::default());
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("read config.ini failed: {}", path.display()))?;
    let mut ini = Ini::new_cs(); // case-sensitive section/key
    ini.read(content)
        .map_err(|e| anyhow::anyhow!("parse config.ini failed: {e}"))?;

    let mut cfg = AppConfig::default();

    // [global]
    cfg.auto_update = ini_bool01(&ini, "global", "auto-update", false);
    cfg.gh_proxy = ini_str(&ini, "global", "gh-proxy", "");
    cfg.cf_bypass = ini_str(&ini, "global", "cf-bypass", "");

    // [download]
    cfg.download_path = ini_str(&ini, "download", "download-path", "downloads");
    cfg.ext_name = ExportFormat::parse(&ini_str(&ini, "download", "extname", "epub"));
    cfg.txt_encoding = ini_str(&ini, "download", "txt-encoding", "UTF-8");
    cfg.preserve_chapter_cache = ini_bool01(&ini, "download", "preserve-chapter-cache", false);
    cfg.enable_progressbar = ini_bool01(&ini, "download", "enable-progressbar", true);

    // [source]
    let lang_raw = ini_str(&ini, "source", "language", "");
    cfg.language = LangType::parse(&lang_raw).unwrap_or_else(detect_system_lang);
    cfg.active_rules = ini_str(&ini, "source", "active-rules", "main.json");
    let source_id_raw = ini_int(&ini, "source", "source-id", -1);
    cfg.source_id = if source_id_raw < 0 {
        None
    } else {
        Some(sat_i32(source_id_raw))
    };
    let limit_raw = ini_int(&ini, "source", "search-limit", -1);
    cfg.search_limit = if limit_raw < 0 {
        None
    } else {
        Some(sat_i32(limit_raw))
    };
    cfg.search_filter = ini_bool01(&ini, "source", "search-filter", true);

    // [crawl]
    let conc = ini_int(&ini, "crawl", "concurrency", -1);
    cfg.concurrency = if conc < 0 { None } else { Some(sat_i32(conc)) };
    cfg.min_interval = ini_int(&ini, "crawl", "min-interval", 200).max(0) as u32;
    cfg.max_interval = ini_int(&ini, "crawl", "max-interval", 400).max(0) as u32;
    cfg.enable_retry = ini_bool01(&ini, "crawl", "enable-retry", true);
    cfg.max_retries = ini_int(&ini, "crawl", "max-retries", 5).max(0) as u32;
    cfg.retry_min_interval = ini_int(&ini, "crawl", "retry-min-interval", 2000).max(0) as u32;
    cfg.retry_max_interval = ini_int(&ini, "crawl", "retry-max-interval", 4000).max(0) as u32;

    // [web]
    cfg.web_enabled = ini_bool01(&ini, "web", "enabled", false);
    cfg.web_port = ini_int(&ini, "web", "port", 7765).clamp(0, u16::MAX as i64) as u16;

    // [cookie]
    cfg.qidian_cookie = ini_str(&ini, "cookie", "qidian", "");

    // [proxy]
    cfg.proxy_enabled = ini_bool01(&ini, "proxy", "enabled", false);
    cfg.proxy_host = ini_str(&ini, "proxy", "host", "127.0.0.1");
    cfg.proxy_port = ini_int(&ini, "proxy", "port", 7890).clamp(0, u16::MAX as i64) as u16;

    Ok(cfg)
}

/// 把 AppConfig 写回 INI。保持 Java 端字段名与 0/1 表示。
pub fn save_config(path: &Path, cfg: &AppConfig) -> Result<()> {
    let mut out = String::new();

    // 简单的手写序列化保证字段顺序与 Java 端一致。
    out.push_str("[global]\n");
    out.push_str(&format!("auto-update = {}\n", bool_to_01(cfg.auto_update)));
    out.push_str(&format!("gh-proxy = {}\n", cfg.gh_proxy));
    out.push_str(&format!("cf-bypass = {}\n\n", cfg.cf_bypass));

    out.push_str("[download]\n");
    out.push_str(&format!("download-path = {}\n", cfg.download_path));
    out.push_str(&format!("extname = {}\n", cfg.ext_name.as_lower()));
    out.push_str(&format!("txt-encoding = {}\n", cfg.txt_encoding));
    out.push_str(&format!(
        "preserve-chapter-cache = {}\n",
        bool_to_01(cfg.preserve_chapter_cache)
    ));
    out.push_str(&format!(
        "enable-progressbar = {}\n\n",
        bool_to_01(cfg.enable_progressbar)
    ));

    out.push_str("[source]\n");
    out.push_str(&format!("language = {}\n", cfg.language.as_str()));
    out.push_str(&format!("active-rules = {}\n", cfg.active_rules));
    out.push_str(&format!(
        "source-id = {}\n",
        cfg.source_id.map(|v| v.to_string()).unwrap_or_default()
    ));
    out.push_str(&format!(
        "search-limit = {}\n",
        cfg.search_limit
            .map(|v| v.to_string())
            .unwrap_or_else(|| "30".to_string())
    ));
    out.push_str(&format!(
        "search-filter = {}\n\n",
        bool_to_01(cfg.search_filter)
    ));

    out.push_str("[crawl]\n");
    out.push_str(&format!(
        "concurrency = {}\n",
        cfg.concurrency.map(|v| v.to_string()).unwrap_or_default()
    ));
    out.push_str(&format!("min-interval = {}\n", cfg.min_interval));
    out.push_str(&format!("max-interval = {}\n", cfg.max_interval));
    out.push_str(&format!(
        "enable-retry = {}\n",
        bool_to_01(cfg.enable_retry)
    ));
    out.push_str(&format!("max-retries = {}\n", cfg.max_retries));
    out.push_str(&format!(
        "retry-min-interval = {}\n",
        cfg.retry_min_interval
    ));
    out.push_str(&format!(
        "retry-max-interval = {}\n\n",
        cfg.retry_max_interval
    ));

    out.push_str("[web]\n");
    out.push_str(&format!("enabled = {}\n", bool_to_01(cfg.web_enabled)));
    out.push_str(&format!("port = {}\n\n", cfg.web_port));

    out.push_str("[cookie]\n");
    out.push_str(&format!("qidian = {}\n\n", cfg.qidian_cookie));

    out.push_str("[proxy]\n");
    out.push_str(&format!("enabled = {}\n", bool_to_01(cfg.proxy_enabled)));
    out.push_str(&format!("host = {}\n", cfg.proxy_host));
    out.push_str(&format!("port = {}\n", cfg.proxy_port));

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, out).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

fn bool_to_01(b: bool) -> u8 {
    if b {
        1
    } else {
        0
    }
}

/// 程序启动时关心的几条路径。
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    /// `config.ini` 路径。
    pub config_file: PathBuf,
    /// 规则目录或单文件路径。如果 `active-rules` 是文件名，
    /// 会被拼到 `<repo>/bundle/rules/` 或 `<bin>/rules/` 下。
    pub rules_dir: PathBuf,
    /// 用户对书源的覆写（启用/禁用）。sidecar JSON，不污染上游 rules JSON。
    /// 与 `config_file` 同目录。
    pub source_overrides_file: PathBuf,
    /// SQLite 数据库文件，存下载任务记录（后续书源管理也走这里）。
    /// 与 `config_file` 同目录。
    pub download_db_file: PathBuf,
}

impl ConfigPaths {
    /// 按 Java 端的路径约定查找：
    /// 1. 当前目录下 `bundle/config.ini` + `bundle/rules/`（开发态，仓库根直接 `cargo run`）；
    /// 2. 当前目录下 `config.ini` + `rules/`（生产态，与 Java 打包一致）。
    pub fn discover() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let dev_cfg = cwd.join("bundle").join("config.ini");
        let dev_rules = cwd.join("bundle").join("rules");
        if dev_cfg.exists() && dev_rules.exists() {
            return Self {
                source_overrides_file: dev_cfg
                    .parent()
                    .map(|p| p.join("source-overrides.json"))
                    .unwrap_or_else(|| PathBuf::from("source-overrides.json")),
                download_db_file: dev_cfg
                    .parent()
                    .map(|p| p.join("downloads.db"))
                    .unwrap_or_else(|| PathBuf::from("downloads.db")),
                config_file: dev_cfg,
                rules_dir: dev_rules,
            };
        }

        Self {
            source_overrides_file: cwd.join("source-overrides.json"),
            download_db_file: cwd.join("downloads.db"),
            config_file: cwd.join("config.ini"),
            rules_dir: cwd.join("rules"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_bundle_config() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("bundle")
            .join("config.ini")
    }

    #[test]
    fn loads_default_when_missing() {
        let cfg = load_config(&PathBuf::from("/definitely/does/not/exist.ini")).unwrap();
        assert_eq!(cfg.active_rules, "main.json");
        assert_eq!(cfg.min_interval, 200);
        assert_eq!(cfg.max_interval, 400);
        assert!(cfg.enable_retry);
        assert!(cfg.search_filter);
        assert_eq!(cfg.web_port, 7765);
        assert_eq!(cfg.ext_name, ExportFormat::Epub);
    }

    #[test]
    fn loads_real_bundle_config_ini() {
        // 直接读仓库根 bundle/config.ini，确保字段映射没回归。
        let path = repo_bundle_config();
        assert!(
            path.exists(),
            "expected {} to exist (run from repo root)",
            path.display()
        );
        let cfg = load_config(&path).unwrap();

        // 来自 bundle/config.ini 的硬编码值（详见审计 §3.1）：
        assert!(!cfg.auto_update);
        assert_eq!(cfg.download_path, "downloads");
        assert_eq!(cfg.ext_name, ExportFormat::Epub);
        assert_eq!(cfg.txt_encoding, "UTF-8"); // 空串走默认
        assert!(!cfg.preserve_chapter_cache);
        assert!(cfg.enable_progressbar);

        assert_eq!(cfg.active_rules, "main.json");
        assert_eq!(cfg.search_limit, Some(30));
        assert!(cfg.search_filter);

        assert_eq!(cfg.min_interval, 200);
        assert_eq!(cfg.max_interval, 400);
        assert!(cfg.enable_retry);
        assert_eq!(cfg.retry_min_interval, 2000);
        assert_eq!(cfg.retry_max_interval, 4000);

        assert!(!cfg.web_enabled);
        assert_eq!(cfg.web_port, 7765);

        assert!(!cfg.proxy_enabled);
        assert_eq!(cfg.proxy_host, "127.0.0.1");
    }

    #[test]
    fn round_trip_through_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.ini");

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
}
