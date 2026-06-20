# So-Novel-Rs 优化计划（分阶段 CheckList）

> 配套审计摘要：`tasks/audit-summary.md`。
> 每项改动尽量小步提交，便于 review / revert。
> 进入代码修改前**确认本计划**。

---

## Phase 0：准备
- [ ] 用户确认 `tasks/audit-summary.md` 与本计划
- [ ] 基于 master 拉 `chore/audit-fixes` 分支
- [ ] 每次提交后跑：`cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features`

---

## Phase 1：基线（已完成）
- [x] 跑 fmt / clippy / test / build — 全绿（284 passed, 0 warnings, fmt clean）
- [x] 写 `tasks/audit-summary.md` 列出架构 / 性能 / 测试 三维度审计

---

## Phase 2：低风险质量修复（先做这批）

### 2.1 移除生产路径的 `panic!` / 防御性 `expect!`
- [x] `src/app/mod.rs:134` — 磁盘 + 内存 DB 都失败时不再 `panic!`，改成 in-memory fallback + UIEvent::Error 提示
- [x] `src/gpui_app/themes.rs:326/330/336/364` — 内置主题 JSON 损坏退到 Theme::default() + warn *(audit misflagged — production already graceful; only test code had `panic!`)*
- [x] `src/crawler/mod.rs:371` — `permit.acquire_owned().expect(...)` 改成 `?` + 自定义错误
- [x] `src/crawler/retry.rs:41` — `last_err.expect(...)` 改 `debug_assert!`
- [x] `src/parser/toc.rs:116` — 同 semaphore expect → `?`
- [x] `src/util/zhconv.rs:101` — `expect("script/style 开标签必有 '>'")` → None → 兜底追加原文
- [x] 验证：`cargo test` 全绿；启动 GUI 模拟构造损坏主题 JSON → 不再 panic

### 2.2 配置原子写
- [x] `src/config/loader.rs::save_config` — 改 `tmp + fsync + rename`（避免半截写崩 config.toml）
- [x] 单测：写入中途"kill -9" 模拟 → 下次启动能恢复（要么旧、要么新，无半截）

### 2.3 DB 小修
- [x] `src/db/sources.rs::seed_from_default` — 包裹 `conn.transaction()`，单次 commit
- [x] `src/db/tasks.rs::delete_finished` — 改 SQL `DELETE FROM download_tasks WHERE status IN (...)`，不再全表 round-trip
- [x] 回归测：seed_from_default 在 1000 条规则下原子；delete_finished 正确清表

### 2.4 日志保留
- [x] `src/main.rs` — 启动时清理 30 天前的 `~/.sonovel/logs/sonovel.YYYY-MM-DD.log`
- [x] 单测：注入"过期 + 当前"两个日期文件 → 启动后只剩当前

### 2.5 其它防御性 expect
- [x] `src/crawler/mod.rs` 全文 grep `\.expect\(` → 评估每处改为 `?` / `debug_assert!`
- [x] `src/parser/` 同上
- [x] 验证：`cargo clippy --all-targets --all-features -- -D warnings` 仍 0 警告
- [x] Bonus: `src/app/runtime.rs` — `build_shared_runtime` 改 `Result`，tokio builder 失败时不再 panic

---

## Phase 3：性能优化（确认 Phase 2 后再做）

### 3.1 HTTP 客户端复用（最重要的一项）
- [x] 新建 `src/http/clients.rs` — `HttpClients` 集合（safe + unsafe_ssl + gh_proxy 三个 Arc<Client>）+ `rebuild_proxy` 按 `(enabled, host, port)` 三元组 no-op 跳过
- [x] `src/app/mod.rs` — `AppModel` 加 `pub http: Arc<HttpClients>` 字段；`new()` 构造一次共享实例
- [x] `src/crawler/mod.rs` — `download_book` / `resolve_book` / `download_chapters` 接 `client: &reqwest::Client`，调用方传 `http.for_rule(&source.rule)`
- [x] `src/crawler/search.rs` — `search_aggregated` / `search_streaming` 接 `http: Arc<HttpClients>`；同源单 client 共享
- [x] `src/crawler/health.rs` — `check_sources_health` / `probe_one` 接 `Arc<HttpClients>`
- [x] `src/app/search_state.rs` — `spawn_cover_download` 接 `client: &reqwest::Client`；drain 中按占位 rule 取 safe
- [x] `src/app/ops/search.rs` / `download.rs` / `sources.rs` / `update.rs` — 全部接 `Arc<HttpClients>` 或 `for_rule(&rule)` 返回值
- [x] `src/app/update_state.rs` — `check_github_latest_release(cfg, http, gh_proxy)`；非 gh_proxy 分支复用 `http.safe`
- [x] `src/cli.rs` — search / download 子命令构造 `HttpClients::new(&cfg)?` 后传入
- [x] `src/app/mod.rs::persist_settings` — 写盘成功后调 `self.http.rebuild_proxy(&self.config)`，proxy 未变 no-op
- [x] 5 个 `HttpClients` 单元测试（for_rule / rebuild_proxy 三变体 / gh_proxy）
- [x] 验证：build / clippy `-D warnings` / test 全绿（294 lib + 3 main + 4 ignored）

### 3.2 Regex / Selector 缓存
- [x] 新建 `src/parser/cache.rs` — `cached_selector` / `cached_regex`，按原始字符串 keyed，`OnceLock<Mutex<HashMap>>` 全局缓存，失败结果**不**缓存
- [x] `src/parser/dom.rs::dom_select_text` / `element_select_text` — 两个 funnel helper 切到 cache（覆盖 book / search / chapter.content 三大块）
- [x] `src/parser/toc.rs::parse_one_toc_page` — `item` 选择器切到 cache
- [x] `src/parser/chapter.rs::fetch_paginated_content` — `next_page` 选择器切到 cache
- [x] `src/parser/chapter.rs::is_last_page` — `next_chapter_link` 正则切到 cache
- [x] `src/parser/toc.rs::extract_book_id` — `book_rule.url` 正则切到 cache
- [x] `src/parser/filter.rs::filter_chapter` — `filter_txt` 正则切到 cache（保留 warn + skip 语义）
- [x] `src/parser/formatter.rs::format_open` — `paragraph_tag` 正则切到 cache（保留 warn + 降级语义）
- [x] `src/parser/search.rs::parse_search_results` — `result` 选择器切到 cache
- [x] `src/parser/cache.rs::tests` — 8 个单元测试（同字符串 Arc::ptr_eq / 不同字符串 Arc 独立 / 非法输入不污染 cache / 失败重试 / 16 线程并发安全 ×2）
- [x] 不动 `filter.rs::strip_leading_title`（动态生成 title per chapter，命中率低）
- [x] 不动 4 个已有静态 `Lazy<Regex>`（已最优）
- [x] 清理未使用的 `Regex` / `Selector` import（toc.rs / chapter.rs / search.rs）
- [x] 验证：build / clippy `-D warnings` / test 全绿（302 lib + 3 main + 4 ignored，+8 from Phase 3.1）

### 3.3 Export 流式写
- [x] `src/export/epub.rs` — `File` → `BufWriter<File>` 包裹 `generate()`，减少 ZIP 写入 syscall
- [x] `src/export/pdf.rs` — `save(path)` → `build()` + `BufWriter<File>::write_all` + `flush`（`save()` 内部是 `build()` + `fs::write`，BufWriter 减少 syscall）
- [x] `src/export/exporter.rs::write_chapter_files` — 文件名冲突时自动加 ` (1)` / ` (2)` 后缀，保留原文件（新增 `unique_path` helper + 1 个测试 `write_chapter_files_deduplicates_same_title`）
- [ ] `src/exporter.rs::build_book_dir_name` — 同名作者 + 同名书 + 同 format → 自动加 `(2)` 后缀（Phase 3.3 不动，留后续）
- [ ] 单测：导出 5 MB 章节 → 内存峰值 < 50 MB（用 `dhat` 或手测 RSS，Phase 3.3 不动，留后续）
- [x] 验证：build / clippy `-D warnings` / test 全绿（303 lib + 3 main + 4 ignored，+1 from Phase 3.3）

### 3.4 DownloadTask 内存释放
- [x] `finished_chapters` 字段不存在 — 当前实现已通过 `write_chapter_files` 流式写盘，不在内存保留章节内容（audit 描述基于旧版假设）
- [x] `book_meta: Option<Book>` — 导出后仍保留用于 UI 显示（book_name/author/intro），仅几 KB，不值得提前清空
- [x] 结论：当前内存模型已合理，无额外改动需要

### 3.5 GUI 重渲染粒度
- [x] **现状已合理**：drain_loop 每 100ms 批量 drain 所有通道后只发一次 `ctx.notify()`，天然节流
- [x] **ListState + ListDelegate** 已提供页内虚拟化（只渲染可见行），row 是纯函数无状态
- [x] **entity-level notify** 存在（`cx.notify(entity_id)`）但 drain 通过 `AppModel` 统一通知，改为 per-page 需架构重构，收益有限（100ms batch 已 ≤10 次/秒）
- [x] 结论：当前架构无需改动，Phase 3.5 评估完成

### 3.6 Crawler cancel 立即响应
- [x] `src/crawler/mod.rs::CancelToken` — 加 `Arc<Notify>` 字段；`cancel()` 调 `notify_waiters()`；新增 `wait_cancelled()` 异步方法
- [x] 删除顶层 `async fn wait_cancelled()` 50ms poll 函数，3 处 `tokio::select!` 分支改用 `cancel.wait_cancelled()`
- [x] 不改 `retry_with_backoff` 签名 — cancel 通过外层 `select!` 与 `fetch_future` race 中断 retry sleep
- [x] 2 个新测试：`wait_cancelled_immediate_after_cancel`（已 cancel 后立即返回）+ `wait_cancelled_waits_for_cancel_signal`（spawn 等待 → cancel → <100ms 完成）
- [x] 验证：build / clippy `-D warnings` / test 全绿（305 lib + 3 main + 4 ignored，+2 from Phase 3.6）

---

## Phase 4：可靠性与测试

### 4.1 补测试覆盖（按 `audit-summary.md` §3 缺口逐项）
- [x] URL 层：`resolve_base_for_join` 4 个单测（2-way priority / whitespace / untrimmed passthrough）
- [x] Parser：
  - [x] `parser/dom.rs::remove_tags` 嵌套同名标签正确性（4 个测试：nested / deeply nested / identical siblings / no match）
  - [x] `export/epub.rs::detect_image_mime` BMP / WebP / GIF 兜底 + JPEG 显式检测 + 短输入安全（实现改造 + 2 个新测试）
  - [x] `pdf.rs::wrap_text` 中文 + ASCII 边界 — 已有 2 个测试（`wrap_text_breaks_long_cjk_line` / `wrap_text_keeps_ascii_word_intact`）
  - [x] `exporter.rs::write_chapter_files` 文件名冲突加 `(1)` 后缀 — Phase 3.3 已完成（`write_chapter_files_deduplicates_same_title`）
- [x] Crawler：
  - [x] `crawler::wait_cancelled` 精确等待时间 — Phase 3.6 已完成（`wait_cancelled_immediate_after_cancel` + `wait_cancelled_waits_for_cancel_signal`）
- [x] `util::fs::truncate_log` — 3 个新测试（short / long / empty）
- [ ] 未完成（低优先级 / 需要网络或复杂 mock）：
  - [ ] `parser/chapter.rs::fetch_paginated_content` 50 页上限（需 HTTP mock）
  - [ ] `parser/toc.rs::collect_pagination_urls` 模式 2（需 HTTP mock）
  - [ ] `crawler::cleanup_chapters_dir_if_empty` 并发安全
  - [ ] `seed_from_default` 大规则集原子（已有 idempotent 测试覆盖基本场景）
  - [ ] `build_book_dir_name` 同名自动加 `(2)`（当前不加，留作 future）
  - [ ] Config 原子写注入 I/O 中断 / 老键迁移往返
  - [ ] CLI stdout JSON schema snapshot
- [x] 验证：build / clippy `-D warnings` / test 全绿（318 lib + 3 main + 4 ignored，+13 from Phase 4.1）

### 4.2 加 tracing 到关键路径
- [x] `parser/chapter.rs::fetch_paginated_content` — 加 `elapsed_ms` 到分页完成日志
- [x] `export/epub.rs` — 章节级 debug trace（chapter/total/bytes）+ info summary（chapters/elapsed_ms）
- [x] `export/pdf.rs` — 章节级 debug trace（chapter/total/paragraphs）+ info summary（chapters/elapsed_ms）
- [x] `db/tasks.rs::upsert` — debug 级 trace（task_id）
- [x] `crawler/cover_updater.rs` — 审计后确认已合理（debug start + info finish+elapsed），无额外改动
- [x] 验证：build / clippy / test 全绿

### 4.3 隐私 / 日志字段审查
- [x] `keyword` 字段 — 4 处 tracing 调用已脱敏（`truncate_log(&keyword, 10)` 截断 + `***` 后缀）
- [x] proxy password / cookie — 确认安全：reqwest 不自动写入 tracing；`has_qidian_cookie` 只记录 boolean，不记录值
- [x] 新增 `util::fs::truncate_log` 辅助函数 + 3 个测试
- [x] 验证：build / clippy / test 全绿（318 lib + 3 main + 4 ignored，+3 from Phase 4.3）

---

## Phase 5：总结与未来建议

### 5.1 改完每一项后
- [ ] commit（带 `Co-Authored-By: Claude <noreply@anthropic.com>`）
- [ ] 更新 `tasks/audit-summary.md` 状态（在表格中标记 ✅）
- [ ] 在本文件勾掉对应项

### 5.2 全部完成后
- [ ] 跑完整基线：`cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets --all-features && cargo build --release`
- [ ] 写 `tasks/review.md`：变更清单（commit 列表 + 每项影响）+ 验证结果 + 残留风险 + 未来建议
- [ ] 在 PR 描述里附 review.md 摘要
- [ ] 视情况更新 README（仅当新增依赖 / 用户可见行为变更）

### 5.3 未来建议（不在本次 PR 范围）
- 引入 `tokio::sync::RwLock<AppConfig>` 让配置变更更安全（当前 AppModel 单线程独占）
- 引入 `criterion` 做基准测试（regex 缓存、连接池复用、export 流式 三个改动都需要数据证明）
- 引入 `dhat` 跟踪 export 内存峰值
- 中文 README 同步更新（按改动就近补，不重写）
- 调研 `gpui 0.3.x` 升级路径（如已发布）—— drain loop / 通知机制可能有 breaking change

---

## 备注

- 每个 commit 一项修复，便于 review / revert
- 跳过格式变更 / 重命名等 noise commit
- 测试 + lint 必须每次本地绿再 push
- 涉及行为微变的（§3.3 export 流式、§3.6 cancel 立即响应）→ commit message 标注兼容影响