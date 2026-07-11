//! 书源管理端点: 列表 / 禁用切换 / 连通性测试。
//!
//! ## 锁协议
//!
//! - `source_toggle`: 分四步短锁, 每一步取完即放, 避免持锁期间做磁盘 IO。
//! - `source_test`: 只读锁拿 `rule.url`, 释放后再发 HTTP。
//!
//! 注: 本模块定义的 `SourceInfo` 是 web 层 DTO（仅 4 字段: id/name/url/enabled）,
//! 与 `models::SourceInfo`（10 字段, 含 health / `delay_ms` / `http_status）不同`。
//!
//! ## i18n
//!
//! 错误走 [`WebError`]（按请求 locale 翻译 `message`，`code` 稳定不变）。
//! `source_test` 端点始终返 200 + `SourceTestResult.error: Option<String>`，
//! 该 string 自身是 localized text（书源未找到 / "HTTP {status}" 模板）。

use axum::Json;
use axum::extract::State;

use crate::core::sources as core_sources;
use crate::i18n::ts_for_locale;
use crate::utils::lock::{rw_read_or, rw_write_or};
use crate::web::SharedState;
use crate::web::error::{WebError, read_state_or_json};
use crate::web::locale::Locale;

/// `GET /api/sources` / `POST /api/sources/{id}/toggle` 响应体。
#[derive(serde::Serialize)]
pub struct SourceInfo {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub enabled: bool,
}

/// `GET /api/sources` — 列出所有书源（含禁用）。
///
/// # Errors
///
/// - `WebError::Internal` (500) — `state.rules` 锁被毒化
#[tracing::instrument(name = "web::sources_list", skip_all)]
pub async fn sources_list(
    State(state): State<SharedState>,
) -> Result<Json<Vec<SourceInfo>>, WebError> {
    let rules = read_state_or_json("sources_list", || rw_read_or("sources_list", &state.rules))?;
    let sources: Vec<SourceInfo> = rules
        .iter()
        .map(|r| SourceInfo {
            id: r.id,
            name: r.name.clone(),
            url: r.url.clone(),
            enabled: !r.disabled,
        })
        .collect();
    drop(rules);
    Ok(Json(sources))
}

/// `POST /api/sources/{id}/toggle` — 切换书源禁用状态 + 落盘 `sources_config.json`。
///
/// # Errors
///
/// - `WebError::NotFound` (404) — 书源 id 不存在
/// - `WebError::Internal` (500) — 锁被毒化
#[tracing::instrument(name = "web::source_toggle", skip_all, fields(source_id = id))]
pub async fn source_toggle(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<i32>,
) -> Result<Json<SourceInfo>, WebError> {
    let url = {
        let rules = read_state_or_json("source_toggle:read_url", || {
            rw_read_or("source_toggle:read_url", &state.rules)
        })?;
        core_sources::find_rule_by_id(&rules, id)
            .map(|r| r.url.clone())
            .ok_or(WebError::NotFound(""))?
    };

    let (now_disabled, to_save) = {
        let mut sc = read_state_or_json("source_toggle:write_sc", || {
            rw_write_or("source_toggle:write_sc", &state.sources_config)
        })?;
        let d = sc.toggle_disabled(&url);
        (d, sc.clone())
    };
    if let Err(e) = to_save.save(&state.sources_config_path) {
        tracing::warn!("保存 sources_config.json 失败: {e}");
    }

    read_state_or_json("source_toggle:write_rules", || -> Result<(), String> {
        let mut rules = rw_write_or("source_toggle:write_rules", &state.rules)?;
        if core_sources::find_rule_by_id(&rules, id).is_some()
            && let Some(r) = rules.iter_mut().find(|r| r.id == id)
        {
            r.disabled = now_disabled;
        }
        // 显式 drop, 让锁尽早释放 (clippy::significant_drop_tightening)
        drop(rules);
        Ok(())
    })?;

    let rules = read_state_or_json("source_toggle:read_back", || {
        rw_read_or("source_toggle:read_back", &state.rules)
    })?;
    let r = core_sources::find_rule_by_id(&rules, id).ok_or(WebError::NotFound(""))?;
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

/// `POST /api/sources/{id}/test` 响应体。
#[derive(serde::Serialize)]
pub struct SourceTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// `POST /api/sources/{id}/test` — 用 GET 探活, 10s 超时。
///
/// 端点**始终**返 200 + JSON body —— 即使书源未找到，也用 `SourceTestResult.error`
/// 字段报错（前端统一按 JSON 渲染，不必为这个 case 单独处理 404）。
///
/// `error` 字段是按 locale 翻译的文案（书源未找到 / "HTTP {status}" 模板 /
/// reqwest 错误原文本）。
///
/// # Errors
///
/// - `WebError::Internal` (500) — `state.rules` 锁被毒化
#[tracing::instrument(name = "web::source_test", skip_all, fields(source_id = id))]
pub async fn source_test(
    Locale(locale): Locale,
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<i32>,
) -> Result<Json<SourceTestResult>, WebError> {
    let rule = {
        let rules = read_state_or_json("source_test", || rw_read_or("source_test", &state.rules))?;
        match core_sources::find_rule_by_id_cloned(&rules, id) {
            Some(r) => r,
            None => {
                return Ok(Json(SourceTestResult {
                    ok: false,
                    latency_ms: 0,
                    error: Some(ts_for_locale(locale, "WebErrors.source_not_found")),
                }));
            }
        }
    };
    let client = state.http.for_rule(&rule);
    let url = rule.url;
    let (url, client) = (url, client);

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
                    // `WebErrors.source_test_http_status` 在 3 locale 都是字面量
                    // `"HTTP {status}"` —— 不是自然语言，所以 3 locale 输出相同。
                    Some(ts_for_locale(locale, "WebErrors.source_test_http_status"))
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
