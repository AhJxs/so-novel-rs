//! Web API integration tests.
//!
//! 构造一个 `SharedState` → 调用 `routes::build_router` → 用
//! `tower::ServiceExt::oneshot` 跑 axum 请求，验证响应。不发任何网络请求，
//! 全部跑在内存里 + tempfile 做 tasks.json / `sources_config.json` 的隔离。
//!
//! 关注点：
//! - 不依赖外网书源状态 —— rule 用 `make_rule()` 构造（id/name/url/disabled）
//! - `session_store` 走 `SessionStore::<SessionNullPool>` 默认内存实现
//! - 失败原因（lock poison / 业务 4xx）作为已知信号验证，**不**与"返回 200"混同

#![cfg(feature = "web")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines
)]

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::config::AppConfig;
use crate::db::SourcesConfig;
use crate::http::HttpClients;
use crate::models::Rule;
use crate::web::{SharedState, WebInitParams, WebState};

use super::routes;

/// 构造最小可路由 SharedState：默认配置 + 空 rules + 空 tasks。
fn build_test_state_with(dir: &std::path::Path) -> SharedState {
    let cfg = AppConfig::default();
    let http = HttpClients::new(&cfg).expect("HttpClients::new ok with default cfg");
    let rules = Vec::<Rule>::new();
    let params = WebInitParams {
        sources_config: SourcesConfig::default(),
        sources_config_path: dir.join("sources_config.json"),
        tasks: Vec::new(),
        tasks_file: dir.join("tasks.json"),
        next_task_id: 1,
    };
    Arc::new(WebState::new(cfg, http.into(), rules, params))
}

/// 构造带初始 rules 的 `SharedState`。
fn build_test_state_with_rules(dir: &std::path::Path, rules: Vec<Rule>) -> SharedState {
    let cfg = AppConfig::default();
    let http = HttpClients::new(&cfg).expect("HttpClients::new ok");
    let params = WebInitParams {
        sources_config: SourcesConfig::default(),
        sources_config_path: dir.join("sources_config.json"),
        tasks: Vec::new(),
        tasks_file: dir.join("tasks.json"),
        next_task_id: 1,
    };
    Arc::new(WebState::new(cfg, http.into(), rules, params))
}

/// `Rule` 没实现 `Default`，且 handler 不读 sub-Option 字段（搜索/解析都靠
/// `search`/`book`/`toc`/`chapter`/`crawl` 任意一个非 None 才会走）—— 测试里
/// 只要"存在且 disabled/id/name/url 正确"，其余字段留 None + 空字符串即可。
fn make_rule(id: i32, name: &str, url: &str, disabled: bool) -> Rule {
    Rule {
        id,
        url: url.to_string(),
        name: name.to_string(),
        comment: String::new(),
        language: String::new(),
        need_proxy: false,
        disabled,
        ignore_ssl: false,
        search: None,
        book: None,
        toc: None,
        chapter: None,
        crawl: None,
    }
}

async fn build_test_router(state: SharedState) -> axum::Router {
    use axum_session::{SessionConfig, SessionNullPool, SessionStore};
    let session_store = SessionStore::<SessionNullPool>::new(None, SessionConfig::default())
        .await
        .expect("SessionStore::new ok");
    routes::build_router(state, session_store)
}

/// `tower::ServiceExt::oneshot` 的薄壳：构建 Request → 调 router → 拿到 Response。
async fn dispatch(router: axum::Router, req: Request<Body>) -> axum::response::Response {
    router.oneshot(req).await.expect("router serves")
}

async fn read_json(resp: axum::response::Response) -> serde_json::Value {
    let body = to_bytes(resp.into_body(), 64 * 1024).await.unwrap();
    serde_json::from_slice(&body).unwrap_or_else(|e| {
        panic!(
            "response body is not JSON: {e}; raw = {}",
            String::from_utf8_lossy(&body)
        )
    })
}

// ── /api/health ──────────────────────────────────────────────────────────

#[tokio::test]
async fn health_returns_ok_and_zero_counts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    let resp = dispatch(
        app,
        Request::builder()
            .uri("/api/health")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let json = read_json(resp).await;
    assert_eq!(json["status"], "ok");
    assert_eq!(json["rules_count"], 0);
    assert_eq!(json["active_tasks"], 0);
    assert!(json["version"].as_str().is_some());
}

// ── /api/sources ─────────────────────────────────────────────────────────

#[tokio::test]
async fn sources_list_returns_rules_with_enabled_flag() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let rules = vec![
        make_rule(1, "A", "https://example.com/a", false),
        make_rule(2, "B", "https://example.com/b", true),
    ];
    let state = build_test_state_with_rules(tmp.path(), rules);
    let app = build_test_router(state).await;

    let resp = dispatch(
        app,
        Request::builder()
            .uri("/api/sources")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let json = read_json(resp).await;
    let arr = json.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["id"], 1);
    assert_eq!(arr[0]["enabled"], true);
    assert_eq!(arr[1]["id"], 2);
    assert_eq!(arr[1]["enabled"], false);
}

#[tokio::test]
async fn sources_list_empty_when_no_rules() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;
    let resp = dispatch(
        app,
        Request::builder()
            .uri("/api/sources")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = read_json(resp).await;
    assert_eq!(json.as_array().map(std::vec::Vec::len), Some(0));
}

// ── /api/sources/{id}/toggle ─────────────────────────────────────────────

#[tokio::test]
async fn source_toggle_flips_disabled_and_persists() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let rules = vec![make_rule(42, "X", "https://example.com/x", false)];
    let state = build_test_state_with_rules(dir, rules);

    // 第一下：false → true（禁用）
    let app = build_test_router(Arc::clone(&state)).await;
    let resp = dispatch(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/sources/42/toggle")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = read_json(resp).await;
    assert_eq!(json["enabled"], false);

    // sources_config.json 应该已经持久化（toggle_disabled 内部会 save）。
    assert!(dir.join("sources_config.json").exists());

    // 第二下：true → false（重新启用）
    let app = build_test_router(state).await;
    let resp = dispatch(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/sources/42/toggle")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = read_json(resp).await;
    assert_eq!(json["enabled"], true);
}

#[tokio::test]
async fn source_toggle_404_on_unknown_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    let resp = dispatch(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/sources/9999/toggle")
            .body(Body::empty())
            .unwrap(),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── /api/settings ────────────────────────────────────────────────────────

#[tokio::test]
async fn settings_put_rejects_empty_download_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    let body = serde_json::json!({ "download_path": "" });
    let resp = dispatch(
        app,
        Request::builder()
            .method("PUT")
            .uri("/api/settings")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn settings_put_rejects_non_existing_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    let body = serde_json::json!({
        "download_path": "Z:/__so_novel_unlikely_path_does_not_exist_123__/__"
    });
    let resp = dispatch(
        app,
        Request::builder()
            .method("PUT")
            .uri("/api/settings")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn settings_get_round_trips() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    let resp = dispatch(
        app,
        Request::builder()
            .uri("/api/settings")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = read_json(resp).await;
    assert!(json["download_path"].as_str().is_some());
}

// ── /api/tasks ──────────────────────────────────────────────────────────

#[tokio::test]
async fn tasks_list_empty_initially() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    let resp = dispatch(
        app,
        Request::builder()
            .uri("/api/tasks")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = read_json(resp).await;
    assert_eq!(json.as_array().map(std::vec::Vec::len), Some(0));
}

#[tokio::test]
async fn task_delete_404_on_unknown_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    let resp = dispatch(
        app,
        Request::builder()
            .method("DELETE")
            .uri("/api/tasks/777")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn task_cancel_404_on_unknown_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    let resp = dispatch(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/tasks/777/cancel")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── /api/download (JSON task_id) ───────────────────────────────────────

#[tokio::test]
async fn download_returns_task_id_json_and_pushes_task() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path();
    let rules = vec![make_rule(7, "src", "https://example.com", false)];
    let state = build_test_state_with_rules(dir, rules);
    let app = build_test_router(Arc::clone(&state)).await;

    let body = serde_json::json!({ "url": "https://example.com/book/1", "source_id": 7 });
    let resp = dispatch(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/download")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = read_json(resp).await;
    let task_id = json["task_id"].as_u64().expect("task_id present");

    // 任务已入 state.tasks：GET /api/tasks 应能看到该 id
    let app2 = build_test_router(state).await;
    let resp = dispatch(
        app2,
        Request::builder()
            .uri("/api/tasks")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    let arr = read_json(resp).await;
    let ids: Vec<u64> = arr
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|t| t["id"].as_u64())
        .collect();
    assert!(ids.contains(&task_id), "task {task_id} should be listed, got {ids:?}");
}

// ── /api/library ─────────────────────────────────────────────────────────

// ── /api/search (任务轮询) ────────────────────────────────────────────

#[tokio::test]
async fn search_create_returns_task_id_then_done_with_zero_sources() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path()); // 空 rules
    let app = build_test_router(state).await;

    let body = serde_json::json!({ "keyword": "三体" });
    // dispatch 按值消费 router，后续轮询都传 app.clone()
    let resp = dispatch(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/search")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let json = read_json(resp).await;
    let task_id = json["task_id"].as_u64().expect("task_id present");

    // 空 rules → 0 源 → 后台 spawn 很快标 done。轮询几次等 done。
    for _ in 0..50 {
        let resp = dispatch(
            app.clone(),
            Request::builder()
                .uri(format!("/api/search/{task_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        let st = read_json(resp).await;
        assert_eq!(st["total_sources"], 0);
        if st["status"] == "Done" {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("search task did not reach Done");
}

#[tokio::test]
async fn search_create_rejects_empty_keyword() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    let body = serde_json::json!({ "keyword": "   " });
    let resp = dispatch(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/search")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn search_status_404_on_unknown_task() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;
    let resp = dispatch(
        app,
        Request::builder()
            .uri("/api/search/99999")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn search_delete_is_idempotent_204() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;

    // 先建一个任务
    let body = serde_json::json!({ "keyword": "test" });
    let resp = dispatch(
        app.clone(),
        Request::builder()
            .method("POST")
            .uri("/api/search")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap(),
    )
    .await;
    let task_id = read_json(resp).await["task_id"].as_u64().expect("task_id");

    // 删除 → 204
    let resp = dispatch(
        app.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/search/{task_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 再删同一 id → 仍 204（幂等）
    let resp = dispatch(
        app.clone(),
        Request::builder()
            .method("DELETE")
            .uri(format!("/api/search/{task_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 删后 GET → 404
    let resp = dispatch(
        app,
        Request::builder()
            .uri(format!("/api/search/{task_id}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn library_list_handles_missing_dir_gracefully() {
    // 默认 download_path 指向用户家目录下的 .sonovel —— 测试机不一定存在。
    // 关键诉求：路由能命中、不 panic；返回 200 + 数组就是合格。
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = build_test_state_with(tmp.path());
    let app = build_test_router(state).await;
    let resp = dispatch(
        app,
        Request::builder()
            .uri("/api/library")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let json = read_json(resp).await;
    assert!(json.is_array());
}
