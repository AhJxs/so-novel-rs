//! 健康检查端点。
//!
//! ## 设计
//!
//! 返回 server 版本 + rules 数量 + 当前任务数。**不**做深度检查
//! （不发 HTTP 请求验证书源）—— 那是 `/api/sources/{id}/test` 的事。
//! `/api/health` 只证明"进程跑着 + 内存里的核心状态可访问"。
//!
//! ## 用途
//!
//! - Docker `HEALTHCHECK`（已有 `wget --spider`, 但 JSON 端点能区分 alive / ready）
//! - K8s `readinessProbe` / `livenessProbe`
//! - 监控抓点（无锁读规则数 + 任务数指标）

use axum::Json;
use axum::extract::State;

use crate::web::SharedState;

/// `GET /api/health` 响应体。
#[derive(serde::Serialize)]
pub struct HealthInfo {
    /// 字面量 `"ok"`；未来可扩展为 `"degraded"` 之类状态机。
    pub status: &'static str,
    /// `Cargo.toml` 的 `version` 字段, 编译期嵌入。
    pub version: &'static str,
    /// 当前内存中的书源数量（含禁用）。
    pub rules_count: usize,
    /// 未结束的任务数（下载中 + 排队中, 不含已完成 / 失败 / 已取消）。
    pub active_tasks: usize,
}

/// `GET /api/health` handler.
///
/// 锁失败 → `0`, 记 warn; 监控不应因为锁抖动而误报。
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
