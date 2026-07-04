//! 健康检查 / 书源 / 设置。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

use super::super::SharedState;

pub async fn health() -> &'static str {
    "ok"
}

#[derive(Serialize)]
pub(crate) struct SourceInfo {
    id: i32,
    name: String,
    url: String,
    enabled: bool,
}

pub async fn sources_list(State(state): State<SharedState>) -> Json<Vec<SourceInfo>> {
    let rules = state.rules.read().unwrap();
    let sources: Vec<SourceInfo> = rules
        .iter()
        .map(|r| SourceInfo {
            id: r.id,
            name: r.name.clone(),
            url: r.url.clone(),
            enabled: !r.disabled,
        })
        .collect();
    Json(sources)
}

pub async fn source_toggle(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<i32>,
) -> Result<Json<SourceInfo>, (StatusCode, String)> {
    // 1. 先从 rules 中取到目标书源的 URL（短锁，取完即放）。
    let url = {
        let rules = state.rules.read().unwrap();
        rules
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.url.clone())
            .ok_or_else(|| (StatusCode::NOT_FOUND, "书源未找到".to_string()))?
    };

    // 2. 切换 SourcesConfig 并持久化到磁盘。
    let now_disabled = {
        let mut sc = state.sources_config.write().unwrap();
        let d = sc.toggle_disabled(&url);
        if let Err(e) = sc.save(&state.sources_config_path) {
            tracing::warn!("保存 sources_config.json 失败: {e}");
        }
        d
    };

    // 3. 同步更新内存中的 Rule.disabled。
    {
        let mut rules = state.rules.write().unwrap();
        if let Some(r) = rules.iter_mut().find(|r| r.id == id) {
            r.disabled = now_disabled;
        }
    }

    // 4. 返回更新后的信息。
    let rules = state.rules.read().unwrap();
    let r = rules.iter().find(|r| r.id == id).unwrap();
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
) -> Json<SourceTestResult> {
    let (url, client) = {
        let rules = state.rules.read().unwrap();
        let rule = match rules.iter().find(|r| r.id == id) {
            Some(r) => r.clone(),
            None => {
                return Json(SourceTestResult {
                    ok: false,
                    latency_ms: 0,
                    error: Some("书源未找到".into()),
                });
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

    match result {
        Ok(resp) => {
            let ok = resp.status().is_success();
            Json(SourceTestResult {
                ok,
                latency_ms: latency,
                error: if ok {
                    None
                } else {
                    Some(format!("HTTP {}", resp.status()))
                },
            })
        }
        Err(e) => Json(SourceTestResult {
            ok: false,
            latency_ms: latency,
            error: Some(e.to_string()),
        }),
    }
}

pub async fn settings_get(State(state): State<SharedState>) -> Json<AppConfig> {
    let cfg = state.config.read().unwrap();
    Json(cfg.clone())
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

    let mut cfg = state.config.write().unwrap();

    if let Some(v) = update.download_path {
        cfg.download_path = v.trim().to_string();
    }
    if let Some(v) = update.ext_name {
        cfg.ext_name = crate::config::ExportFormat::parse(&v);
    }
    if let Some(v) = update.txt_encoding {
        cfg.txt_encoding = v;
    }
    if let Some(v) = update.search_filter {
        cfg.search_filter = v;
    }
    if let Some(v) = update.proxy_enabled {
        cfg.proxy_enabled = v;
    }
    if let Some(v) = update.proxy_host {
        cfg.proxy_host = v;
    }
    if let Some(v) = update.proxy_port {
        cfg.proxy_port = v;
    }
    if let Some(v) = update.concurrency {
        cfg.concurrency = Some(v);
    }
    if let Some(v) = update.max_retries {
        cfg.max_retries = v;
    }
    if let Some(v) = update.enable_retry {
        cfg.enable_retry = v;
    }
    if let Some(v) = update.language {
        cfg.language = v;
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
