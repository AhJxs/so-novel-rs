//! 书源管理端点: 列表 / 禁用切换 / 连通性测试。
//!
//! ## 锁协议
//!
//! - `source_toggle`: 分四步短锁, 每一步取完即放, 避免持锁期间做磁盘 IO。
//! - `source_test`: 只读锁拿 `rule.url`, 释放后再发 HTTP。
//!
//! 注: 本模块定义的 `SourceInfo` 是 web 层 DTO（仅 4 字段: id/name/url/enabled）,
//! 与 `models::SourceInfo`（10 字段, 含 health / `delay_ms` / `http_status）不同`。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use crate::core::sources as core_sources;
use crate::utils::lock::{rw_read_or, rw_write_or};
use crate::web::SharedState;
use crate::web::error::read_state_or_json;

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
/// - `(INTERNAL_SERVER_ERROR, ...)` — `state.rules` 锁被毒化
#[tracing::instrument(name = "web::sources_list", skip_all)]
pub async fn sources_list(
    State(state): State<SharedState>,
) -> Result<Json<Vec<SourceInfo>>, (StatusCode, String)> {
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
/// - `(NOT_FOUND, ...)` — 书源 id 不存在
/// - `(INTERNAL_SERVER_ERROR, ...)` — 锁被毒化
#[tracing::instrument(name = "web::source_toggle", skip_all, fields(source_id = id))]
pub async fn source_toggle(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<i32>,
) -> Result<Json<SourceInfo>, (StatusCode, String)> {
    // 1. 先从 rules 中取到目标书源的 URL（短锁，取完即放）。
    let url = {
        let rules = read_state_or_json("source_toggle:read_url", || {
            rw_read_or("source_toggle:read_url", &state.rules)
        })?;
        core_sources::find_rule_by_id(&rules, id)
            .map(|r| r.url.clone())
            .ok_or_else(|| (StatusCode::NOT_FOUND, "书源未找到".to_string()))?
    };

    // 2. 切换 SourcesConfig 并持久化到磁盘。
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

    // 3. 同步更新内存中的 Rule.disabled。
    read_state_or_json("source_toggle:write_rules", || {
        let mut rules = rw_write_or("source_toggle:write_rules", &state.rules)?;
        // 借用 disjoint: find(&) 拿不可变借用只在条件判断作用域内；
        // 立刻 drop，再 iter_mut 拿可变借用更新 disabled。
        if core_sources::find_rule_by_id(&rules, id).is_some() {
            if let Some(r) = rules.iter_mut().find(|r| r.id == id) {
                r.disabled = now_disabled;
            }
        }
        Ok(())
    })?;

    // 4. 返回更新后的信息。
    let rules = read_state_or_json("source_toggle:read_back", || {
        rw_read_or("source_toggle:read_back", &state.rules)
    })?;
    let r = core_sources::find_rule_by_id(&rules, id)
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

/// `POST /api/sources/{id}/test` 响应体。
#[derive(serde::Serialize)]
pub struct SourceTestResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// `POST /api/sources/{id}/test` — 用 GET 探活, 10s 超时。
///
/// 书源未找到 → 返回 `ok: false` 而非 404, 让前端统一按 JSON 渲染。
///
/// # Errors
///
/// - `(INTERNAL_SERVER_ERROR, ...)` — `state.rules` 锁被毒化
#[tracing::instrument(name = "web::source_test", skip_all, fields(source_id = id))]
pub async fn source_test(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<i32>,
) -> Result<Json<SourceTestResult>, (StatusCode, String)> {
    let rule = {
        let rules = read_state_or_json("source_test", || rw_read_or("source_test", &state.rules))?;
        match core_sources::find_rule_by_id_cloned(&rules, id) {
            Some(r) => r,
            None => {
                return Ok(Json(SourceTestResult {
                    ok: false,
                    latency_ms: 0,
                    error: Some("书源未找到".into()),
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
