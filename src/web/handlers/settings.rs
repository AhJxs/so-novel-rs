//! 设置端点: 读取 (脱敏) / 写入 (部分字段)。
//!
//! ## 安全: GET 端点必须脱敏
//!
//! `qidian_cookie` 是 GPUI 桌面端专用的起点站 cookie, web 模式下不需要也**不应该**看到
//! 明文 cookie —— 否则监听 `0.0.0.0:8080` 时任意能访问端口的客户端都能拉走用户的起点站
//! cookie。`PublicSettings` 用 `has_qidian_cookie: bool` 替代, UI 仍能告知用户
//! "已设置/未设置"。

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;
use crate::utils::lock::{rw_read_or, rw_write_or};
use crate::web::SharedState;
use crate::web::error::{WebError, read_state_or_json};

/// `GET /api/settings` 返回的脱敏 DTO。
///
/// 关键差异 vs `AppConfig`:
/// - 不含 `qidian_cookie` 明文;
/// - 用 `has_qidian_cookie: bool` 替代, 让 UI 仍能告知用户"已设置/未设置";
/// - 字段顺序 / 命名与 `AppConfig` 完全一致, 前端按字段名取, 无破坏性变更。
#[derive(Serialize)]
pub struct PublicSettings {
    pub version: String,
    pub theme_pref: crate::config::ThemePref,
    pub language: crate::config::Language,
    pub gh_proxy: String,
    pub cf_bypass: String,
    pub sidebar_collapsed: bool,
    pub font_size: f32,
    pub download_path: String,
    pub ext_name: crate::config::ExportFormat,
    pub txt_encoding: String,
    pub preserve_chapter_cache: bool,
    pub search_limit: Option<i32>,
    pub search_filter: bool,
    pub concurrency: Option<i32>,
    pub min_interval: u32,
    pub max_interval: u32,
    pub enable_retry: bool,
    pub max_retries: u32,
    pub retry_min_interval: u32,
    pub retry_max_interval: u32,
    pub has_qidian_cookie: bool,
    pub proxy_enabled: bool,
    pub proxy_host: String,
    pub proxy_port: u16,
}

impl From<&AppConfig> for PublicSettings {
    fn from(cfg: &AppConfig) -> Self {
        Self {
            version: cfg.version.clone(),
            theme_pref: cfg.global.theme_pref.clone(),
            language: cfg.global.language,
            gh_proxy: cfg.global.gh_proxy.clone(),
            cf_bypass: cfg.global.cf_bypass.clone(),
            sidebar_collapsed: cfg.global.sidebar_collapsed,
            font_size: cfg.global.font_size,
            download_path: cfg.download.download_path.clone(),
            ext_name: cfg.download.ext_name,
            txt_encoding: cfg.download.txt_encoding.clone(),
            preserve_chapter_cache: cfg.download.preserve_chapter_cache,
            search_limit: cfg.source.search_limit,
            search_filter: cfg.source.search_filter,
            concurrency: cfg.crawl.concurrency,
            min_interval: cfg.crawl.min_interval,
            max_interval: cfg.crawl.max_interval,
            max_retries: cfg.crawl.max_retries,
            enable_retry: cfg.crawl.enable_retry,
            retry_min_interval: cfg.crawl.retry_min_interval,
            retry_max_interval: cfg.crawl.retry_max_interval,
            has_qidian_cookie: !cfg.cookie.qidian_cookie.trim().is_empty(),
            proxy_enabled: cfg.proxy.proxy_enabled,
            proxy_host: cfg.proxy.proxy_host.clone(),
            proxy_port: cfg.proxy.proxy_port,
        }
    }
}

/// `PUT /api/settings` 入参: 全部字段 Option, 缺省表示不修改。
#[derive(Deserialize)]
pub struct SettingsUpdate {
    pub download_path: Option<String>,
    pub ext_name: Option<String>,
    pub txt_encoding: Option<String>,
    pub search_filter: Option<bool>,
    pub proxy_enabled: Option<bool>,
    pub proxy_host: Option<String>,
    pub proxy_port: Option<u16>,
    pub concurrency: Option<i32>,
    pub max_retries: Option<u32>,
    pub enable_retry: Option<bool>,
    pub language: Option<crate::config::Language>,
}

/// `GET /api/settings` — 返回脱敏后的 `PublicSettings`。
///
/// # Errors
///
/// - `WebError::Internal` (500) — `state.config` 锁被毒化
#[tracing::instrument(name = "web::settings_get", skip_all)]
pub async fn settings_get(
    State(state): State<SharedState>,
) -> Result<Json<PublicSettings>, WebError> {
    let cfg = read_state_or_json("settings_get", || rw_read_or("settings_get", &state.config))?;
    Ok(Json(PublicSettings::from(&*cfg)))
}

/// `PUT /api/settings` — 部分字段写入 + 落盘 `config.toml` + 重建 HTTP proxy client。
///
/// `download_path` 若被修改: 必须非空且为已存在的目录 (自动保存前端会先做非空校验,
/// 目录存在性只能后端判断)。校验失败返回 400, 前端据此在字段下显示错误 —— 通过
/// 错误 `code` 字段（`3004` / `3005`）区分两种 empty / `not_dir` 情况，比原来
/// `msg.includes('download_path_empty')` 字符串匹配更稳。
///
/// 改 `proxy_*` 字段会触发 `state.http.rebuild_proxy(&cfg)` —— reqwest client 构造后
/// 不能 in-place 改 proxy, `整体重建。gh_proxy` 客户端不受影响。
///
/// # Errors
///
/// - `WebError::DownloadPathEmpty` (400, code 3004) — `download_path` 是空串
/// - `WebError::DownloadPathNotDir` (400, code 3005) — `download_path` 不是已存在目录
/// - `WebError::Internal` (500) — `state.config` 锁被毒化
#[tracing::instrument(name = "web::settings_put", skip_all)]
pub async fn settings_put(
    State(state): State<SharedState>,
    Json(update): Json<SettingsUpdate>,
) -> Result<Json<AppConfig>, WebError> {
    if let Some(v) = &update.download_path {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            return Err(WebError::DownloadPathEmpty);
        }
        if !std::path::Path::new(trimmed).is_dir() {
            return Err(WebError::DownloadPathNotDir);
        }
    }

    let mut cfg = read_state_or_json("settings_put", || {
        rw_write_or("settings_put", &state.config)
    })?;

    if let Some(v) = update.download_path {
        cfg.download.download_path = v.trim().to_string();
    }
    if let Some(v) = update.ext_name {
        cfg.download.ext_name = crate::config::ExportFormat::parse(&v);
    }
    if let Some(v) = update.txt_encoding {
        cfg.download.txt_encoding = v;
    }
    if let Some(v) = update.search_filter {
        cfg.source.search_filter = v;
    }
    if let Some(v) = update.proxy_enabled {
        cfg.proxy.proxy_enabled = v;
    }
    if let Some(v) = update.proxy_host {
        cfg.proxy.proxy_host = v;
    }
    if let Some(v) = update.proxy_port {
        cfg.proxy.proxy_port = v;
    }
    if let Some(v) = update.concurrency {
        cfg.crawl.concurrency = Some(v);
    }
    if let Some(v) = update.max_retries {
        cfg.crawl.max_retries = v;
    }
    if let Some(v) = update.enable_retry {
        cfg.crawl.enable_retry = v;
    }
    if let Some(v) = update.language {
        cfg.global.language = v;
    }

    let paths = crate::config::ConfigPaths::discover();
    if let Err(e) = crate::config::save_config(&paths.config_file, &cfg) {
        tracing::warn!("保存配置失败: {e}");
    }

    if let Err(e) = state.http.rebuild_proxy(&cfg) {
        tracing::warn!("重建 HTTP 客户端失败: {e}");
    }

    Ok(Json(cfg.clone()))
}
