//! 首页 / 健康检查 / 书源 / 设置。

use axum::body::Body;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, Json, Response};
use axum_session::{Session, SessionNullPool};
use serde::{Deserialize, Serialize};

use crate::config::AppConfig;

use super::super::SharedState;

/// 前端 HTML 模板（编译期嵌入）。
const INDEX_TEMPLATE: &str = include_str!("index.html");
/// 自定义 CSS（编译期嵌入）。
const STYLE_CSS: &str = include_str!("../static/style.css");
/// 页面组件 HTML（编译期嵌入）。
const COMPONENTS_HTML: &str = include_str!("../static/components.html");
/// Alpine 状态 + API 调用（编译期嵌入）。
const APP_JS: &str = include_str!("../static/app.js");
/// 暗色模式跟随系统（编译期嵌入）。
const THEME_JS: &str = include_str!("../static/theme.js");
/// 浏览器图标（编译期嵌入）。
const FAVICON_ICO: &[u8] = include_bytes!("../../../assets/logo.ico");
const LOGO_PNG: &[u8] = include_bytes!("../../../assets/logo.png");

/// 编译期拼装：把 CSS / HTML / JS 注入模板占位符。
fn render_index() -> &'static str {
    let s = INDEX_TEMPLATE
        .replace("__STYLE__", STYLE_CSS)
        .replace("__BODY__", COMPONENTS_HTML)
        .replace("__THEME_JS__", THEME_JS)
        .replace("__APP_JS__", APP_JS);
    Box::leak(s.into_boxed_str())
}

pub async fn index_page() -> Html<&'static str> {
    Html(render_index())
}

fn static_asset(content_type: &'static str, data: &'static [u8]) -> Response<Body> {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "public, max-age=86400")
        .body(Body::from(data))
        .unwrap()
}

pub async fn favicon() -> Response<Body> {
    static_asset("image/x-icon", FAVICON_ICO)
}

pub async fn logo() -> Response<Body> {
    static_asset("image/png", LOGO_PNG)
}

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
    let now_disabled = {
        let db = state.db.lock().unwrap();
        crate::db::sources::toggle_disabled(db.conn(), id)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")))?
    };
    // 同步到内存中的 rules
    let mut rules = state.rules.write().unwrap();
    if let Some(r) = rules.iter_mut().find(|r| r.id == id) {
        r.disabled = now_disabled;
    }
    let r = rules
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "书源未找到".to_string()))?;
    Ok(Json(SourceInfo {
        id: r.id,
        name: r.name.clone(),
        url: r.url.clone(),
        enabled: !r.disabled,
    }))
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
}

pub async fn settings_put(
    State(state): State<SharedState>,
    Json(update): Json<SettingsUpdate>,
) -> Result<Json<AppConfig>, (StatusCode, String)> {
    let mut cfg = state.config.write().unwrap();

    if let Some(v) = update.download_path {
        cfg.download_path = v;
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

    let paths = crate::config::ConfigPaths::discover();
    if let Err(e) = crate::config::save_config(&paths.config_file, &cfg) {
        tracing::warn!("保存配置失败: {e}");
    }

    if let Err(e) = state.http.rebuild_proxy(&cfg) {
        tracing::warn!("重建 HTTP 客户端失败: {e}");
    }

    Ok(Json(cfg.clone()))
}

#[derive(Deserialize)]
pub(crate) struct AuthRequest {
    code: String,
}

#[derive(Serialize)]
pub(crate) struct AuthResponse {
    ok: bool,
}

pub async fn auth_verify(
    State(state): State<SharedState>,
    session: Session<SessionNullPool>,
    Json(req): Json<AuthRequest>,
) -> Json<AuthResponse> {
    let access_code = state.access_code.lock().unwrap();
    // access_code 为空表示不启用验证
    let ok = access_code.is_empty() || *access_code == req.code;
    if ok {
        session.set("authenticated", true);
    }
    Json(AuthResponse { ok })
}

#[derive(Serialize)]
pub(crate) struct AuthStatusResponse {
    required: bool,
    authenticated: bool,
}

pub async fn auth_status(
    State(state): State<SharedState>,
    session: Session<SessionNullPool>,
) -> Json<AuthStatusResponse> {
    let access_code = state.access_code.lock().unwrap();
    let required = !access_code.is_empty();
    let authenticated = session.get::<bool>("authenticated").unwrap_or(false);
    Json(AuthStatusResponse {
        required,
        authenticated,
    })
}

#[derive(Deserialize)]
pub(crate) struct AccessCodeUpdate {
    access_code: String,
}

/// 设置访问码（仅存内存，重启后失效）。
pub async fn set_access_code(
    State(state): State<SharedState>,
    Json(update): Json<AccessCodeUpdate>,
) -> Json<serde_json::Value> {
    let mut access_code = state.access_code.lock().unwrap();
    *access_code = update.access_code;
    Json(serde_json::json!({ "ok": true }))
}
