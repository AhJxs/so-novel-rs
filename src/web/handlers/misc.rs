//! 健康检查 / 书源 / 设置。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::config::{AppConfig, CookieCfg, CrawlCfg, DownloadCfg, GlobalCfg, ProxyCfg, SourceCfg};

use super::super::SharedState;
use super::lock::{rw_read, rw_write};

pub async fn health(State(state): State<SharedState>) -> Json<HealthInfo> {
    // 健康检查：返回 server 版本 + build feature + rules 数量 + 当前任务数。
    // 用途：
    // - Docker HEALTHCHECK（已有 `wget --spider`，但 JSON 端点能区分 alive / ready）
    // - K8s readinessProbe / livenessProbe
    // - 监控抓点（无锁读规则数 + 任务数指标）
    //
    // 设计上**不**做深度检查（不发 HTTP 请求验证书源）—— 那是 /api/sources/{id}/test
    // 的事。health 只证明"进程跑着 + 内存里的核心状态可访问"。
    let rules_count = state.rules.read().map(|r| r.len()).unwrap_or_else(|e| {
        tracing::warn!("health: rules RwLock poisoned: {e}");
        0
    });
    let active_tasks = state
        .tasks
        .lock()
        .map(|t| t.iter().filter(|t| t.finished.is_none()).count())
        .unwrap_or_else(|e| {
            tracing::warn!("health: tasks Mutex poisoned: {e}");
            0
        });

    Json(HealthInfo {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        rules_count,
        active_tasks,
    })
}

#[derive(Serialize)]
pub struct HealthInfo {
    /// 字面量 "ok"；未来可扩展为 "degraded" 之类状态机。
    status: &'static str,
    /// `Cargo.toml` 的 `version` 字段，编译期嵌入。
    version: &'static str,
    /// 当前内存中的书源数量（含禁用）。
    rules_count: usize,
    /// 未结束的任务数（下载中 + 排队中，不含已完成 / 失败 / 已取消）。
    active_tasks: usize,
}

/// GET /api/settings 返回的脱敏 DTO。
///
/// 关键差异 vs `AppConfig`:
/// - 不含 `qidian_cookie` 明文（web 模式下 cookie 仅 GPUI 桌面端使用，
///   web 端不需要也**不应该**看到明文 cookie —— 否则监听 0.0.0.0:8080 时
///   任意能访问端口的客户端都能拉走用户的起点站 cookie）
/// - 用 `has_qidian_cookie: bool` 替代，让 UI 仍能告知用户"已设置/未设置"
/// - 字段顺序 / 命名与 `AppConfig` 完全一致，前端按字段名取，无破坏性变更
#[derive(Serialize)]
pub(crate) struct PublicSettings {
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

#[derive(Serialize)]
pub(crate) struct SourceInfo {
    id: i32,
    name: String,
    url: String,
    enabled: bool,
}

pub async fn sources_list(
    State(state): State<SharedState>,
) -> Result<Json<Vec<SourceInfo>>, (StatusCode, String)> {
    let rules = rw_read("sources_list", &state.rules)?;
    let sources: Vec<SourceInfo> = rules
        .iter()
        .map(|r| SourceInfo {
            id: r.id,
            name: r.name.clone(),
            url: r.url.clone(),
            enabled: !r.disabled,
        })
        .collect();
    Ok(Json(sources))
}

pub async fn source_toggle(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<i32>,
) -> Result<Json<SourceInfo>, (StatusCode, String)> {
    // 1. 先从 rules 中取到目标书源的 URL（短锁，取完即放）。
    let url = {
        let rules = rw_read("source_toggle:read_url", &state.rules)?;
        rules
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.url.clone())
            .ok_or_else(|| (StatusCode::NOT_FOUND, "书源未找到".to_string()))?
    };

    // 2. 切换 SourcesConfig 并持久化到磁盘。
    let now_disabled = {
        let mut sc = rw_write("source_toggle:write_sc", &state.sources_config)?;
        let d = sc.toggle_disabled(&url);
        if let Err(e) = sc.save(&state.sources_config_path) {
            tracing::warn!("保存 sources_config.json 失败: {e}");
        }
        d
    };

    // 3. 同步更新内存中的 Rule.disabled。
    {
        let mut rules = rw_write("source_toggle:write_rules", &state.rules)?;
        if let Some(r) = rules.iter_mut().find(|r| r.id == id) {
            r.disabled = now_disabled;
        }
    }

    // 4. 返回更新后的信息。
    let rules = rw_read("source_toggle:read_back", &state.rules)?;
    let r = rules
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "书源未找到".to_string()))?;
    let info = SourceInfo {
        id: r.id,
        name: r.name.clone(),
        url: r.url.clone(),
        enabled: !r.disabled,
    };
    drop(rules);

    tracing::info!("书源 #{id} 切换禁用状态: {now_disabled}");
    Ok(Json(info))
}

#[derive(Serialize)]
pub(crate) struct SourceTestResult {
    ok: bool,
    latency_ms: u64,
    error: Option<String>,
}

pub async fn source_test(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<i32>,
) -> Result<Json<SourceTestResult>, (StatusCode, String)> {
    let (url, client) = {
        let rules = rw_read("source_test", &state.rules)?;
        let rule = match rules.iter().find(|r| r.id == id).cloned() {
            Some(r) => r,
            None => {
                return Ok(Json(SourceTestResult {
                    ok: false,
                    latency_ms: 0,
                    error: Some("书源未找到".into()),
                }));
            }
        };
        let client = state.http.for_rule(&rule);
        (rule.url.clone(), client)
    };

    let start = std::time::Instant::now();
    let result = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;
    let latency = start.elapsed().as_millis() as u64;

    Ok(Json(match result {
        Ok(resp) => {
            let ok = resp.status().is_success();
            SourceTestResult {
                ok,
                latency_ms: latency,
                error: if ok {
                    None
                } else {
                    Some(format!("HTTP {}", resp.status()))
                },
            }
        }
        Err(e) => SourceTestResult {
            ok: false,
            latency_ms: latency,
            error: Some(e.to_string()),
        },
    }))
}

pub async fn settings_get(
    State(state): State<SharedState>,
) -> Result<Json<PublicSettings>, (StatusCode, String)> {
    let cfg = rw_read("settings_get", &state.config)?;
    Ok(Json(PublicSettings::from(&*cfg)))
}

#[derive(Deserialize)]
pub(crate) struct SettingsUpdate {
    download_path: Option<String>,
    ext_name: Option<String>,
    txt_encoding: Option<String>,
    search_filter: Option<bool>,
    proxy_enabled: Option<bool>,
    proxy_host: Option<String>,
    proxy_port: Option<u16>,
    concurrency: Option<i32>,
    max_retries: Option<u32>,
    enable_retry: Option<bool>,
    language: Option<crate::config::Language>,
}

pub async fn settings_put(
    State(state): State<SharedState>,
    Json(update): Json<SettingsUpdate>,
) -> Result<Json<AppConfig>, (StatusCode, String)> {
    // download_path 若被修改：必须非空且为已存在的目录（自动保存前端会先做非空校验，
    // 目录存在性只能后端判断）。校验失败返回 400，前端据此在字段下显示错误。
    if let Some(v) = &update.download_path {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            return Err((StatusCode::BAD_REQUEST, "download_path_empty".into()));
        }
        if !std::path::Path::new(trimmed).is_dir() {
            return Err((StatusCode::BAD_REQUEST, "download_path_not_dir".into()));
        }
    }

    let mut cfg = rw_write("settings_put", &state.config)?;

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
