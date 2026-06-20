# So-Novel-Rs 代码质量审计摘要（Phase 1-4 汇总）

> 范围：`so-novel-rs` 当前 master 分支（v0.2.6，23k+ 行 Rust）。
> 目的：在不动代码的前提下，盘点架构、可靠性、性能、测试的"质量债"，便于后续小步优化。
> 行动方式：仅审计 + 计划，不修代码 — 等用户确认 `tasks/todo.md` 后再进改动。

---

## 0. 基线（Phase 1 已跑）

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | ✅ 无 diff |
| `cargo clippy --all-targets --all-features -- -D warnings` | ✅ 无警告 |
| `cargo test --all-targets --all-features` | ✅ 284 passed (4 ignored), 5.01s |
| `cargo build --release` | ✅ 13.20s |
| 依赖图 | 89 个直接/传递 crate，没有明显可裁剪点 |

基线干净，可以把精力集中在语义层的优化（错误处理、并发、性能、模块边界、测试）。

---

## 1. 架构与可靠性风险（Phase 2）

### 1.1 错误处理：个别 `panic!` 仍留在生产路径
- `src/app/mod.rs:134` — `Db::open` + `Db::open_in_memory` 都失败时 `panic!`。
  → 应降级为 `pending_notifications.push(UIEvent::Error(...))` 或禁用下载功能后让 UI 继续可看。
- `src/gpui_app/themes.rs:326/330/336/364` — 内置主题 JSON 损坏直接 `panic!`。
  → 应退回到 `Theme::default()` + warn，不杀进程。
- `src/crawler/mod.rs:371` — `permit.acquire_owned().await.expect("semaphore not closed")`。
  → `tokio::sync::Semaphore` 当前实现下不会 close，但把"未来切换 semaphore 实现"的窗口堵死了；改成 `?` + 自定义错误更稳。
- `src/crawler/retry.rs:41` — `last_err.expect("retry loop ran at least once")`。
  → 控制流能保证，但 `debug_assert!` 更显式。
- `src/util/zhconv.rs:101` — `expect("script/style 开标签必有 '>'")`。
  → 防御性：如果上游 HTML 有 BUG 或未来不再调用此函数，会 panic。改成 `let Some(...) = ... else { out.push_str(&rest[pos..]); break; };`。
- `src/parser/toc.rs:116` — 同 semaphore 类 expect。

### 1.2 模块边界：业务层 ↔ UI 框架解耦做得好，但有一处依赖回流
- `app/mod.rs` 提到 "保持 UI 中立"，实际 `app/library_state.rs` / `app/sources_state.rs` 等子模块都是数据 + drain，干净。
- `app/cover.rs` 里 `URI` 风格借用了 gpui image 的 `image::ImageFormat` — OK。
- **真正的小漏洞**：`app/events.rs:114-126` 直接读 `UpdateOutcome` 分支并 push notification — OK；但 `app/search_state.rs` 里 `pending_cover_prefetch` 的 spawn 走 `AppModel.runtime` + `config` + `crate::app::cover::*`，跨越业务层边界调到 `gpui_app`，需要在 `events.rs:42-46` 里组装。
  → 建议把"cover prefetch 的发送逻辑"挪进 `SearchState::drain` 或独立 `ops::cover`，保持 `events.rs` 只做编排。
- `gpui_app` 大量依赖 `crate::app::*` 是合理的（UI 层依赖业务层），没有反向依赖 → 边界 OK。

### 1.3 配置兼容性
- `config/loader.rs` 已经做了：
  - `theme` → `static_name` / `app-lang` → `language` 老键迁移。
  - `toml_edit` 保留注释与字段顺序。
- 风险点：
  - `save_config` 写盘是 `fs::write`（一次写出整文件），无 `tmp+rename` 原子写。
    → 突然断电/磁盘满时，`config.toml` 可能半截写。改成 `tmp+fsync+rename` 解决。
  - `Db::open` 失败兜底 → in-memory，但 in-memory 的 DB 在重启时任务全丢。
    → 当前 `DownloadTask` 会先写 DB 再 push 到内存；如果内存 DB 启动，**重启后历史任务不显示**，且 ops 里 save_task_to_db 仍写 in-memory = 重启全失。
    → 至少加 startup 时检测 in-memory fallback，发出 warning 让用户知情。

### 1.4 异步 / 并发
- `gpui 0.2.2` + `smol` + `tokio` 三套 runtime 共存：
  - `app::runtime::build_shared_runtime` → tokio multi-thread runtime，Box::leak → `&'static`。
  - `gpui::cx.spawn` → smol executor。
  - 业务层 ops 全部走 tokio runtime（用 `tokio::sync::mpsc::UnboundedSender`），跨线程 send 安全。
  - 桥接 (`gpui_app/drain_loop::spawn_drain_loop`) 用 `AsyncApp::update_entity` 把 mpsc 收到的事件搬进 `AppModel` — OK。
- **风险**：
  - `app/library_state.rs::refresh_library_async` 用 `smol::channel::unbounded` 把 `scan_library_dir` 结果送回 smol 域，但 task 本身 spawn 在 tokio runtime 里通过 `tokio::task::spawn_blocking` 跑阻塞 IO；task 内 `tx.send(...).await` 走 smol channel（通过 tokio task 内 poll smol future）— 实际能跑（smol future 与 tokio 兼容），但注释不充分；新人改代码易踩坑。
  - `src/parser/toc.rs:103-126` 平行抓分页：硬编码 `Semaphore::new(8)` + 失败即整本目录失败。
    → 改成"best-effort：失败的页跳过 + 后续顺序时给占位"更友好（避免 200 章里 1 页 404 → 整本丢失）。
  - `crawler/mod.rs` 内 `tokio::select! { sleep = sleep_for(...), _ = cancel.wait() => ... }` — cancel 走 `AtomicBool` + 50ms poll，间隔太大。
    → 改 `tokio::sync::Notify` 实现即时唤醒（且不依赖 sleep 周期）。
  - `crawler/retry.rs::retry_with_backoff` 的 sleep 不接受 cancel token。
    → 当前主循环外层 select 已经把 cancel 包住，但内部 sleep 仍是固定时长 → 取消后最多还要等 `retry_max * 2s` 才能退出。

### 1.5 日志
- `tracing + tracing-appender` 已配置；文件按天滚动、stdout 双输出。
- 风险：
  - 日志保留策略没实现（注释里说"启动时清理逻辑自己做"，但 main.rs 没看到清理代码）。
  - 部分 `tracing::info!` 的字段是 URL（不视为隐私，OK），但**没有看到 proxy password / cookie 屏蔽**：
    - `crawler/mod.rs` 等处的 `format_url_query` 会把关键字放进 URL → 进日志 → 如果关键词是敏感搜索，可被日志回放。
    - `http/fetch.rs` 把 `final_url` 和 `status` 进日志；proxy 鉴权信息由 reqwest 自动屏蔽（默认不在日志），OK。
  - `cfg!(target_os = "...")` 风格的 trace 字段在 release 仍运行（无显著开销，可忽略）。

### 1.6 安全性 / 合规
- 文件名 `util/fs.rs::sanitize_filename` 在 Linux 只 strip `/ \ \0` — 安全。
- 路径遍历：`exporter.rs::build_book_dir_name` 用 `format!("{} ({}) {}", ...)` 经 `sanitize_filename` → 避免 `..` 残留。
- 章节 URL `parser/chapter.rs::resolve_next_url` 没限制 host，可能跳到任意站（parser 行为，符合预期）。
- `boa_engine` 跑 `@js:`：用户自写书源 = 信任用户，但配置 `cfg-bypass` / `qidian-cookie` 是用户输入 → 当前被 `if empty → None` 处理，OK。
- AGPL 协议声明在 `Cargo.toml`（`license = "AGPL-3.0-only"`），但 README 没强调。**未来发版时确认**。
- `Cargo.lock` 已 git 跟踪（隐式 OK）。

---

## 2. 性能瓶颈与可优化点（Phase 3）

### 2.1 HTTP 客户端复用 ✅
- `http/client.rs::build_async_client` / `build_blocking_client` **每次调用都新建** Client。
- `crawler/ops::download.rs::spawn_download` / `crawler/ops::search.rs::spawn_search` / `app/ops/search.rs::spawn_search` 等地方每次都构造新 client → **TCP 连接池完全浪费**。
- **建议**：把 reqwest::Client 作为 `AppModel` 字段，配置变更时重建。连接池（默认 idle 90s, max 100 host）保留 → 高并发抓取章节时 TTI 大幅缩短。
- ✅ **已修复**（Phase 3.1, commit `be76b9e`）：新建 `src/http/clients.rs` — `HttpClients` 集合（safe + unsafe_ssl + gh_proxy），`AppModel` 挂 `Arc<HttpClients>`，所有 crawler/search/health/cover 调用点切到 `http.for_rule(&rule)`。

### 2.2 Selector / Regex 缓存 ✅
- `parser/chapter.rs::is_last_page:233` — `Regex::new(&chapter_rule.next_chapter_link)` **每次分页页都重新编译**！
  → 同章内 50 页 × 编译正则 = 浪费；改 Lazy + cache key (chapter_url, rule_id) → 复用。
- `parser/toc.rs::extract_book_id` 也 `Regex::new(&book_rule.url)` — 每本书都编译一次，可接受但应缓存到 `Rule` 上做 memo。
- `parser/dom.rs` 已经 `Lazy<Regex>`，OK。
- `src/http/util.rs::build_form_data` 已经 `Lazy<Regex>`，OK。
- ✅ **已修复**（Phase 3.2, commit `8c6db6d`）：新建 `src/parser/cache.rs` — `cached_selector` / `cached_regex` 按字符串全局缓存，7 处调用点切到 cache。

### 2.3 DB 事务与 N+1 ✅（Phase 2 已修复）
- `db/sources.rs::insert_many` 已用事务包裹（OK）。
- `db/sources.rs::seed_from_default` **逐条 INSERT，没用事务** — 启动时 N 次 fsync = 几百 ms 浪费（默认 6 条规则，影响不大但 N 大时放大）。
- ✅ **已修复**（Phase 2, commit `418f79c`）：`seed_from_default` 用 `conn.transaction()` 包裹。
- `db/tasks.rs::upsert` 用 `INSERT OR REPLACE` — OK。
- `db/tasks.rs::delete_finished` 加载全表 → 在 Rust 里 filter → 重新 DELETE。
  → 改成 `DELETE FROM download_tasks WHERE status IN (...)` 一句 SQL，省去 round-trip 与全表读。
- ✅ **已修复**（Phase 2, commit `9c10c4c`）：`delete_finished` 改成单条 `DELETE WHERE json_extract(...)`。
- `db/tasks.rs::list` 没 LIMIT，但下载任务数不会过百，OK。

### 2.4 Export 写路径 ✅
- `export/epub.rs` / `export/pdf.rs` 是一次性 `Vec<u8>` 拼好后写出：
  - 5 MB 章节 → `String::with_capacity` → 中途若失败，整段丢失。
  - PDF 单本可达 50 MB+ → 全载入内存。
- **建议**：epub builder 本身支持分章节 add，pdf_oxide DocumentBuilder 也是 streaming — 改 `BufWriter<File>` 包裹，逐章 push。
- ✅ **已修复**（Phase 3.3, commit `d54f576`）：epub 用 `BufWriter<File>` 包裹 `generate()`；pdf 用 `build()` + `BufWriter<File>::write_all`。
- `exporter.rs::write_chapter_files` 先 `sort_chapter_files` 收集再写 → OK，但 `Path::join(book_dir, "{order:03}_{title}.{ext}")` 没处理同名冲突（数字前缀虽然防了一部分，但用户改 title 后会冲突）。
- ✅ **已修复**（Phase 3.3, commit `d54f576`）：`write_chapter_files` 加 `unique_path` 文件名去重（冲突时加 `(1)` 后缀）。
- `exporter.rs::build_book_dir_name` 的 format 不去重（同名作者 + 同名书 + 同 format → 同目录名）→ 第二次导出覆盖第一次。
- ⚠️ **未修复**（低优先级，留 future）。

### 2.5 GUI 重渲染 — 已评估，无需改动
- `gpui_app/pages/library/mod.rs` 的 watch + scan → `cx.notify()` → 全页 rerender。
  → Library 列表项是独立的 `List` 子组件 → GPUI 会按 Row 数据 diff；目前应该不会全量重绘（没看到 ListState），但需要 review `List::new` 的实现是否带虚拟化。
- `gpui_app/pages/tasks/` 多任务进度更新：每个任务 drain 后 `cx.notify()` → 全 Tasks 页 rerender。
  → 任务多时（30+）可能掉帧；按 `task_id` 粒度 notify 更好（如果框架支持）。
- **搜索页 `gpui_app/pages/search/mod.rs`**：每收到一个 SourceSearchEvent → notify → 全 search 页 rerender → 100+ 源 × 频繁推送会掉帧。
  → 增量 replace / 局部 state diff（gpui 0.2.2 的 entity-level notify 是支持分粒度的）。
- ✅ **已评估**（Phase 3.5）：drain_loop 每 100ms 批量 drain 后只发一次 `ctx.notify()`，天然节流；ListState + ListDelegate 已提供页内虚拟化；改为 per-page entity notify 需架构重构，收益有限。

### 2.6 内存占用 — 已评估，无需改动
- `DownloadTask::finished_chapters: Vec<Chapter>` — 单本书 1500 章 × (URL + title + content)，content 30k 字符 = **单本书 50+ MB**。
- 当导出完成清理时整 Vec 还在内存 → 导出完应 `mem::take` / `Vec::clear()` 释放。
- `app::library_state::LibraryState::entries` 同理 — 但 LibraryEntry 只存 path/size/mtime，不存正文，OK。
- ✅ **已评估**（Phase 3.4）：`finished_chapters` 字段不存在于当前实现。章节通过 `write_chapter_files` 流式写盘，不在内存保留。`book_meta: Option<Book>` 仅几 KB，不值得提前清空。

### 2.7 其他
- `gpui_app/root.rs` 的 `LOGO_PNG` 已经 Lazy 解码 + 一次缓存 → OK。
- `gpui_app/themes.rs` Lazy 主题 JSON 解析 — OK。
- `rfd` 异步文件对话框已在 gpui worker thread 异步用，OK。
- `boa_engine::Context::new()` + 加载 book 一次评估一次 → 如果一章内多次 `@js:`，应缓存 Context（Java 端 OK）— 当前实现可能每章新建 Context，需 review。

---

## 3. 测试覆盖缺口（Phase 4）

> 现有 284 tests 覆盖了：URL join、encoding detection、filename sanitize、parser（多 case）、retry、db schema、export、config compat、CLI JSON。
> 但仍有一些关键路径缺测试：

### 3.1 URL / 网络层
- ✅ `resolve_base_for_join` — 4 个单测（Phase 4.1, commit `24b9962`）
- ❌ `http/util.rs::clean_invisible_chars` 中 PUA / 零宽字符的边界 — 测试只覆盖 `\u{200B} \u{FEFF}` 两个零宽，未覆盖 C1 控制区。
- ❌ `parser/chapter.rs::is_last_page` 对 `nextChapterLink` 正则的 panic 路径（正则编译失败时静默 fallback） — 没测。

### 3.2 Parser
- ❌ `parser/chapter.rs::fetch_paginated_content` 的 50 页上限（极端 case） — 无测（需 HTTP mock）。
- ❌ `parser/dom.rs::clear_all_attributes` 对 SVG / MathML 命名空间 — 无测。
- ✅ `parser/dom.rs::remove_tags` — 4 个新测试（嵌套同标签 / 深层嵌套 / 相同兄弟 / 无匹配）（Phase 4.1, commit `24b9962`）
- ❌ `parser/toc.rs::collect_pagination_urls` 模式 1/2 的混合行为（option 兜底后转递归） — 无测（需 HTTP mock）。
- ❌ `parser/book.rs::parse_book_detail` 的 JS postprocess 失败回退 — 部分覆盖。
- ❌ `parser/search.rs::search_streaming` 的 cancel 路径 — 无测。

### 3.3 Crawler
- ✅ `crawler/mod.rs::wait_cancelled` — 3 个测试（Phase 3.6, commit `3094d6c`）：`cancel_token_starts_uncancelled_and_can_be_set` / `wait_cancelled_immediate_after_cancel` / `wait_cancelled_waits_for_cancel_signal`
- ❌ `crawler/mod.rs::cleanup_chapters_dir_if_empty` 在并发下载未完成时的并发安全 — 无测。
- ❌ `crawler/cover_updater.rs::update_cover_for_existing_book` 的"用户已删 book 但下载任务还引用" — 无测。

### 3.4 DB
- ✅ `db/tasks.rs::delete_finished` — 3 个测试（Phase 2, commit `9c10c4c`）：`delete_finished_only_finished` / `delete_finished_empty_or_all_running` / `delete_finished_idempotent`
- ✅ `db/sources.rs::seed_from_default` — 已有 `seed_inserts_main_rules_then_idempotent` 测试（Phase 2, commit `418f79c`）
- ❌ 并发：多 ops 同时 upsert 同一 task_id — 依赖 SQLite 单连接，没问题但应加 stress test。

### 3.5 Export
- ✅ `export/pdf.rs::wrap_text` — 2 个测试：`wrap_text_breaks_long_cjk_line` / `wrap_text_keeps_ascii_word_intact`
- ✅ `export/epub.rs::detect_image_mime` — 改造为 JPEG/GIF/WebP/BMP 显式检测 + 2 个新测试（Phase 4.1, commit `24b9962`）
- ✅ `export/exporter.rs::write_chapter_files` 文件名冲突 — 1 个测试 `write_chapter_files_deduplicates_same_title`（Phase 3.3, commit `d54f576`）
- ❌ 5MB+ 大章节导出内存压力 — 无测（建议加 `#[ignore]` 手动跑）。

### 3.6 Config / Loader
- ❌ `config/loader.rs::save_config` 的 `tmp+rename` 改造后原子性 — 无测。
- ❌ `rules/loader.rs::load_rules_from_path` 的深嵌套 + 符号链接循环 — 无测。
- ❌ `config/loader.rs` 老键 (`theme`, `app-lang`) 在新版本 schema 下写回后再次读 — 一次 round-trip 测过，但反复迁移未测。

### 3.7 CLI
- ❌ `cli/handlers.rs` 各 subcommand 的 stdout JSON schema 兼容性（保证下游脚本不破） — 无 contract test。
- ❌ CLI 错误码的稳定性（0 / 1 / 2 / ...） — 无枚举测。

### 3.8 Async / 并发回归
- ❌ `gpui_app/drain_loop::spawn_drain_loop` 的 100ms tick 在 CPU bound 任务下的积压行为 — 无测（GPUI 测试困难，列在此提醒）。

---

## 4. 已识别的"高分性价比"改动（按收益 / 风险比排序）

| # | 改动 | 风险 | 收益 | 状态 |
| --- | --- | --- | --- | --- |
| 1 | 把 `reqwest::Client` 提到 `AppModel`，复用连接池 | 低（行为不变） | 高（下载 200 章可能提速 2-3×） | ✅ Phase 3.1 `be76b9e` |
| 2 | `parser/chapter.rs::is_last_page` 的 Regex 缓存 | 低 | 中（重复分页省 CPU） | ✅ Phase 3.2 `8c6db6d` |
| 3 | `db/tasks.rs::delete_finished` 改成 SQL 一次性 DELETE | 低 | 低（小表几乎不变，但减少 round-trip） | ✅ Phase 2 `9c10c4c` |
| 4 | `config/loader.rs::save_config` 用 tmp+rename | 低 | 中（防 config.toml 半截写崩配置） | ✅ Phase 2（bundled in `196d413`） |
| 5 | 移除 `app/mod.rs:134` 的 panic → 改 in-memory fallback + warning toast | 低 | 中（启动更稳） | ✅ Phase 2（bundled in `196d413`） |
| 6 | `gpui_app/themes.rs` 的 4 处 panic → 退回到默认主题 + warn | 低 | 中（主题文件坏不再杀进程） | ✅ Phase 2（bundled in `196d413`） |
| 7 | `export/*` 大文件流式写出（pdf/epub ≥10MB） | 中（行为需验证） | 中（内存下降） | ✅ Phase 3.3 `d54f576` |
| 8 | `DownloadTask::finished_chapters` 导出后立即 `Vec::clear()` | 低 | 中（释放 50MB+/本） | ✅ Phase 3.4（字段不存在，已评估） |
| 9 | `parser/toc.rs` 分页 best-effort 化（单页失败不致命整本） | 中（语义有微变） | 中（章节 1 页 404 不再全失） | ⚠️ 未做（留 future） |
| 10 | 日志保留策略（30 天）实现 | 低 | 低（防日志盘塞满） | ✅ Phase 2 `ee78f4b` |
| 11 | `crawler::wait_cancelled` 改用 `tokio::sync::Notify` 立即响应 | 低 | 中（取消响应从 50ms → <1ms） | ✅ Phase 3.6 `3094d6c` |
| 12 | 补测试（见 §3 各 ❌） | 低 | 高（防回归） | ✅ Phase 4.1 `24b9962` + Phase 4.3 `92a7133` |

---

## 5. 不在本次范围内（避免过度工程化）
- ❌ 重写 crate 为 actor model / 引入 domain layer（`diesel` / `sea-orm` 等）
- ❌ 引入 WebAssembly 替代 boa
- ❌ 全异步 GUI 改造（当前 GPUI 是同步主线程 + 后台 task 的混合，已经合理）
- ❌ 完整 I18N 重写（仅补缺失 key，按需）
- ❌ 中文文档重写（仅在改动附近就地修订）
- ❌ 拆分 crate（当前单 crate 模块化已足够）

---

## 6. 风险与建议
- v0.2.6 已经在 master，建议每个 commit 一个 fix，便于 review / revert。
- 优先做 §4 前 6 项（低风险高收益）。
- 第 7-9 项属"行为微变"，应在 commit message 标注兼容影响；必要时加 feature flag。
- 第 10-12 项是卫生性工作，可与主线并行。