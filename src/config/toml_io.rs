//! `config.toml` 读写：核心 `load_config` / `save_config` + 各种 TOML helper。
//!
//! 设计目标：
//! - 配置文件就在项目根目录（`./config.toml`）— 与 Java 时代的 bundle/config.ini 不同；
//! - 用 `toml_edit` 保留注释 + 字段顺序，UI 设置页改完写回不会洗掉用户注释；
//! - 字段语义沿用旧版（`extname` / `min-interval` 等保留 kebab-case）；
//! - 旧 INI 默认 `1` / `0` 表示布尔，TOML 用真正的 bool；
//! - 旧版 `-1` 占位"未指定"的整数，TOML 一律用键缺失（`Option`）。
//!
//! `source-id` / `search-limit` / `concurrency` 在 TOML 里如果不写就视为未指定。

use std::path::Path;

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Item, value};

use super::defaults::default_template_doc;
use super::types::{AppConfig, ExportFormat, Language, ThemeDynMode, ThemeKind, ThemePref};

// ---------- TOML helper ----------

/// 从 TOML 文档中取 `table.key` 对应的 `Item`。
fn t_item<'a>(doc: &'a DocumentMut, table: &str, key: &str) -> Option<&'a Item> {
    t_table(doc, table).and_then(|t| t.get(key))
}

/// 从 TOML 文档中取 `table` 对应的 `Table`。
pub fn t_table<'a>(doc: &'a DocumentMut, table: &str) -> Option<&'a toml_edit::Table> {
    doc.get(table).and_then(|t| t.as_table())
}

/// 从 TOML 文档中取 `table.key` 对应的字符串值；空串视为 None。
fn t_str(doc: &DocumentMut, table: &str, key: &str) -> Option<String> {
    t_item(doc, table, key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

fn t_bool(doc: &DocumentMut, table: &str, key: &str) -> Option<bool> {
    t_item(doc, table, key).and_then(|v| v.as_bool())
}

fn t_int(doc: &DocumentMut, table: &str, key: &str) -> Option<i64> {
    t_item(doc, table, key).and_then(|v| v.as_integer())
}

/// 读浮点（兼容 TOML 里写成整数 `16` 或浮点 `16.0` 两种形式）。
fn t_float(doc: &DocumentMut, table: &str, key: &str) -> Option<f32> {
    let v = t_item(doc, table, key)?;
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

// ---------- load_config ----------

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
    // 旧键 `[global].theme = "X"`（单一主题名）的兼容迁移在本函数末尾 inline 做。
    let theme_kind = t_str(&doc, "global", "theme-kind");
    if let Some(v) = &theme_kind {
        cfg.theme_pref.kind = ThemeKind::parse(v);
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

    // [source]
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

    let theme_kind_present = t_table(&doc, "global")
        .and_then(|t| t.get("theme-kind"))
        .is_some();
    if !theme_kind_present {
        if let Some(v) = t_str(&doc, "global", "theme") {
            cfg.theme_pref = ThemePref {
                kind: ThemeKind::Static,
                static_name: v,
                ..ThemePref::default()
            };
        }
    }

    Ok(cfg)
}

// ---------- save_config ----------

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
    fn set_item(doc: &mut DocumentMut, table: &str, key: &str, v: impl Into<Item>) {
        let t = doc.entry(table).or_insert(Item::Table(Default::default()));
        if let Some(t) = t.as_table_mut() {
            t[key] = v.into();
        }
    }
    fn set_str(doc: &mut DocumentMut, table: &str, key: &str, v: &str) {
        set_item(doc, table, key, value(v));
    }
    fn set_bool(doc: &mut DocumentMut, table: &str, key: &str, v: bool) {
        set_item(doc, table, key, value(v));
    }
    fn set_int(doc: &mut DocumentMut, table: &str, key: &str, v: i64) {
        set_item(doc, table, key, value(v));
    }
    fn set_float(doc: &mut DocumentMut, table: &str, key: &str, v: f64) {
        set_item(doc, table, key, value(v));
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

    // [source]
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
    set_str(&mut doc, "cookie", "qidian-cookie", &cfg.qidian_cookie);

    // [proxy]
    set_bool(&mut doc, "proxy", "enabled", cfg.proxy_enabled);
    set_str(&mut doc, "proxy", "host", &cfg.proxy_host);
    set_int(&mut doc, "proxy", "port", cfg.proxy_port as i64);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // 原子写：先写同目录下的临时文件 → fsync → rename → 避免断电/进程崩溃
    // 时留下半截文件导致下次启动 config 解析失败。
    crate::db::write_atomically(path, doc.to_string().as_bytes())
        .with_context(|| format!("原子写入 {}", path.display()))?;
    Ok(())
}
