//! 健康检查端点。
//!
//! 返回版本 + rules 数量 + 活跃任务数，只证明「进程跑着 + 核心状态可访问」。
//! **不**做深度检查（不发 HTTP 探书源）—— 那是 `/api/sources/{id}/test` 的事。
//! 供 Docker HEALTHCHECK / K8s 探针 / 监控抓点使用。

use axum::Json;
use axum::extract::State;

use crate::web::SharedState;

/// `GET /api/health` 响应体。
#[derive(serde::Serialize)]
pub struct HealthInfo {
    /// 字面量 `"ok"`（未来可扩展为 `"degraded"` 等状态机）。
    pub status: &'static str,
    /// `Cargo.toml` 的 `version`，编译期嵌入。
    pub version: &'static str,
    /// 当前内存中的书源数量（含禁用）。
    pub rules_count: usize,
    /// 未结束的任务数（不含已完成 / 失败 / 已取消）。
    pub active_tasks: usize,
}

/// `GET /api/health` handler。
///
/// 锁失败 → `0` 并记 warn：监控不应因锁抖动而误报。
#[tracing::instrument(name = "web::health", skip_all)]
pub async fn health(State(state): State<SharedState>) -> Json<HealthInfo> {
    let rules_count = state.rules.read().map_or_else(
        |e| {
            tracing::warn!("health: rules RwLock poisoned: {e}");
            0
        },
        |r| r.len(),
    );
    let active_tasks = state.tasks.lock().map_or_else(
        |e| {
            tracing::warn!("health: tasks Mutex poisoned: {e}");
            0
        },
        |t| t.iter().filter(|t| t.finished.is_none()).count(),
    );

    Json(HealthInfo {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        rules_count,
        active_tasks,
    })
}
