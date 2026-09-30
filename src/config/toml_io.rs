//! `config.toml` 读写：`load_config` / `save_config` + 各种 TOML helper。
//!
//! 用 `toml_edit` 而非 serde：保留注释 + 字段顺序，UI 设置页写回不会洗掉用户注释。
//! 键名保持 kebab-case（`extname` / `min-interval`，与既有配置文件兼容）；"未指定"的
//! 整数字段一律用**键缺失**（`Option`）表示，不写哨兵值 —— `source-id` / `search-limit` /
//! `concurrency` 不写即视为未指定。

use std::path::Path;

use anyhow::{Context, Result};
use toml_edit::{DocumentMut, Item, Table, value};

use super::defaults::default_template_doc;
use super::types::{AppConfig, ExportFormat, Language, ThemeDynMode, ThemeKind, ThemePref};

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
        .map(std::string::ToString::to_string)
        .filter(|s| !s.trim().is_empty())
}

fn t_bool(doc: &DocumentMut, table: &str, key: &str) -> Option<bool> {
    t_item(doc, table, key).and_then(toml_edit::Item::as_bool)
}

fn t_int(doc: &DocumentMut, table: &str, key: &str) -> Option<i64> {
    t_item(doc, table, key).and_then(toml_edit::Item::as_integer)
}

/// 读浮点（兼容 TOML 里写成整数 `16` 或浮点 `16.0` 两种形式）。
fn t_float(doc: &DocumentMut, table: &str, key: &str) -> Option<f32> {
    let v = t_item(doc, table, key)?;
    v.as_float()
        .map(|f| f as f32)
        // i 已由外层 `i32::try_from` clamp 到 i32 范围，i32→f32 只丢 9 位精度；
        // 配置值（端口 / 超时秒数）< 2^23 时无损。
        .or_else(|| {
            v.as_integer()
                .and_then(|i| i32::try_from(i).ok())
                .map(|i| (i as f64) as f32)
        })
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
///
/// # Examples
///
/// 启动路径见 `core::bootstrap`：`load_config(&paths.config_file)`。
///
/// # Errors
///
/// 文件读取失败 / `toml_edit` 语法错 / 字段类型转换失败，均由 `Context` 包装返回。
#[tracing::instrument(name = "config::load", skip_all, fields(path = %path.display()))]
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

    // [global]：旧键 `[global].theme = "X"`（单一主题名）的兼容迁移在本函数末尾做。
    let theme_kind = t_str(&doc, "global", "theme-kind");
    if let Some(v) = &theme_kind {
        cfg.global.theme_pref.kind = ThemeKind::parse(v);
    }
    if let Some(v) = t_str(&doc, "global", "theme-name") {
        cfg.global.theme_pref.static_name = v;
    }
    if let Some(v) = t_str(&doc, "global", "theme-dyn-mode") {
        cfg.global.theme_pref.dyn_mode = ThemeDynMode::parse(&v);
    }
    if let Some(v) = t_str(&doc, "global", "theme-light") {
        cfg.global.theme_pref.dyn_light = v;
    }
    if let Some(v) = t_str(&doc, "global", "theme-dark") {
        cfg.global.theme_pref.dyn_dark = v;
    }
    if let Some(v) = t_str(&doc, "global", "language")
        && let Some(parsed) = Language::parse(&v)
    {
        cfg.global.language = parsed;
    }
    if let Some(v) = t_str(&doc, "global", "gh-proxy") {
        cfg.global.gh_proxy = v;
    }
    if let Some(v) = t_str(&doc, "global", "cf-bypass") {
        cfg.global.cf_bypass = v;
    }
    if let Some(v) = t_bool(&doc, "global", "sidebar-collapsed") {
        cfg.global.sidebar_collapsed = v;
    }
    if let Some(v) = t_float(&doc, "global", "font-size") {
        cfg.global.font_size = v;
    }

    if let Some(v) = t_str(&doc, "download", "download-path") {
        cfg.download.download_path = v;
    }
    if let Some(v) = t_str(&doc, "download", "extname") {
        cfg.download.ext_name = ExportFormat::parse(&v);
    }
    if let Some(v) = t_str(&doc, "download", "txt-encoding") {
        cfg.download.txt_encoding = v;
    }
    if let Some(v) = t_bool(&doc, "download", "preserve-chapter-cache") {
        cfg.download.preserve_chapter_cache = v;
    }

    cfg.source.search_limit = t_int(&doc, "source", "search-limit").map(sat_i32);
    if let Some(v) = t_bool(&doc, "source", "search-filter") {
        cfg.source.search_filter = v;
    }

    cfg.crawl.concurrency = t_int(&doc, "crawl", "concurrency").map(sat_i32);
    if let Some(v) = t_int(&doc, "crawl", "min-interval") {
        cfg.crawl.min_interval = sat_u32(v);
    }
    if let Some(v) = t_int(&doc, "crawl", "max-interval") {
        cfg.crawl.max_interval = sat_u32(v);
    }
    if let Some(v) = t_bool(&doc, "crawl", "enable-retry") {
        cfg.crawl.enable_retry = v;
    }
    if let Some(v) = t_int(&doc, "crawl", "max-retries") {
        cfg.crawl.max_retries = sat_u32(v);
    }
    if let Some(v) = t_int(&doc, "crawl", "retry-min-interval") {
        cfg.crawl.retry_min_interval = sat_u32(v);
    }
    if let Some(v) = t_int(&doc, "crawl", "retry-max-interval") {
        cfg.crawl.retry_max_interval = sat_u32(v);
    }

    if let Some(v) = t_str(&doc, "cookie", "qidian-cookie") {
        cfg.cookie.qidian_cookie = v;
    }

    if let Some(v) = t_bool(&doc, "proxy", "enabled") {
        cfg.proxy.proxy_enabled = v;
    }
    if let Some(v) = t_str(&doc, "proxy", "host") {
        cfg.proxy.proxy_host = v;
    }
    if let Some(v) = t_int(&doc, "proxy", "port") {
        cfg.proxy.proxy_port = sat_u16(v);
    }

    let theme_kind_present = t_table(&doc, "global")
        .and_then(|t| t.get("theme-kind"))
        .is_some();
    if !theme_kind_present && let Some(v) = t_str(&doc, "global", "theme") {
        cfg.global.theme_pref = ThemePref {
            kind: ThemeKind::Static,
            static_name: v,
            ..ThemePref::default()
        };
    }

    Ok(cfg)
}

/// 把 `AppConfig` 写回 TOML：原文件存在就在它上面 in-place 改字段（保留注释），
/// 不存在则用统一模板生成。
///
/// # Examples
///
/// 设置页保存路径见 `web::handlers::settings` / `desktop::model::ops::settings`。
///
/// # Errors
///
/// 读旧文件 / 写新文件失败；旧 `config.toml` 解析失败时 fallback 用模板覆盖。
#[tracing::instrument(name = "config::save", skip_all, fields(path = %path.display()))]
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
        let t = doc.entry(table).or_insert(Item::Table(Table::default()));
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
    set_str(
        &mut doc,
        "global",
        "theme-kind",
        cfg.global.theme_pref.kind.as_str(),
    );
    set_str(
        &mut doc,
        "global",
        "theme-name",
        &cfg.global.theme_pref.static_name,
    );
    set_str(
        &mut doc,
        "global",
        "theme-dyn-mode",
        cfg.global.theme_pref.dyn_mode.as_str(),
    );
    set_str(
        &mut doc,
        "global",
        "theme-light",
        &cfg.global.theme_pref.dyn_light,
    );
    set_str(
        &mut doc,
        "global",
        "theme-dark",
        &cfg.global.theme_pref.dyn_dark,
    );
    set_str(&mut doc, "global", "language", cfg.global.language.as_str());
    set_str(&mut doc, "global", "gh-proxy", &cfg.global.gh_proxy);
    set_str(&mut doc, "global", "cf-bypass", &cfg.global.cf_bypass);
    set_bool(
        &mut doc,
        "global",
        "sidebar-collapsed",
        cfg.global.sidebar_collapsed,
    );
    set_float(&mut doc, "global", "font-size", cfg.global.font_size as f64);

    set_str(
        &mut doc,
        "download",
        "download-path",
        &cfg.download.download_path,
    );
    set_str(
        &mut doc,
        "download",
        "extname",
        cfg.download.ext_name.as_lower(),
    );
    set_str(
        &mut doc,
        "download",
        "txt-encoding",
        &cfg.download.txt_encoding,
    );
    set_bool(
        &mut doc,
        "download",
        "preserve-chapter-cache",
        cfg.download.preserve_chapter_cache,
    );

    match cfg.source.search_limit {
        Some(v) => set_int(&mut doc, "source", "search-limit", v as i64),
        None => unset(&mut doc, "source", "search-limit"),
    }
    set_bool(
        &mut doc,
        "source",
        "search-filter",
        cfg.source.search_filter,
    );

    match cfg.crawl.concurrency {
        Some(v) => set_int(&mut doc, "crawl", "concurrency", v as i64),
        None => unset(&mut doc, "crawl", "concurrency"),
    }
    set_int(
        &mut doc,
        "crawl",
        "min-interval",
        cfg.crawl.min_interval as i64,
    );
    set_int(
        &mut doc,
        "crawl",
        "max-interval",
        cfg.crawl.max_interval as i64,
    );
    set_bool(&mut doc, "crawl", "enable-retry", cfg.crawl.enable_retry);
    set_int(
        &mut doc,
        "crawl",
        "max-retries",
        cfg.crawl.max_retries as i64,
    );
    set_int(
        &mut doc,
        "crawl",
        "retry-min-interval",
        cfg.crawl.retry_min_interval as i64,
    );
    set_int(
        &mut doc,
        "crawl",
        "retry-max-interval",
        cfg.crawl.retry_max_interval as i64,
    );

    set_str(
        &mut doc,
        "cookie",
        "qidian-cookie",
        &cfg.cookie.qidian_cookie,
    );

    set_bool(&mut doc, "proxy", "enabled", cfg.proxy.proxy_enabled);
    set_str(&mut doc, "proxy", "host", &cfg.proxy.proxy_host);
    set_int(&mut doc, "proxy", "port", cfg.proxy.proxy_port as i64);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // 原子写：同目录临时文件 → fsync → rename，避免断电/崩溃留下半截 config。
    crate::db::write_atomically(path, doc.to_string().as_bytes())
        .with_context(|| format!("原子写入 {}", path.display()))?;
    Ok(())
}
