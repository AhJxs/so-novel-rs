# Web 前端迁移 shadcn + 全面重设计 + 后端 API 轮询化 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 web-ui 从 HeroUI 迁移到 shadcn（Nova），侧边栏重设计 6 页，并把搜索/下载从 SSE 流改为任务轮询（后端 `src/web/` + 前端数据层同步改）。

**Architecture:** 后端先行双阶段。Phase A（M1–M2）：`src/web/` 去掉 SSE，搜索改「`POST /api/search` 建任务 → `GET /api/search/{id}` 轮询」，下载 `POST /api/download` 直接返 `{task_id}`；前端仅改数据层（api.ts/types/hooks/context），UI 仍 HeroUI 可运行。Phase B（M3–M5）：shadcn Nova 初始化 + 侧边栏骨架 + 六页视觉重做 + 移除 HeroUI 依赖。

**Tech Stack:** Rust (axum/tokio/serde) · React 19 + TypeScript · Vite 8 + Tailwind v4 · shadcn/ui (Nova) + lucide-react · react-query · react-router v7 · i18next（zh-CN/zh-TW/en）· next-themes · sonner

**Spec:** [docs/superpowers/specs/2026-09-08-web-shadcn-redesign-design.md](../specs/2026-09-08-web-shadcn-redesign-design.md) — 本计划的所有设计依据都在 spec 里，执行者先读 spec 再读本计划。

## Global Constraints

- Rust 1.95+；`cargo build --features web` 与 `cargo test`（web feature 下）必须全绿。
- 错误响应统一走 `WebError` JSON envelope `{ error: { code, code_id, message } }`，`message` 按请求 locale 翻译；**不泄漏内部 cause**。
- 前端 `npm run build`（= `tsc --noEmit && vite build`）与 `npm run lint`（oxlint）必须全绿。
- i18n 三语（zh-CN/zh-TW/en）保留，key 复用现状；仅新增必需的新 key。
- react-query queryKey 保持不变：`['tasks']` / `['library']` / `['sources']` / `['settings']`。
- shadcn Nova 视觉 + 语义 token（`text-muted-foreground` 等），不用 HeroUI class（`text-default-500` 等）。
- 状态色（blue/green/red 等数据语义色）可用 tailwind 原色 class（非主题 token，豁免语义 token 规则）。
- 搜索轮询间隔固定 800ms；搜索任务 TTL 10 分钟。
- 桌面 GPUI（`src/desktop/`）、CLI、`bundle/rules/`、`core/` 爬虫一律不碰。

---

## M1 — Phase A 后端（Rust）

### Task 1: WebState 增加搜索任务类型与存储

**Files:**
- Modify: `src/web/mod.rs`

**Interfaces:**
- Consumes: 无（纯新增）
- Produces:
  - `crate::web::SearchStatus` enum（`Running | Done`，derive `Clone, Copy, PartialEq, Eq, Serialize`）
  - `crate::web::SourceSearchError` struct（`source_id: i32, source_name: String, error: String`，derive `Clone, Serialize`）
  - `crate::web::SearchTask` struct（`id: u64, keyword: String, created_at_unix: u64, status: SearchStatus, sources_total: usize, sources_done: usize, results: Vec<crate::models::SearchResult>, source_errors: Vec<SourceSearchError>`）
  - `WebState` 新增字段 `search_tasks: Mutex<HashMap<u64, SearchTask>>`、`next_search_id: Mutex<u64>`

- [ ] **Step 1: 加 import 与类型**

在 `src/web/mod.rs` 顶部加 `use std::collections::HashMap;`（已有 `use std::sync::{Arc, Mutex, RwLock};`）。

在 `TaskStatus` enum 之后（约 124 行）追加：

```rust
/// 搜索任务状态（API 返回用）。搜索是内存态，进程重启即失。
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SearchStatus {
    Running,
    Done,
}

/// 单源搜索失败信息（`GET /api/search/{id}` 的 `source_errors` 项）。
#[derive(Clone, Serialize)]
pub struct SourceSearchError {
    pub source_id: i32,
    pub source_name: String,
    /// 按请求 locale 翻译的错误文案（不泄漏内部 cause）。
    pub error: String,
}

/// 内存态搜索任务（**不持久化**）。
pub struct SearchTask {
    pub id: u64,
    pub keyword: String,
    pub created_at_unix: u64,
    pub status: SearchStatus,
    pub sources_total: usize,
    pub sources_done: usize,
    pub results: Vec<crate::models::SearchResult>,
    pub source_errors: Vec<SourceSearchError>,
}
```

- [ ] **Step 2: WebState 加字段 + 初始化**

`WebState` struct 里 `next_task_id: Mutex<u64>,` 之后加：

```rust
    /// 内存态搜索任务注册表（搜索是瞬态，不落盘）。
    pub search_tasks: Mutex<HashMap<u64, SearchTask>>,
    /// 搜索任务 id 计数器（与 `next_task_id` 独立）。
    pub next_search_id: Mutex<u64>,
```

`WebState::new` 里 `next_task_id: Mutex::new(params.next_task_id),` 之后加：

```rust
            search_tasks: Mutex::new(HashMap::new()),
            next_search_id: Mutex::new(1),
```

- [ ] **Step 3: 编译验证**

Run: `cargo check --features web`
Expected: PASS（无错误；若报 unused import 先不管，M1 末尾统一清）

- [ ] **Step 4: 提交**

```bash
git add src/web/mod.rs
git commit -m "feat(web): WebState 增加内存态搜索任务存储与类型"
```

---

### Task 2: `POST /api/search` 端点 + 集成测试

**Files:**
- Create: （无新文件）
- Modify: `src/web/handlers/search.rs`（整体重写）、`src/web/handlers/mod.rs`（无需改，search 已注册）、`src/web/routes.rs`（加 POST 路由）
- Test: `src/web/tests.rs`

**Interfaces:**
- Consumes: `crate::web::SearchTask` / `SearchStatus`（Task 1）；`crate::crawler::search::search_streaming(http, sources, keyword, limit, cf_bypass, tx)`；`core::search::select_sources(&rules, &config, source_id)`；`config_helpers::cf_bypass(&config)`；`ts_for_locale(locale, key)`；`read_state_or_json(label, || ...)?`；`mutex_or` / `rw_read_or`
- Produces:
  - `pub async fn search_create(Locale, State<SharedState>, Json<SearchCreateRequest>) -> Result<Json<SearchCreateResponse>, WebError>`
  - `SearchCreateRequest { keyword: String, source_id: Option<i32>, limit: Option<i32> }`（Deserialize）
  - `SearchCreateResponse { task_id: u64 }`（Serialize）

- [ ] **Step 1: 写失败的集成测试**

在 `src/web/tests.rs` 末尾追加：

```rust
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --features web search_create_ -- --nocapture`
Expected: FAIL（路由 404，`assert_eq!(resp.status(), StatusCode::CREATED)` 失败）—— 测试先行。

- [ ] **Step 3: 重写 search.rs**

整文件替换为（保留头部模块注释，说明改为轮询模型）：

```rust
//! 搜索 API（任务轮询模型）。
//!
//! `POST /api/search` 创建内存态搜索任务并立即返回 `task_id`；
//! `GET /api/search/{task_id}` 轮询当前累计状态（Task 3）；`DELETE` 显式清理（Task 4）。
//! 替代旧 SSE 流式实现。crawler 复用 `search_streaming` 的 mpsc 通道，仅消费端改为累计进 task。

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::core::{config_helpers, search as core_search};
use crate::i18n::ts_for_locale;
use crate::web::error::WebError;
use crate::web::error::read_state_or_json;
use crate::web::error_code::ErrorCode;
use crate::web::locale::Locale;
use crate::web::{SearchStatus, SharedState, SourceSearchError};
use crate::utils::lock::{mutex_or, rw_read_or};
use crate::utils::time::now_unix_secs;

/// 搜索任务 TTL（秒）：超时后 `POST /api/search` 会 sweep 掉。
const SEARCH_TTL_SECS: u64 = 600;

/// `POST /api/search` 请求体。
#[derive(Deserialize)]
pub struct SearchCreateRequest {
    pub keyword: String,
    pub source_id: Option<i32>,
    pub limit: Option<i32>,
}

/// `POST /api/search` 响应体。
#[derive(Serialize)]
pub struct SearchCreateResponse {
    pub task_id: u64,
}

/// 把 [`crate::parser::SearchError`] 映射到稳定的 [`ErrorCode`]（沿用旧实现）。
const fn search_err_code(e: &crate::parser::SearchError) -> ErrorCode {
    use crate::parser::SearchError;
    use ErrorCode as C;
    match e {
        SearchError::SearchDisabled => C::SearchDisabled,
        SearchError::SourceDisabled => C::SourceDisabled,
        SearchError::Http(_) => C::SearchHttp,
        SearchError::Cloudflare(_) => C::SearchCloudflare,
        SearchError::Parse(_) | SearchError::Selector(_) => C::SearchParse,
    }
}

/// `POST /api/search` — 创建搜索任务。
///
/// 校验 keyword → sweep 过期任务 → mint id → 插入 `state.search_tasks` →
/// spawn crawler（`search_streaming` 持有 tx，消费端循环累计进 task）→ 立即返回 `task_id`。
pub async fn search_create(
    Locale(locale): Locale,
    State(state): State<SharedState>,
    Json(req): Json<SearchCreateRequest>,
) -> Result<Json<SearchCreateResponse>, WebError> {
    let keyword = req.keyword.trim().to_string();
    if keyword.is_empty() {
        return Err(WebError::BadRequest("search_keyword_empty"));
    }
    let config = read_state_or_json("search:cfg", || {
        Ok(rw_read_or("search:cfg", &state.config)?.clone())
    })?;
    let rules = read_state_or_json("search:rules", || {
        Ok(rw_read_or("search:rules", &state.rules)?.clone())
    })?;
    let http = Arc::clone(&state.http);

    let sources = core_search::select_sources(&rules, &config, req.source_id);
    let limit = req.limit.map(|v| v.max(0) as usize).filter(|v| *v > 0);
    let cf_bypass = config_helpers::cf_bypass(&config);

    // sweep 过期 + mint id + 插入
    let task_id = {
        let mut tasks = mutex_or("search:sweep", &state.search_tasks)?;
        let now = now_unix_secs();
        tasks.retain(|_, t| now.saturating_sub(t.created_at_unix) < SEARCH_TTL_SECS);
        let mut next = mutex_or("search:next_id", &state.next_search_id)?;
        let id = *next;
        *next += 1;
        drop(next);
        tasks.insert(
            id,
            crate::web::SearchTask {
                id,
                keyword: keyword.clone(),
                created_at_unix: now,
                status: SearchStatus::Running,
                sources_total: sources.len(),
                sources_done: 0,
                results: Vec::new(),
                source_errors: Vec::new(),
            },
        );
        id
    };

    // spawn：crawler 独立子任务持有 tx；外层循环消费 rx 累计进 task。
    let (tx, rx) = mpsc::unbounded_channel::<crate::crawler::search::SourceSearchOutcome>();
    let http_for_crawler = Arc::clone(&http);
    let state_for_spawn = Arc::clone(&state);
    tokio::spawn(async move {
        tokio::spawn(async move {
            crate::crawler::search::search_streaming(
                http_for_crawler,
                sources,
                keyword,
                limit,
                cf_bypass,
                tx,
            )
            .await;
        });
        let mut rx = rx;
        while let Some(outcome) = rx.recv().await {
            let err_msg = match &outcome.result {
                Ok(_) => None,
                Err(e) => Some(ts_for_locale(&locale, search_err_code(e).key())),
            };
            if let Ok(mut tasks) = state_for_spawn.search_tasks.lock() {
                if let Some(task) = tasks.get_mut(&task_id) {
                    task.sources_done += 1;
                    if let Ok(list) = &outcome.result {
                        task.results.extend(list.iter().cloned());
                    }
                    if let Some(msg) = err_msg {
                        task.source_errors.push(SourceSearchError {
                            source_id: outcome.source_id,
                            source_name: outcome.source_name.clone(),
                            error: msg,
                        });
                    }
                    if task.sources_done >= task.sources_total {
                        task.status = SearchStatus::Done;
                    }
                }
            }
        }
        // crawler 退出（tx drop）→ rx 关闭 → 0 源场景兜底标 done。
        if let Ok(mut tasks) = state_for_spawn.search_tasks.lock() {
            if let Some(task) = tasks.get_mut(&task_id)
                && task.status == SearchStatus::Running
                && task.sources_total == 0
            {
                task.status = SearchStatus::Done;
            }
        }
    });

    Ok(Json(SearchCreateResponse { task_id }))
}
```

注：`ts_for_locale(&locale, ...)` 若签名是 `fn ts_for_locale(locale: &str, key: &str)` 则把 `&locale` 传 `locale`（`Locale` extractor 解构出 String，spawn 里 move）。以实际编译为准，`.key()` 返回 `&'static str`。

- [ ] **Step 4: routes.rs 注册 POST**

在 `src/web/routes.rs` 搜索区，把：

```rust
        .route("/search", get(handlers::search::search))
```

替换为：

```rust
        .route("/search", post(handlers::search::search_create))
```

（`get` import 可能仍被其它路由使用，保留 import。）

- [ ] **Step 5: 运行确认通过**

Run: `cargo test --features web search_ -- --nocapture`
Expected: PASS（3 个新测试 + 旧搜索相关若有则更新）

- [ ] **Step 6: 提交**

```bash
git add src/web/handlers/search.rs src/web/routes.rs src/web/tests.rs
git commit -m "feat(web): 搜索改 POST /api/search 创建任务（SSE 移除，轮询模型）"
```

---

### Task 3: `GET /api/search/{task_id}` + `DELETE /api/search/{task_id}` 端点

**Files:**
- Modify: `src/web/handlers/search.rs`、`src/web/routes.rs`
- Test: `src/web/tests.rs`

**Interfaces:**
- Consumes: `SearchStatus` / `SourceSearchError`（Task 1）；`SearchStatusResponse` 形状见下
- Produces:
  - `pub async fn search_status(State<SharedState>, Path<u64>) -> Result<Json<SearchStatusResponse>, WebError>`
  - `pub async fn search_delete(State<SharedState>, Path<u64>) -> Result<StatusCode, WebError>`
  - `SearchStatusResponse { status: SearchStatus, total_sources: usize, done_sources: usize, results: Vec<SearchResult>, source_errors: Vec<SourceSearchError> }`（Serialize）
  - `GET /api/search/{task_id}` 路由 + `DELETE /api/search/{task_id}` 路由

- [ ] **Step 1: 写失败的集成测试**

在 `src/web/tests.rs` 追加：

```rust
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --features web search_delete_ -- --nocapture`
Expected: FAIL（路由不存在，404 而非 204）。

- [ ] **Step 3: 实现两个 handler**

在 `src/web/handlers/search.rs` 追加 import：`use axum::extract::{Path, State};`、`use axum::http::StatusCode;`、`use crate::models::SearchResult;`，然后追加：

```rust
/// `GET /api/search/{task_id}` 响应体（每轮轮询返回当前累计）。
#[derive(Serialize)]
pub struct SearchStatusResponse {
    pub status: SearchStatus,
    pub total_sources: usize,
    pub done_sources: usize,
    pub results: Vec<SearchResult>,
    pub source_errors: Vec<SourceSearchError>,
}

/// `GET /api/search/{task_id}` — 轮询当前累计状态。
pub async fn search_status(
    State(state): State<SharedState>,
    Path(task_id): Path<u64>,
) -> Result<Json<SearchStatusResponse>, WebError> {
    let tasks =
        read_state_or_json("search:status", || mutex_or("search:status", &state.search_tasks))?;
    let Some(task) = tasks.get(&task_id) else {
        return Err(WebError::NotFound("search_task"));
    };
    Ok(Json(SearchStatusResponse {
        status: task.status,
        total_sources: task.sources_total,
        done_sources: task.sources_done,
        results: task.results.clone(),
        source_errors: task.source_errors.clone(),
    }))
}

/// `DELETE /api/search/{task_id}` — 丢弃搜索任务（幂等，不存在也 204）。
pub async fn search_delete(
    State(state): State<SharedState>,
    Path(task_id): Path<u64>,
) -> Result<StatusCode, WebError> {
    mutex_or("search:delete", &state.search_tasks)?.remove(&task_id);
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 4: routes.rs 注册两个路由**

在 `src/web/routes.rs` 的 `.route("/search", post(...))` 之后加：

```rust
        .route("/search/{task_id}", get(handlers::search::search_status))
        .route("/search/{task_id}", delete(handlers::search::search_delete))
```

（`delete` import 已有，用于 tasks 删除。）

- [ ] **Step 5: 运行确认通过**

Run: `cargo test --features web search_ -- --nocapture`
Expected: PASS（含 Task 2 的 3 个测试 + 本 Task 的 1 个）

- [ ] **Step 6: 提交**

```bash
git add src/web/handlers/search.rs src/web/routes.rs src/web/tests.rs
git commit -m "feat(web): 搜索任务轮询 GET/DELETE /api/search/{id} 端点"
```

---

### Task 4: `POST /api/download` 改返回 JSON `task_id`

**Files:**
- Modify: `src/web/handlers/download.rs`、`src/web/handlers/tasks.rs`
- Test: `src/web/tests.rs`

**Interfaces:**
- Consumes: `spawn_task_drain`（本 Task 改签名）；`DownloadOptions`；`crawler::Progress`
- Produces:
  - `pub async fn download(...) -> Result<Json<DownloadResponse>, WebError>`（不再返回 `Sse`）
  - `DownloadResponse { task_id: u64 }`（Serialize）
  - `pub(super) fn spawn_task_drain(state: Arc<WebState>, task_id: u64, crawler_rx: mpsc::UnboundedReceiver<Progress>)`（去掉 `sse_tx` 参数与 broadcast 转发）

- [ ] **Step 1: 写失败的集成测试**

在 `src/web/tests.rs` 追加：

```rust
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
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test --features web download_returns_ -- --nocapture`
Expected: FAIL（当前返回 SSE 流，`read_json` panic "not JSON"）。

- [ ] **Step 3: 改 tasks.rs drain 签名**

`src/web/handlers/tasks.rs`：

- 删 `use tokio::sync::{broadcast, mpsc};` 里的 `broadcast`（保留 `mpsc`）。
- `spawn_task_drain` 签名去掉 `sse_tx: broadcast::Sender<Progress>` 参数。
- 函数体内删掉 `let _ = sse_tx.send(progress);` 与 `progress.clone()`（`task.apply_progress(progress);` 不再需要 clone）。
- 更新函数头注释：去掉「3. broadcast producer」一条，改为「状态更新者 + 持久化兜底」。

- [ ] **Step 4: 改 download.rs**

`src/web/handlers/download.rs`：

1. 顶部 import：删 `axum::response::Sse`、`futures::stream::Stream`、`tokio::sync::broadcast`、`super::super::error::read_state_or_sse`、`crate::web::locale::Locale`（若不再用）、`BoxedSseStream` 定义。
2. 删 `lock_failure_stream` 函数（整段）。
3. 加：

```rust
/// `POST /api/download` 响应体（JSON，替代 SSE 进度流）。
#[derive(Serialize)]
pub struct DownloadResponse {
    pub task_id: u64,
}
```

4. `download` 签名改为：

```rust
pub async fn download(
    Locale(locale): Locale,
    State(state): State<SharedState>,
    Json(req): Json<DownloadRequest>,
) -> Result<Json<DownloadResponse>, WebError> {
```

5. 三处 `read_state_or_sse(...)` 改为 `read_state_or_json(...)?`：
   - `"download:cfg"`、`"download:rules"`、`"download:next_task_id"`、`"download:push_task"` 的 `match ... { Err(sse) => return sse }` 模式全部改为 `let ... = ...?;` 直通（`read_state_or_json` 返回 `Result<_, WebError>`）。
   - 规则查找的 `let Some(rule) = rule else { return ...; };` 改为 `let Some(rule) = rule else { return Err(WebError::NotFound("source")); };`
6. mint id 的 `read_state_or_json("download:next_task_id", || -> Result<u64, String> {...})?` 保留闭包内部（mint + drop guard），只是包一层 `?`。
7. push 任务块：`read_state_or_json("download:push_task", || -> Result<(), String> {...})?;`
8. 删 `let (sse_tx, _) = broadcast::channel::<Progress>(256);`
9. `spawn_task_drain(Arc::clone(&state), task_id, crawler_rx);`（去掉 sse_tx 参数）
10. crawler spawn：删 `let sse_tx_for_crawler = sse_tx.clone();`；resolve 失败分支里把 `let _ = sse_tx_for_crawler.send(Progress::Failed { reason });` 改为经 mpsc 走 drain：

```rust
                let reason = ts_for_locale(locale, code.key());
                let _ = opts.progress.send(Progress::Failed { reason });
```

（`opts` 在该 Err 分支内仍持有 `progress` 所有权，发送后提前 return，`download_chapters` 不会执行——语义正确：任务经 drain `apply_progress` 标 Failed，随后 mpsc 关闭时因 `finished.is_some()` 不会被 AppRestarted 覆盖。）

11. 删除函数尾部第 5 步的整个 SSE 流组装（`let mut sse_rx = sse_tx.subscribe();` 到 `Sse::new(Box::pin(stream))`），改为：

```rust
    Ok(Json(DownloadResponse { task_id }))
```

12. 保留 `ProgressEvent` struct？—— 不再使用，删除（连同 `Default` derive 用途检查）。

- [ ] **Step 5: 运行确认通过 + 修复编译**

Run: `cargo test --features web download_returns_ -- --nocapture`
Expected: 先修编译错（unused import / 未用字段），再 PASS。

Run: `cargo build --features web`
Expected: PASS

- [ ] **Step 6: 提交**

```bash
git add src/web/handlers/download.rs src/web/handlers/tasks.rs src/web/tests.rs
git commit -m "feat(web): 下载 POST /api/download 改返回 JSON task_id，进度走任务轮询"
```

---

### Task 5: 移除 SSE 基建 + M1 全量验证

**Files:**
- Modify: `src/web/error.rs`（删 `read_state_or_sse`）、`src/web/mod.rs`（清理 import/注释）、`src/web/handlers/tasks.rs`（清理注释）

**Interfaces:**
- Consumes: 无
- Produces: `src/web` 内不再有 `Sse` / `broadcast` / `read_state_or_sse` 残留

- [ ] **Step 1: 删 `read_state_or_sse`**

`src/web/error.rs`：删除 `read_state_or_sse` 函数整段 + `use axum::response::sse::Sse;` + `use futures::stream::Stream;` + `use std::convert::Infallible;`（若其它地方未用）。`read_state_or_json` 保留。

- [ ] **Step 2: 清理 mod.rs 注释与 import**

`src/web/mod.rs`：doc 注释里提到「SSE 推送」「SSE `broadcast_tx`」的地方改为「REST + 任务轮询」「任务状态更新 + 持久化」。删除不再使用的 import（`cargo build` 会提示）。

- [ ] **Step 3: 清理 tasks.rs 注释**

`src/web/handlers/tasks.rs` 顶部 doc 注释里「drain 收到的进度转发 SSE」相关文字更新为「drain 只更新 `state.tasks` + 持久化」。

- [ ] **Step 4: 全量验证**

Run: `cargo build --features web`
Expected: PASS（无 warning 报错）
Run: `cargo test --features web`
Expected: 全部 PASS（新旧 web 测试）

- [ ] **Step 5: 提交**

```bash
git add src/web
git commit -m "refactor(web): 移除 SSE 基建（read_state_or_sse / broadcast / 流类型）"
```

---

## M2 — Phase A 前端适配层

> 本阶段 UI 仍是 HeroUI，数据层切到新契约。**验收标准**：`npm run build` + `npm run lint` 全绿，手工搜索/下载行为与现状等价。

### Task 6: types.ts 更新

**Files:**
- Modify: `web-ui/src/lib/types.ts`

**Interfaces:**
- Consumes: 现有 `SearchResult`
- Produces: `SearchStatus`、`SearchSourceError`、`SearchStatusResponse`（Task 7 用）；删除 `SearchStreamEvent`/`SearchDoneEvent`/`DownloadProgressEvent`

- [ ] **Step 1: 删除三个 SSE 类型**

删除 `SearchStreamEvent`、`SearchDoneEvent`、`DownloadProgressEvent` 三个 interface 及其注释块。

- [ ] **Step 2: 新增轮询类型**

在 `SearchResult` 之后追加：

```ts
/** 搜索任务状态。对应后端 `web::SearchStatus`（Running/Done，PascalCase 序列化）。 */
export type SearchStatus = 'Running' | 'Done'

/** 单源搜索失败。对应后端 `web::SourceSearchError`。 */
export interface SearchSourceError {
  source_id: number
  source_name: string
  error: string
}

/** GET /api/search/{task_id} 轮询体。对应后端 `handlers::search::SearchStatusResponse`。 */
export interface SearchStatusResponse {
  status: SearchStatus
  total_sources: number
  done_sources: number
  results: SearchResult[]
  source_errors: SearchSourceError[]
}
```

- [ ] **Step 3: 编译验证**

Run: `cd web-ui && npx tsc --noEmit`
Expected: FAIL（api.ts 仍引用已删类型——预期，Task 7 一起修）。若报错只在 api.ts，符合预期，进入 Task 7。

- [ ] **Step 4: 提交**

```bash
git add web-ui/src/lib/types.ts
git commit -m "feat(web-ui): types 移除 SSE 事件类型，新增搜索轮询类型"
```

---

### Task 7: api.ts 重写搜索/下载

**Files:**
- Modify: `web-ui/src/lib/api.ts`

**Interfaces:**
- Consumes: `SearchStatusResponse` / `StartDownloadResult` / `DownloadOptions`（types.ts）；删除 `consumeSse` / `SseEvent`（sse.ts）
- Produces:
  - `createSearch(keyword: string, sourceId?: number): Promise<{ task_id: number }>`
  - `getSearchStatus(taskId: number): Promise<SearchStatusResponse>`
  - `deleteSearch(taskId: number): Promise<void>`
  - `startDownload(opts: DownloadOptions): Promise<StartDownloadResult>`
  - 删除 `searchBooks`、`SearchCallbacks`、`DownloadCallbacks`、`consumeSseStream`

- [ ] **Step 1: 删 SSE 相关函数**

删除：`searchBooks`、`SearchCallbacks`、`DownloadCallbacks`、`consumeSseStream`、`import { consumeSse, type SseEvent } from './sse'`。删 `startDownload` 的旧 SSE 实现。

- [ ] **Step 2: 新增搜索轮询函数**

在「── 搜索（SSE） ──」区替换为：

```ts
// ─── 搜索（任务轮询） ─────────────────────────────────────────
// POST   /api/search              → { task_id }
// GET    /api/search/{task_id}    → SearchStatusResponse（轮询当前累计）
// DELETE /api/search/{task_id}    → 204（幂等，清理）

export interface StartSearchResult {
  task_id: number
}

export function createSearch(keyword: string, sourceId?: number): Promise<StartSearchResult> {
  const body: Record<string, unknown> = { keyword }
  if (sourceId != null) body.source_id = sourceId
  return apiFetch<StartSearchResult>('/search', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}

export function getSearchStatus(taskId: number): Promise<SearchStatusResponse> {
  return apiFetch<SearchStatusResponse>(`/search/${taskId}`)
}

export function deleteSearch(taskId: number): Promise<void> {
  return apiFetch<void>(`/search/${taskId}`, { method: 'DELETE' })
}
```

- [ ] **Step 3: 重写 startDownload**

「── 下载（SSE 流） ──」区替换为：

```ts
// ─── 下载（JSON task_id，进度走任务轮询） ─────────────────────
// POST /api/download { url, source_id, ... } → { task_id }
// 进度由 GET /api/tasks 轮询（useTasks 已有 refetchInterval 机制），无 SSE。

export interface DownloadOptions {
  url: string
  sourceId: number
  format?: ExportFormat
  chapterStart?: number
  chapterEnd?: number
}

export async function startDownload(opts: DownloadOptions): Promise<StartDownloadResult> {
  const body: Record<string, unknown> = { url: opts.url, source_id: opts.sourceId }
  if (opts.format) body.format = opts.format
  if (opts.chapterStart != null) body.chapter_start = opts.chapterStart
  if (opts.chapterEnd != null) body.chapter_end = opts.chapterEnd
  return apiFetch<StartDownloadResult>('/download', {
    method: 'POST',
    body: JSON.stringify(body),
  })
}
```

- [ ] **Step 4: 编译验证**

Run: `cd web-ui && npx tsc --noEmit`
Expected: FAIL（search-context.tsx / use-download.ts 仍引用旧函数——预期，Task 9/10 一起修）。记录报错清单供下一步对照。

- [ ] **Step 5: 提交**

```bash
git add web-ui/src/lib/api.ts
git commit -m "feat(web-ui): api.ts 搜索/下载改任务轮询契约"
```

---

### Task 8: 删除 sse.ts + sse.test.ts

**Files:**
- Delete: `web-ui/src/lib/sse.ts`、`web-ui/src/lib/sse.test.ts`

- [ ] **Step 1: 确认无引用**

Run: `cd web-ui && npx oxlint src/lib` 与 grep 确认 `sse` 不再被 import（Task 7 已删唯一引用）。

- [ ] **Step 2: 删除文件**

```bash
rm web-ui/src/lib/sse.ts web-ui/src/lib/sse.test.ts
```

- [ ] **Step 3: 提交**

```bash
git add -u web-ui/src/lib
git commit -m "chore(web-ui): 删除 SSE 解析器及其测试（无消费者）"
```

---

### Task 9: SearchProvider 轮询化

**Files:**
- Modify: `web-ui/src/contexts/search-context.tsx`

**Interfaces:**
- Consumes: `createSearch` / `getSearchStatus` / `deleteSearch`（Task 7）
- Produces: `UseSearchReturn` **契约不变**：`{ results, isFetching, searched, sourceCount, error, search(keyword, sourceId?), reset }`（`search` 的 `signal?` 参数移除，调用方均未传）

- [ ] **Step 1: 整文件重写**

```tsx
// 搜索状态 Context —— 把 useSearch 的状态提到应用根，跨路由切换保留结果。
// 数据层：POST /api/search 建任务 → 每 800ms 轮询 GET /api/search/{id} 累计结果。

import { createContext, useCallback, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { createSearch, deleteSearch, getSearchStatus } from '@/lib/api'
import type { SearchResult } from '@/lib/types'

export interface UseSearchReturn {
  /** 已累加的搜索结果（所有源合并）。 */
  results: SearchResult[]
  /** 是否正在轮询中（search 的 promise 未结束）。 */
  isFetching: boolean
  /** 是否已发起过至少一次搜索（区分初始空态与未搜索态）。 */
  searched: boolean
  /** 已完成的源数量（含出错源，对应后端 done_sources）。 */
  sourceCount: number
  /** 流级或源级错误信息（取首个非空）。 */
  error: string | null
  /** 发起搜索；自动取消上一轮轮询并清理旧任务。 */
  search: (keyword: string, sourceId?: number) => Promise<void>
  /** 重置全部状态 + 取消轮询 + 清理服务端搜索任务。 */
  reset: () => void
}

// eslint-disable-next-line react-refresh/only-export-components
export const SearchContext = createContext<UseSearchReturn | null>(null)

const POLL_INTERVAL_MS = 800

/** sleep 且可被 signal 提前中断（abort 时立即 resolve，不抛错）。 */
function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) return resolve()
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort)
      resolve()
    }, ms)
    function onAbort() {
      clearTimeout(timer)
      resolve()
    }
    signal.addEventListener('abort', onAbort, { once: true })
  })
}

export function SearchProvider({ children }: { children: ReactNode }) {
  const [results, setResults] = useState<SearchResult[]>([])
  const [isFetching, setFetching] = useState(false)
  const [searched, setSearched] = useState(false)
  const [sourceCount, setSourceCount] = useState(0)
  const [error, setError] = useState<string | null>(null)
  const abortRef = useRef<AbortController | null>(null)
  const taskIdRef = useRef<number | null>(null)

  const cleanupSearch = useCallback(() => {
    if (abortRef.current) abortRef.current.abort()
    if (taskIdRef.current != null) {
      // fire-and-forget：服务端有 TTL 兜底，失败无所谓
      void deleteSearch(taskIdRef.current).catch(() => {})
      taskIdRef.current = null
    }
  }, [])

  const reset = useCallback(() => {
    cleanupSearch()
    setResults([])
    setFetching(false)
    setSearched(false)
    setSourceCount(0)
    setError(null)
  }, [cleanupSearch])

  const search = useCallback(
    async (keyword: string, sourceId?: number) => {
      cleanupSearch()
      const controller = new AbortController()
      abortRef.current = controller

      setResults([])
      setError(null)
      setSourceCount(0)
      setSearched(true)
      setFetching(true)

      try {
        const { task_id } = await createSearch(keyword, sourceId)
        taskIdRef.current = task_id
        for (;;) {
          const st = await getSearchStatus(task_id)
          setResults(st.results)
          setSourceCount(st.done_sources)
          const firstErr = st.source_errors[0]
          if (firstErr) setError((prev) => prev ?? firstErr.error)
          if (st.status === 'Done' || controller.signal.aborted) break
          await sleep(POLL_INTERVAL_MS, controller.signal)
        }
      } catch (e) {
        const err = e instanceof Error ? e : new Error(String(e))
        if (!controller.signal.aborted) setError(err.message)
      } finally {
        if (abortRef.current === controller) abortRef.current = null
        setFetching(false)
      }
    },
    [cleanupSearch],
  )

  return (
    <SearchContext.Provider value={{ results, isFetching, searched, sourceCount, error, search, reset }}>
      {children}
    </SearchContext.Provider>
  )
}
```

- [ ] **Step 2: 编译验证**

Run: `cd web-ui && npx tsc --noEmit`
Expected: FAIL（use-download.ts 仍引用旧 `startDownload` 三件套——预期，Task 10 修）。

- [ ] **Step 3: 提交**

```bash
git add web-ui/src/contexts/search-context.tsx
git commit -m "feat(web-ui): SearchProvider 改建任务+轮询，契约不变"
```

---

### Task 10: useDownload 简化

**Files:**
- Modify: `web-ui/src/hooks/use-download.ts`

**Interfaces:**
- Consumes: `startDownload(opts) -> Promise<StartDownloadResult>`（Task 7）
- Produces: `useDownload(): { start(opts: DownloadOptions) => Promise<StartDownloadResult> }`（`state`/`cancel` 删除）

- [ ] **Step 1: 整文件重写**

```ts
// 下载 hook —— POST /api/download 返回 { task_id } 即任务已入 state.tasks。
// 进度不在此追踪（无 SSE）：任务页用 useTasks 轮询 GET /api/tasks 展示。

import { useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { startDownload } from '@/lib/api'
import type { DownloadOptions } from '@/lib/api'
import type { StartDownloadResult } from '@/lib/types'

export interface UseDownloadReturn {
  /**
   * 启动下载并返回 { task_id }。resolve 时后端已 push 任务到 state.tasks，
   * 并已 invalidate ['tasks']，调用方跳转 /tasks 后列表 refetch 可见新任务。
   */
  start: (opts: DownloadOptions) => Promise<StartDownloadResult>
}

export function useDownload(): UseDownloadReturn {
  const qc = useQueryClient()
  const start = useCallback(
    async (opts: DownloadOptions) => {
      const res = await startDownload(opts)
      qc.invalidateQueries({ queryKey: ['tasks'] })
      return res
    },
    [qc],
  )
  return { start }
}
```

- [ ] **Step 2: 全量构建验证**

Run: `cd web-ui && npm run build && npm run lint`
Expected: PASS（tsc + vite build + oxlint 全绿）

- [ ] **Step 3: 手工验证（Phase A 里程碑验收）**

Run: `cd web-ui && npm run dev`，后端 `cargo run --features web`（或按项目现有启动方式）。
手工核对：搜索能渐进出结果并到 Done；下载能建任务、任务页进度推进、取消/删除正常；书库/书源/设置正常。若任一页面报类型错或行为不对，修复后重验。

- [ ] **Step 4: 提交**

```bash
git add web-ui/src/hooks/use-download.ts
git commit -m "feat(web-ui): useDownload 简化（去 SSE 进度状态机）"
```

---

## M3 — Phase B 骨架（shadcn Nova + 侧边栏）

> 本阶段起 UI 全面重设计。每步之后跑 `npm run build` + `npm run lint`。

### Task 11: shadcn Nova 初始化 + 添加组件

**Files:**
- Modify: `web-ui/components.json`、`web-ui/src/index.css`、`web-ui/package.json`、`web-ui/src/main.tsx`（如 CLI 提示）、`web-ui/tailwind.config.ts`（如 CLI 生成）
- Create: `web-ui/src/components/ui/*`（CLI 生成）

- [ ] **Step 1: 初始化 Nova**

在 `web-ui/` 下执行（先读 shadcn 技能确认当前 init 用法）：

```bash
cd web-ui
npx shadcn@latest init --preset nova
```

按提示接受默认（framework 已检测为 Manual；base 若询问选 radix 或 CLI 默认）。目标状态：
- `components.json` 更新（style 指向 nova、iconLibrary=lucide）
- `web-ui/src/index.css` 写入 Nova 的 CSS 变量（替换原 `@import "@heroui/styles"` 与手写 accent/field 覆盖）
- 若生成 `tailwind.config.ts` 或改写 `index.css`，`@import "tailwindcss"` 保留

- [ ] **Step 2: 添加组件**

```bash
npx shadcn@latest add sidebar card button input select switch tabs badge skeleton pagination progress dialog alert-dialog dropdown-menu tooltip separator scroll-area spinner alert
```

若 `spinner`/`alert`/`empty` 在注册表里不可用（`npx shadcn@latest search -q "spinner"` / `"empty"` 查），改用替代：Spinner → lucide `Loader2` + `animate-spin`；Alert → 现成 Alert 若没有则用 Card + `text-destructive`；Empty → 自定义空态 div（读 shadcn `composition.md` 规则）。

- [ ] **Step 3: 核对组件规则**

按 shadcn 技能步骤 7：逐一读新增的 `web-ui/src/components/ui/*.tsx`，修正硬编码 import、缺 sub-component、图标库不匹配（应为 lucide-react）。

- [ ] **Step 4: 安装 lucide-react + 移除 HeroUI 依赖**

```bash
cd web-ui
npm install lucide-react
npm uninstall @heroui/react @gravity-ui/icons
```

预期：`npm run build` 会报错（页面仍引用 HeroUI）——**本步只装/卸依赖，不改页面**，错误属预期；M4 页面迁移完再回到全绿。

- [ ] **Step 5: 提交**

```bash
git add web-ui/components.json web-ui/src/index.css web-ui/src/components/ui web-ui/package.json web-ui/package-lock.json
git commit -m "feat(web-ui): shadcn Nova 初始化 + 组件库就位"
```

---

### Task 12: 侧边栏布局骨架

**Files:**
- Create: `web-ui/src/components/layout/sidebar.tsx`
- Modify: `web-ui/src/components/layout/layout.tsx`（重构）
- Delete: `web-ui/src/components/layout/navbar.tsx`

**Interfaces:**
- Consumes: `useTasks()`（下载中计数 badge）；`ThemeToggle`（Task 13 改）；`NAV` 五页映射（lucide 图标）
- Produces: `AppSidebar`（shadcn Sidebar 组合：logo + 导航项 + 底部 ThemeToggle）；`Layout` 用 `<SidebarProvider><AppSidebar/><SidebarInset>...`

- [ ] **Step 1: 写侧边栏**

`web-ui/src/components/layout/sidebar.tsx`（要点，按 shadcn sidebar 规则补全细节）：

```tsx
import { NavLink, useLocation } from 'react-router-dom'
import { MagnifyingGlass, ArrowDownToLine, BookOpen, ListFilter, Settings } from 'lucide-react'
import {
  Sidebar, SidebarContent, SidebarGroup, SidebarGroupLabel, SidebarGroupContent,
  SidebarHeader, SidebarFooter, SidebarMenu, SidebarMenuItem, SidebarMenuButton,
  SidebarTrigger,
} from '@/components/ui/sidebar'
import ThemeToggle from '../theme-toggle'
import { useTasks } from '@/hooks/use-tasks'
import { useTranslation } from 'react-i18next'

const NAV = [
  { to: '/search',   labelKey: 'nav.search',   icon: MagnifyingGlass },
  { to: '/tasks',    labelKey: 'nav.tasks',    icon: ArrowDownToLine },
  { to: '/library',  labelKey: 'nav.library',  icon: BookOpen },
  { to: '/sources',  labelKey: 'nav.sources',  icon: ListFilter },
  { to: '/settings', labelKey: 'nav.settings', icon: Settings },
] as const

export function AppSidebar() {
  const { data: tasks = [] } = useTasks()
  const { t } = useTranslation()
  const { pathname } = useLocation()
  const active = tasks.filter((x) => x.status === 'Downloading').length
  // 顶层路径：/search/:bookUrl 这类详情路由也归到 /search 项高亮（沿用旧 navbar 逻辑）
  const activePath = '/' + (pathname.split('/').filter(Boolean)[0] ?? 'search')

  return (
    <Sidebar>
      <SidebarHeader>
        <div className="flex items-center gap-2 px-2 py-1">
          <img src="/logo.png" alt="" className="size-6" />
          <span className="font-semibold">{t('app.title')}</span>
        </div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>{t('nav.group')}</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {NAV.map(({ to, labelKey, icon: Icon }) => (
                <SidebarMenuItem key={to}>
                  <SidebarMenuButton asChild isActive={activePath === to}>
                    <NavLink to={to} end={to === '/search'}>
                      <Icon />
                      <span>{t(labelKey)}</span>
                      {to === '/tasks' && active > 0 && (
                        <span className="ml-auto rounded-full bg-primary px-2 text-xs text-primary-foreground">{active}</span>
                      )}
                    </NavLink>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        <ThemeToggle />
      </SidebarFooter>
    </Sidebar>
  )
}
```

> 注：`/search/:bookUrl` 详情页也归到 /search 项高亮——`isActive` 用 `useLocation().pathname.startsWith(to)`（除 `/search` 用 `=== '/search'` 或 prefix 均可，按 NavLink `end` 语义微调）。

- [ ] **Step 2: 重构 layout.tsx**

```tsx
import { Outlet } from 'react-router-dom'
import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar'
import { AppSidebar } from './sidebar'

/** 根布局：侧边栏 + 内容区。侧边栏跨路由保持。 */
export default function Layout() {
  return (
    <SidebarProvider>
      <AppSidebar />
      <SidebarInset>
        <header className="flex h-12 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger />
        </header>
        <main className="mx-auto w-full max-w-6xl flex-1 p-4 lg:p-6">
          <Outlet />
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}
```

- [ ] **Step 3: 删除 navbar.tsx**

```bash
rm web-ui/src/components/layout/navbar.tsx
```

（原 HeroUI Tabs 导航逻辑已由侧边栏承接。）

- [ ] **Step 4: 编译验证**

Run: `cd web-ui && npx tsc --noEmit`
Expected: 仅剩页面层 HeroUI 引用报错（预期，M4 逐个清）。sidebar 相关类型错误先修干净。

- [ ] **Step 5: 提交**

```bash
git add web-ui/src/components/layout
git commit -m "feat(web-ui): 侧边栏布局骨架（Sidebar + SidebarInset）"
```

---

### Task 13: 共享组件迁移

**Files:**
- Delete: `web-ui/src/components/app-select.tsx`、`app-switch.tsx`、`number-input.tsx`、`confirm-dialog.tsx`
- Modify: `web-ui/src/components/theme-toggle.tsx`（重写为 DropdownMenu）

**Interfaces:**
- Consumes: shadcn Select/Switch/AlertDialog/DropdownMenu/Input；lucide icons
- Produces: `ThemeToggle`（DropdownMenu：浅色/深色/跟随系统）
- 注：原 wrapper（AppSelect/AppSwitch/NumberInput/ConfirmDialog）**直接删除**，由 M4 各页内联使用 shadcn 组件——避免两层封装。

- [ ] **Step 1: 重写 theme-toggle.tsx**

```tsx
import { Moon, Sun, Monitor } from 'lucide-react'
import { useTheme } from 'next-themes'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { useTranslation } from 'react-i18next'

export default function ThemeToggle() {
  const { setTheme } = useTheme()
  const { t } = useTranslation()
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label={t('theme.label')}>
          <Sun data-icon="inline-start" className="dark:hidden" />
          <Moon data-icon="inline-start" className="hidden dark:block" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onClick={() => setTheme('light')}><Sun data-icon="inline-start" />{t('theme.light')}</DropdownMenuItem>
        <DropdownMenuItem onClick={() => setTheme('dark')}><Moon data-icon="inline-start" />{t('theme.dark')}</DropdownMenuItem>
        <DropdownMenuItem onClick={() => setTheme('system')}><Monitor data-icon="inline-start" />{t('theme.system')}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
```

（i18n key `theme.*` 三语补进 `web-ui/src/i18n/locales/*.json`。）

- [ ] **Step 2: 删除四个 wrapper**

```bash
rm web-ui/src/components/app-select.tsx web-ui/src/components/app-switch.tsx \
   web-ui/src/components/number-input.tsx web-ui/src/components/confirm-dialog.tsx
```

- [ ] **Step 3: 编译验证**

Run: `cd web-ui && npx tsc --noEmit`
Expected: 页面层对 wrapper 的 import 报错（预期，M4 清）。`theme-toggle` 无报错。

- [ ] **Step 4: 提交**

```bash
git add web-ui/src/components
git commit -m "refactor(web-ui): 共享组件迁移（删 HeroUI wrapper，重写主题切换）"
```

---

## M4 — Phase B 六页迁移

> 每页一个 Task，原则：**视觉换 shadcn，行为全保留**。每个 Task 结束跑 `npx tsc --noEmit` + `npm run lint`；六页完成后跑 `npm run build` 全绿。i18n 三语文件同步补新增 key。

### Task 14: 搜索页

**Files:**
- Modify: `web-ui/src/routes/search.tsx`

**结构要点（对照现有实现）：**
- 搜索栏：`<div className="flex flex-col gap-2 sm:flex-row sm:items-center">` 内含 `Input`（带放大镜图标：`<Search className="absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />` + `className="pl-9"`）+ shadcn `Select`（源过滤，`value` 受控 string）+ `Button`（`disabled={isFetching || !keyword.trim()}`，文案 `isFetching ? t('search.searching', { count: sourceCount }) : t('search.searchButton')`）
- 流错误：`error && <Alert variant="destructive"><AlertDescription>{error}</AlertDescription></Alert>`
- 骨架：`isFetching && 5 个 <Skeleton className="h-16 w-full" />`
- 结果：`ResultCard` 用 `Card`（`hover:bg-muted/50 transition-colors cursor-pointer`），内部：书名 `font-semibold` + 作者 `text-muted-foreground text-xs`；简介 `text-sm text-muted-foreground line-clamp-2`；Badge 行（`category`/`status`/`word_count` 用 `<Badge variant="secondary">`）；最新章节 + 更新时间 `text-xs text-muted-foreground`；来源 `text-xs text-muted-foreground` 靠右
- 分页：shadcn `Pagination`（沿用现有 `pageItems()` 折叠逻辑 + `PAGE_SIZE=12`）
- 空态/初始态：`text-muted-foreground` 居中文案（沿用现有条件分支）
- 点击结果：`navigate(\`/search/${encodeURIComponent(r.url)}\`, { state: { sourceId: r.source_id } })` 不变

- [ ] **Step 1: 重写页面**（读现有 search.tsx + shadcn 规则，按上述结构替换 HeroUI 组件）
- [ ] **Step 2: 验证** — `cd web-ui && npx tsc --noEmit && npm run lint`
- [ ] **Step 3: 提交** — `git commit -m "feat(web-ui): 搜索页迁移 shadcn（行为保留）"`

### Task 15: 书籍详情页

**Files:**
- Modify: `web-ui/src/routes/book-detail.tsx`

**结构要点：**
- 返回按钮：`<Button variant="ghost" size="sm" onClick={() => navigate(-1)}><ChevronLeft data-icon="inline-start" />{t('book.backToSearch')}</Button>`
- 双栏 `md:grid md:grid-cols-[280px_1fr] gap-6`：左 Card（封面 `aspect-[3/4] overflow-hidden rounded-lg`，无封面时首字母渐变块保留）；右：书名 `text-xl font-bold`、作者、简介 `line-clamp-3 text-muted-foreground`、Badge（status/latest_chapter）
- TOC：Card 内 `ScrollArea className="h-56"` 包章节网格（沿用 `grid grid-cols-2 sm:grid-cols-3` + `chapters.slice(0, 200)`）；「加载/刷新目录」`Button variant="ghost" size="sm"`
- 下载区：格式用 shadcn `ToggleGroup`（或 `Select`，按 shadcn 规则 2-5 项用 ToggleGroup——5 项边界可放宽用 Select 更简洁，二选一保持一致）+ `Button` 启动下载；`handleDownload` 不变（`await startDl(...)` → `navigate('/tasks')`）
- 保留：`sourceId == null` 提示、`isLoading` Skeleton（`h-64`）、`!book` 失败态

- [ ] **Step 1: 重写页面**
- [ ] **Step 2: 验证** — `npx tsc --noEmit && npm run lint`
- [ ] **Step 3: 提交** — `git commit -m "feat(web-ui): 书籍详情页迁移 shadcn"`

### Task 16: 任务页

**Files:**
- Modify: `web-ui/src/routes/tasks.tsx`

**结构要点：**
- 状态汇总：`(['Downloading','Finished','Failed','Cancelled'] as const)` 各 `<Badge>`（计数=0 不渲染，沿用现有逻辑），颜色用 tailwind 原色 class（Global Constraints 豁免）
- 任务卡 `Card`：左侧状态图标块（`size-10 rounded-lg ${s.bg}` + 图标，Downloading 时 `animate-spin`）；书名 + 状态 Badge；右侧 `Button variant="destructive" size="sm"` 取消/删除
- 进度：`showProgress` 时 `<Progress value={pct} className="h-1.5" />`；解析阶段（`isActive && total_chapters===0`）用 `<Progress />` 不定态（shadcn Progress 无 value 即不定态，`aria-label` 保留）；`pct` 文本、章节数、失败数沿用
- 时间行、下载完成按钮（`ArrowDownToLine` + `<a download>` 模式）保留
- 删除确认：`AlertDialog`（`AlertDialogTrigger` 用按钮；`AlertDialogHeader/Title/Description/Footer` + `AlertDialogAction/Cancel`），替代 ConfirmDialog，触发状态沿用 `pending` state
- 分页沿用 `Pagination` + `pageItems()`

- [ ] **Step 1: 重写页面**
- [ ] **Step 2: 验证** — `npx tsc --noEmit && npm run lint`
- [ ] **Step 3: 提交** — `git commit -m "feat(web-ui): 任务页迁移 shadcn"`

### Task 17: 书库页

**Files:**
- Modify: `web-ui/src/routes/library.tsx`

**结构要点：**
- 过滤：shadcn `Tabs`（`all/epub/txt/pdf/html/md` 6 个 `TabsTrigger`，各挂计数 `<Badge variant="secondary">`，沿用 `TAB_BADGE_COLOR` 逻辑换为 Badge variant/class）
- 文件行 `Card`：ext 色块（`size-10 rounded-lg ${EXT_COLOR[f.ext] ?? 'bg-muted'}` 白字大写）+ 文件名/大小/日期 + 下载（`Button asChild` 包 `<a href download>`）+ 删除（`variant="destructive" size="sm"`）
- 空态、Skeleton、Pagination、AlertDialog 删除确认（同 Task 16 模式）
- `handleFilterChange` 切 tab 时 `setPage(1)` 保留

- [ ] **Step 1: 重写页面**
- [ ] **Step 2: 验证** — `npx tsc --noEmit && npm run lint`
- [ ] **Step 3: 提交** — `git commit -m "feat(web-ui): 书库页迁移 shadcn"`

### Task 18: 书源页

**Files:**
- Modify: `web-ui/src/routes/sources.tsx`

**结构要点：**
- 顶栏：启/停 `<Badge>`（enabled/disabled 计数）+ 一键测速 `Button`（`disabled={testingAll || !sources.length}`，testingAll 时显示 `Loader2 className="animate-spin"` + `t('sources.testingAll', { done, total })`）
- 列表 `Card`：状态点（`size-2.5 rounded-full bg-green-500 / bg-muted`）+ 名称/URL（`truncate`）+ 测速中 `Loader2 animate-spin` / 结果 Badge（`ok ? latency : t('sources.timeout')`）+ shadcn `Switch`（`checked={s.enabled} onCheckedChange={() => toggle(s.id)}`）
- 禁用行 `opacity-60` 保留；`testAll` 并发回填逻辑**原样保留**
- 空态（无书源）可加 `text-muted-foreground` 文案

- [ ] **Step 1: 重写页面**
- [ ] **Step 2: 验证** — `npx tsc --noEmit && npm run lint`
- [ ] **Step 3: 提交** — `git commit -m "feat(web-ui): 书源页迁移 shadcn"`

### Task 19: 设置页

**Files:**
- Modify: `web-ui/src/routes/settings.tsx`

**结构要点（本页逻辑最重，行为零改动）：**
- 保留：`EditableSettings`、`FORMAT_OPTIONS`/`TXT_ENCODINGS`、`DEBOUNCE_MS`、`normalizeFormat`、`validate()`、`commit()`（codeId 3004/3005 dispatch）、`update()` 防抖、语言切换、`form` 仅首载初始化、定时器清理——**全部原样**
- 视觉：`Section` 改 shadcn `Card`（`CardHeader` 内图标 + `CardTitle` + `CardDescription`，`CardContent` 放字段）；`Field` 行改 `div`（label `text-sm font-medium` + description `text-xs text-muted-foreground`，控件 `sm:w-56`）；错误 `text-destructive text-xs`
- 控件：路径/代理输入用 shadcn `Input`（错误态 `aria-invalid` + `border-destructive`）；格式/编码/语言用 shadcn `Select`；开关用 shadcn `Switch`；数字用 shadcn `Input type="number"`（或 base `NumberField`，按 Task 11 搜索到的组件）；只读区三个统计框用 `Card` 或 `bg-muted rounded-lg px-4 py-3`
- `SaveStatus`：`saving` → `Loader2 animate-spin` + 文案；`saved` → `CheckCircle2 text-emerald-600`；`error` → `XCircle text-destructive`
- 注意：HeroUI class（`divide-separator`、`bg-default-100/50`、`border-field-border`）全部换 shadcn token（`divide-border`、`bg-muted`、`border-input`）

- [ ] **Step 1: 重写页面**
- [ ] **Step 2: 验证** — `npx tsc --noEmit && npm run lint`
- [ ] **Step 3: 提交** — `git commit -m "feat(web-ui): 设置页迁移 shadcn"`

### Task 20: 移除 HeroUI 残留 + 全量构建

**Files:**
- Modify: `web-ui/src/main.tsx`（如 `@heroui/styles` import 残留）、`web-ui/vite.config.ts`（如有 heroui 相关注释）

- [ ] **Step 1: 清残留**

Run: `cd web-ui && npx tsc --noEmit` 与 `npm run lint`
逐个修掉 HeroUI/gravity 引用（`@heroui/react`、`@gravity-ui/icons`、`@heroui/styles`、HeroUI class 残留）。删除 `main.tsx`/`vite.config.ts` 里 heroui 注释。

- [ ] **Step 2: 全量验证**

Run: `cd web-ui && npm run build && npm run lint`
Expected: PASS（tsc + vite + oxlint 全绿）
Run: `cargo test --features web`（后端回归，应全绿）

- [ ] **Step 3: 手工走查**

`npm run dev` 起前端（后端已跑）：6 页走查 + 搜索/下载/取消/删除 + 深/浅色切换 + 三语切换 + 侧边栏折叠。发现问题修复。

- [ ] **Step 4: 提交**

```bash
git add web-ui/src
git commit -m "feat(web-ui): 清理 HeroUI 残留，Phase B 全量构建通过"
```

---

## M5 — 收尾

### Task 21: 文档更新 + 全量验证

**Files:**
- Modify: `docs/WEB.md`、`README.md`、`web-ui/src/i18n/locales/*.json`（若 Task 13-19 漏 key）

- [ ] **Step 1: 更新 docs/WEB.md**

把搜索/下载的 SSE 契约段改为轮询模型：`POST /api/search` + `GET/DELETE /api/search/{id}`、`POST /api/download` 返 `{ task_id }`、进度走 `GET /api/tasks`。删除 SSE 相关段落。

- [ ] **Step 2: 更新 README**

技术栈表：`web-ui` 行 HeroUI → shadcn/ui (Nova)；截图说明加注"新版待补截图"（旧截图已失效，由用户重截替换）。

- [ ] **Step 3: 三语 key 完整性检查**

`grep -oE "t\('[^']+'" web-ui/src/routes web-ui/src/components | sort -u` 对照三语 json，缺失 key 补齐。

- [ ] **Step 4: 全量验证**

Run: `cd web-ui && npm run build && npm run lint`
Run: `cargo test --features web`
Run: `cargo build --features web`
Expected: 三端全绿。

- [ ] **Step 5: 提交**

```bash
git add docs README.md web-ui/src/i18n
git commit -m "docs: WEB.md/README 更新 API 契约与前端技术栈"
```

### Task 22: 最终验收 + 收尾提交

- [ ] **Step 1: 对照 spec 的验收清单走查**

逐条核对 spec「测试与验证」段：Phase A/B 所有关卡跑一遍；确认未越界（桌面/CLI/书源规则未动）。

- [ ] **Step 2: 最终提交**

```bash
git add -A
git commit -m "chore: Web 前端迁移 shadcn + 轮询化最终验收"
```

（若无可提交内容则跳过。）

- [ ] **Step 3: 报告**

向用户报告：完成的改动清单、验证结果、遗留事项（README 截图待补、桌面端未受影响）。
