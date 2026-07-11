# 完整变更日志

so-novel-rs 的**所有 git 提交**，按版本分组。最近版本与对外摘要见
[`CHANGELOG.md`](./CHANGELOG.md)。

> **说明**：
> - 早期版本（v0.2.6 之前）的发布以 `release: vX.Y.Z` 单独 commit + 之前若干
>   `chore: bundled changes` 收口，本表按"release commit → 上一 release commit"
>   的范围列出所有中间提交。
> - 同一 commit hash 在多个范围里都出现过是正常的（release commit 自身属于
>   上一个版本的尾）。
> - 短 hash（`%h`）便于在 `git show <hash>` 查详情；日期格式 `YYYY-MM-DD`。

---

## [Unreleased] (master)

> 等待下一个 release 的占位段。

## [0.3.5] - 2026-07-11

> 本版本含 **49 个 commit**（详见文末按主题/时间倒序列出）。
> 主题：模块重组 + 业务层抽离 + error 系统重塑 + lint cleanup + Web 多语言适配。

- **Cargo.toml** `version = "0.3.4"` → `"0.3.5"`；`web-ui/package.json` 同步
  `"0.3.4"` → `"0.3.5"`；`cli::args.rs::VERSION_STRING` 走
  `env!("CARGO_PKG_VERSION")` 自动跟随；`./target/debug/so-novel-rs -V` → `so-novel-rs 0.3.5`

### Phase 3：业务层抽离（`core/` 三端共享）

将 `desktop/` 与 `web/` 共享的业务逻辑从 GUI 渲染层抽出到 `core/`，共 10 个子阶段：

- `46df788` 2026-07-10 — **feat(web)**: phase 3.0 — WebError blanket `From<String>` + `From<&str>`
  （handler 写 `WebError::from(s)` 即可，不再 each call site 手搓 enum）
- `116b77b` 2026-07-10 — **refactor(core)**: phase 3.1 — `core::config_helpers` + `core::search`
  （从 desktop 共享出 config validate helpers + search 入口；3 个 search fn 不再仅供 GUI）
- `06e8a49` 2026-07-10 — **refactor(core)**: phase 3.2 — `core::sources`（rule lookup + parse + URL key）
- `95b1822` 2026-07-10 — **refactor(core)**: phase 3.3 — `core::bootstrap`（同时删除 `cli/util.rs`，
  bootstrap 函数对所有 mode 统一）
- `495636d` 2026-07-10 — **refactor(core)**: phase 3.4 — `core::download_task::drain` 重写 +
  `apply_to_task` 抽出（drain 主循环逻辑独立，与 GPUI model update 行为解耦）
- `7944dca` 2026-07-10 — **refactor(core)**: phase 3.5 — `core::library`（ext + `open_download_file`）
- `48230a5` 2026-07-10 — **refactor(core)**: phase 3.6 — `core::update`（GitHub release check，
  与 gpui 上下文解耦，可在 CLI 复用）
- `3c585ae` 2026-07-10 — **refactor(core)**: phase 3.7 — `core::async_progress::try_drain_all`
  （跨模式的 progress drain 统一抽象）
- `df1d24f` 2026-07-11 — **refactor(web)**: phase 3.8 — `read_state_or_sse` / `read_state_or_json`
  helper（web handler 的标准红绿灯：state error → SSE 直返 / JSON 直返）
- `0763d7a` 2026-07-11 — **refactor(cli)**: phase 3.9 — `build_cli_runtime` + `print_progress_line`
  抽离（runtime setup 与 progress render 独立可测）

### Phase 1+2：模块重命名（清理名字债）

- `4af1f3c` 2026-07-10 — **refactor(modules)**: phase 1+2 —
  - `gpui_app` → `desktop`（更准确的命名，三 mode 之一）
  - `app` → `desktop::model`（业务逻辑贴近 UI 层）
  - `download_task` → `core::download_task`（业务层独立于 GUI）
  - `util` → `utils`（惯用复数命名）、`persistent` → `db`（更准确的命名）

### Enterprise 重构（PR #1–#16，29 个 commit）

#### 错误系统根类型

- `1810492` 2026-07-08 — **feat(error)**: add `AppError` root type + `AppResult` alias (#2)
  —— 5 + 2 = 7 个 ops / message-type ops 函数迁移到 `AppResult<T>` 透传
- `0a3db9c` 2026-07-08 — **feat(constant)**: add `error_code` table (#3) —
  业务层错误码单点维护，`WebError::message()` 改为查表
- `abfd5ad` 2026-07-08 — **feat(db)**: add `DaoError` root type (#11) —
  持久化层错误统一；`tracing::instrument` 加在 atomic write 路径上 + 6 个单元测试

#### 配置拆分

- `dab52e2` 2026-07-08 — **refactor(config)**: split AppConfig into 6 sub-structs (#6) —
  `global / source / crawl / download / cookie / proxy` + `validate()` + singleton 入口

#### AppResult 迁移

- `07a10c0` 2026-07-08 — **refactor(app)**: migrate 5 ops functions to AppResult<T> (#7)
- `a4ef853` 2026-07-08 — **refactor(app)**: migrate 2 message-type fields to AppResult<T> (#8)

#### 命名债 / Lint 套件

- `4d6d23e` 2026-07-08 — **refactor(enterprise)**: rename util→utils, persistent→db;
  add lints (#1) — 加 pedantic lint 套件
- `e7a768a` 2026-07-08 — **chore(lint)**: add `unwrap_used` / `expect_used` / `panic` pedantic lints (PR #5a)
- `9935300` 2026-07-08 — **docs+test(utils)**: document module map; add lock poison tests +
  doctests (#4)

#### 大文件拆分（Batch 2a-5，14 个 commit）

**Batch 1 / logger**
- `12b021e` 2026-07-08 — **feat(logger)**: add JSON-on-by-default logger module

**Batch 2a / crawler**
- `7cff651` 2026-07-08 — **refactor(crawler)**: split mod.rs (827) into 4 focused files

**Batch 2b-i / export pdf**
- `31c9bb8` 2026-07-08 — **refactor(export)**: split pdf.rs (820) into 3 focused files

**Batch 2b-ii / app+db**
- `a4e6c60` 2026-07-08 — **refactor(app,db)**: split mod.rs (624) and rules.rs (617)

**Batch 3 / parser**（4 个 commit）
- `03a8df8` 2026-07-08 — **refactor(parser)**: split dom.rs (581) into selector + transform
- `877c0f7` 2026-07-08 — **refactor(parser)**: split toc.rs (592) into single + paginated + utils
- `2a431a6` 2026-07-08 — **refactor(parser)**: split book.rs (434) into meta + cover
- `39ce8a2` 2026-07-08 — **refactor(parser)**: split chapter.rs (420) into parse + pagination

**Batch 4 / web**（2 个 commit）
- `2149c37` 2026-07-08 — **refactor(web)**: split misc.rs (336) into health + sources + settings (1/2)
- `3f5bb41` 2026-07-08 — **refactor(web)**: split download.rs (535) into download + tasks (2/2)

**gpui_app 拆分**
- `fe910dc` 2026-07-08 — **refactor(gpui_app)**: split themes.rs into embedded + user_dir + apply + init
- `2e17841` 2026-07-08 — **refactor(gpui_app)**: split root.rs (433) into logo + nav + notifications
- `2f16dd8` 2026-07-08 — **refactor(gpui_app)**: separate themes JSON assets from Rust source
  —— themes 改用 `rust-embed` 嵌入 `assets/themes/`，不再混在 `gpui_app/themes.rs`

**Batch 5 / observability**
- `dcd2407` 2026-07-08 — **feat(observability)**: add tracing + rust-doc to key public fns

#### 文档 / 大型图表

- `d3e45fb` 2026-07-08 — **docs(models)**: document field meanings; explain PO+DTO rationale (#10)
- `4131337` 2026-07-08 — **docs(crawler)**: add ASCII architecture diagram; explain why
  no service/ split (#12)
- `8e65652` 2026-07-08 — **docs**: enterprise README additions + refactor summary report (#16)

#### Lint cleanup（3 个 commit）

- `0397752` 2026-07-08 — **fix(lints)**: eliminate all 346 build warnings
- `426d9c1` 2026-07-08 — **docs**: simplify project structure to module-level in README
- `c8114f9` 2026-07-09 — **refactor(lints)**: move lint config from Cargo.toml into `src/lib.rs`

### Web 功能

- `4ffcce6` 2026-07-05 — **feat(web)**: JSON `/api/health` endpoint; drop once_cell;
  add `docker-compose` —— 健康检查 JSON 端点 + Docker Compose 编排
- `750464b` 2026-07-05 — **feat(web)**: harden lock boundaries, add web API tests,
  fix Dockerfile CMD —— web API 锁边界加固（不必要 `.lock().unwrap()` 全干掉）+ 集成测试
- `227f382` 2026-07-06 — **feat(web)**: strip qidian cookie from public settings;
  extract SSE; share lock helpers —— 公开 API 不再泄漏明文起点站 cookie

### Crawler 性能 + Wakeup notify

- `a1290be` 2026-07-06 — **perf(crawler)**: write chapters on completion — 边下边写，
  消除批量 sort + 额外 syscall（不再 buffer 全章节列表最后一次性写盘）
- `d676cd5` 2026-07-07 — **feat(crawler)**: wire up wakeup notify for immediate drain_loop response
- `df65a78` 2026-07-07 — **feat**: add wakeup notify to TOC pre-fetch, search, detail and update checks
  —— `tokio::sync::Notify` 替代轮询

### Fix

- `374e20f` 2026-07-06 — **fix**: remove duplicate `install_console_shim` call, narrow
  `pending_notifications` visibility, fix `unwrap_or` in `refresh_library_async` ——
  一次性清三个 desktop 启动期 bug

### Logger 默认切换

- `71f49f0` 2026-07-08 — **refactor(logger)**: flatten `logger/` dir to single `logger.rs`
  + flip default to Text —— 默认 Text 更适合本地开发排错，JSON 仍可通过
  `RUST_LOG_FORMAT=json` 环境变量启用

### 最终收尾（lint 严格）

- `ea5a3da` 2026-07-11 — **fix(lints)**: zero clippy warnings under `-D warnings` ——
  `cargo clippy --all-features --lib -- -D warnings` 严格通过

### 未单独提交的改动：Web i18n（in-flight）

> Web 模块多语言适配仍在进行中，未作为独立 commit 提交（按 [0.3.4] 模式
> 待 release commit 一并归并）。详细设计见 plan 文件
> `claude/plans/golden-toasting-dragon.md`，概要如下：

#### 背景

- 原 `src/web/` 所有错误响应硬编码中文
- `web-ui/` 前端 i18n 独立维护互不通信
- 后端 `zh-HK` locale tag 跟前端 `zh-TW` 漂移
- 错误 body 是 `{ error: { code, message } }` envelope 但前端只 `res.text()` 当 opaque 文本

#### 方案

- backend 翻译 + frontend 解析 envelope
- per-request locale 不动全局 atomic（不调 `rust_i18n::set_locale`）
- SSE 事件 `error`/`reason` 字段改 localized 文本（保持 SSE event shape 不变）

#### 改动概要

- **`src/web/locale.rs`**（新文件）：`Locale(&'static str)` axum extractor +
  `parse_accept_language` 手搓 BCP-47 解析（q-value + 前缀：`zh-HK/zh-Hans/zh-Hant → zh-TW`；
  `en-US/en-GB → en`） + 8 个单元测试
- **`src/i18n.rs`**：新增 `pub fn ts_for_locale(locale, key)` —— 直接调
  `crate::_rust_i18n_try_translate`，**不**读/写全局 atomic + 4 个新测试
- **i18n**: `zh-HK` → `zh-TW` 全量重命名（`locales/app.yml` + `src/i18n.rs` + tests；
  验证 `grep -rn 'zh-HK' .` 0 hits）
- **`locales/app.yml`** 新增 `WebErrors` 段（38 leaf key × 3 locale）+ 7 个
  UI 杂项文案（success body / SSE error 文案）
- **`src/web/error_code.rs`**: `ErrorCode::key()` + `message_for(locale)` 新 API，
  `message()` 保留为全局 fallback（Display / 日志）；`#[allow(dead_code)]` 防警告
- **`src/web/error.rs`**:
  - 3 新 `WebError` 变体：`DownloadPathEmpty` (3004) / `DownloadPathNotDir` (3005) /
    `TaskAlreadyFinished` (3006)
  - 新 `pub fn into_response_for_locale(self, locale) -> Response`
  - `ErrorBody` 加 `code_id: &'static str` 字段（前端 dispatch 按数字码）
  - `read_state_or_json` 现在返回 `WebError`
  - `read_state_or_sse` 签名变 `(label, locale, make_stream, f)`
  - 2 个新测试（`new_variants_translate_correctly` + `into_response_for_locale_translates_per_request`）
- **8 个 web handler 迁移**（library / sources / tasks / settings / search / download /
  book / health）：全部 `Result<_, WebError>` + `Locale` extractor；SSE handler 改
  `lock_failure_stream(status, msg, locale)` 多 locale 参数
- **`web-ui/src/lib/api.ts`**：`ApiError extends Error` class（`status/code/codeId/message`）
  + `toApiError(res)` 解析 envelope；非-OK 一律 `throw`
- **`web-ui/src/routes/settings.tsx`**：dispatch 改 `err instanceof ApiError && err.codeId === '3005'/'3004'`
- **`web-ui/src/hooks/use-download.ts`**：`useTranslation` + fallback 改 `t('common.error')`

### 验证

- `cargo clippy --all-features --lib -- -D warnings` ✓ clean
- `cargo test --all-features --lib` ✓ 529 passed
- `npx tsc --noEmit` ✓ pass
- `oxlint` ✓ pass
- 3 个 build target（默认 / `--no-default-features --features web` /
  `--no-default-features --features gui`）全 clean

---

## [0.3.4] - 2026-07-04

> 全部改动尚未作为独立 commit 提交（按 [0.3.3] 模式，待 release commit 一并归并）。

- **Cargo.toml** `version = "0.3.3"` → `"0.3.4"`；`cli::args.rs::VERSION_STRING`
  走 `env!("CARGO_PKG_VERSION")` 自动跟随；`./target/debug/so-novel-rs -V` → `so-novel-rs 0.3.4`

### CLI `--help` 多语言

- `locales/app.yml` 追加 `Cli:` 顶层 + ~25 leaf key（zh-CN / zh-HK / en），覆盖顶层 about/long_about/after_help + 4 个全局 flag + 3 个子命令 × about/after_help/per-arg help + sources 子子命令
- `src/i18n.rs` 新增 `pub fn locale_for(lang: Language) -> &'static str`（`zh-TW → zh-HK` 映射；之前在 `gpui_app/mod.rs` 是 cfg-gated，CLI 路径在 web-only build 下不能用，搬到中性模块）；`gpui_app/mod.rs` 改 `use crate::i18n::locale_for`
- `src/cli/args.rs` 新增 `pub(crate) fn build_localized_command(lang) -> clap::Command`（~120 行手搓 Command 树：因 clap 4 顶层 about/long_about/after_help 无 public setter）+ `pub(crate) fn subcommand_name(cmd) -> &'static str`
- `src/cli/mod.rs::run()` 控制流重排：`Cli::parse` → `--version`（locale 无关）→ `load_config` → `set_locale + invalidate_cache` → help dispatch（先 --help/无子命令，再 match 子命令）
- `src/cli/mod.rs::parse_or_help_fallback()` 解决 `search --help` 卡"required arguments not provided"：剥掉 `--help`/`-h` 后再 `try_parse_from`，失败兜底成空 Cli；带 `--help` 时手动 stub `cli.command` 让 find_subcommand 能定位子命令
- `src/cli/tests.rs` 新增 4 个 i18n 测试：`localized_command_matches_derive_structure`（断言手搓与 derive 的 arg IDs / subcommand 集合相等，守结构正确性）、`localized_command_about_changes_with_language`（三语种 about 全不相同 + 关键词包含）、`subcommand_name_maps_variants`、`locale_for_maps_to_rust_i18n_tags`
- 跨 build 验证：`cargo check`（默认） / `cargo check --no-default-features --features web` / `cargo test --lib cli::` 40 passed / `cargo test --lib` 370 passed
- 三语种 manual smoke：`--help` / `-h` / `search --help` / `sources list --help` / `--version` 在 en / zh-CN / zh-TW 各跑一遍，恢复 config.toml `language = "en"`

### `src/main.rs` 重构

- `src/main.rs` 128 → 18 行：只剩 crate attribute + `use anyhow::Result;` + 3 行 `main()`（收集 argv → `startup::detect` → `startup::dispatch`）
- `src/startup/mod.rs` 新建：`LaunchMode { Cli, Web { host, port }, Gui }` enum + `detect(args) -> LaunchMode`（搬原 `is_web` / `is_cli` 判定，single source of precedence）+ `attach_parent_console`（Windows Win32 原样搬 + non-Windows no-op stub）+ `run_gui`（cfg-gated，feature 缺失时 bail）+ `dispatch(mode) -> Result<()>`（CLI 路径直接 `cli::run()` 不 init_tracing；Web/Gui 先 attach_console → init_tracing → 分发，避免 `tracing_subscriber::init()` 双 init panic）
- `src/startup/web.rs` 新建：搬原 `run_web` + `parse_arg_value` 改名 `parse_arg_value_pub`（`pub(super)`，只允许 `startup::mod.rs` 用于 `detect` 解析 `--host` / `--port`）；`DEFAULT_WEB_HOST` / `DEFAULT_WEB_PORT` 弃用（detect 直接 hardcode）
- `src/lib.rs` +1 行 `pub mod startup;`（按字母序在 `persistent` 后、`util` 前）
- "feature not enabled" 的两个 bail（如 `--no-default-features --features gui` 跑 `--web`）从 main.rs 移到各自 `run` 函数内 cfg gate 后
- 跨 build 验证：3 个 `cargo build` target（默认 / `--no-default-features --features web` / `--no-default-features --features gui`）全 clean，0 warning；`cargo test --lib` 370 passed

### 文档

- `docs/CHANGELOG.md` 顶部新增 `## [0.3.4] - 2026-07-04` 区段（Added / Changed / Fixed 三分区，与 [0.3.3] 同结构）
- `README.md` 项目结构树更新：`cli.rs` 单文件 → `cli/` 目录；新增 `startup/`（含 `mod.rs` / `web.rs`）和 `i18n/` 模块；`main.rs` 注释改为 "18 行：crane attribute + 委托给 startup"
- `README.md` CLI 用法区加一句：`--help` / `-h` / 子命令 help 跟随 `config.toml [global].language`，与 GUI 同步

### Fix：release exe 黑控制台窗口

- **症状**：default features 编出的 `so-novel-rs.exe` 双击启动 GUI 时，gpui 主窗口之外**额外**弹一个黑色控制台窗口（用户报告 "release 打包的exe的console没隐藏"）
- **根因**：`src/startup/mod.rs::dispatch()` 对**非 CLI** 模式（含 `LaunchMode::Gui`）调 `attach_parent_console()`。GUI subsystem exe 从 Explorer 双击无父 console → `AttachConsole(ATTACH_PARENT_PROCESS)` 返回 0 → fallback `AllocConsole()` **运行时分配新 console 窗口**
- **修复**：把 `dispatch` 改成按 arm 显式控制；`Gui` arm 仅 `init_tracing()`，**不**调 `attach_parent_console`（`windows_subsystem = "windows"` 已保证进程无 console）；`Web` arm 行为不变（保留 AllocConsole fallback 让 Explorer 双击启动 Web 时能看 axum 日志）；CLI arm 行为不变
- **顺手改**：`dispatch` 的 doc comment + 文件头模块文档同步解释三 arm 区别
- **不在范围**：CLI 从 Explorer stdout 不可见（症状相反、独立 PR）；不改 `attach_parent_console` 自身（对 Web 是正确的）；不改 Web arm 行为；不改任何 cfg gate

### Fix (follow-up)：release exe CLI 无输出

- **症状**：同一 release 二进制跑 `so-novel-rs --help` / `-V` / `search ...` 等 CLI 子命令**完全没有 stdout 输出**（连 cmd 直跑也是），用户报告 "cli 没有任何输出了"
- **根因**：同上是 GUI subsystem exe —— Windows 默认把 stdio 关到 NUL（即使父进程是 cmd 也无效，跟 console subsystem 行为不同）。原 `dispatch` 把 CLI arm 从 attach_console 列表里跳过（pre-refactor main.rs 就这样），写 GUI mode fix 时也保留了这一"错误"行为
- **修复**：`src/startup/mod.rs::dispatch` 的 `LaunchMode::Cli` arm 加 `attach_parent_console();` —— GUI subsystem 下行为：
  - cmd / bash（父控制台存在）：`AttachConsole(ATTACH_PARENT_PROCESS)` 成功，stdout 直通父终端 ✓
  - Explorer 双击（无父控制台）：fallback `AllocConsole()`，用户看到帮助进 console 窗口 ✓
  - pipe 重定向（`foo | so-novel-rs ...`）：rust stdlib 用继承的 file handle 1，AttachConsole 不影响 pipe 行为 ✓
- **不动** `init_tracing` 调用时机 —— CLI 内部仍按 `--verbose` 自决（避免 `tracing_subscriber::init()` 双 init panic）
- **不动** Web / Gui arm 行为
- **验证**：跟 GUI fix 同套 `cargo check --release`（3 features 组合）clean；`cargo test --lib` 370 passed；手动 smoke `target\release\so-novel-rs.exe --help` / `-V` 在 cmd 下输出回到 cmd 终端

---

## [0.3.3] - 2026-06-25

> 5 个 commit + 大量未单独提交的 CLI / docs / Docker 工作

- `94ceecd` 2026-06-25 — **feat(cli)**: add `--verbose` flag; defer tracing init; default web host to loopback
- `38b32ad` 2026-06-24 — **refactor(config)**: split loader into `defaults` / `paths` / `toml_io` / `types` / `tests`; add `ListCache`; cleanup
- `6fc0daa` 2026-06-24 — **feat(sources)**: make URL column a clickable Link
- `5d297be` 2026-06-24 — **chore(rules)**: update 梦书中文 domain to `mcxs.la`
- `3acc1da` 2026-06-24 — **fix(search)**: clear search state when rule set changes

> v0.3.3 期间还包含本仓库一系列 **CLI 重构 + UX 升级 + docs / Docker** 改动（未作为独立 commit 提交）：
> - `src/cli.rs` 549 行 → `src/cli/` 7 文件模块拆分
> - 新增 `search` / `download` / `sources` 子命令（详情见 [docs/CLI.md](./CLI.md)）
> - `--quiet` / `-q` 全局 flag + `SetTrue` 手动分发（中文 `--help` / `--version`）
> - TTY 原地进度行 + Ctrl-C 接 `CancelToken`（Windows console 退出 prompt 修复）
> - `download --from` / `--to` 章节范围下载
> - `sources enable <ID>` / `disable <ID>` 书源管理
> - 搜索走 `search_streaming` + `tokio::spawn` 流式进度（参考 `streaming-search-lesson`）
> - `docs/CLI.md` 完整 CLI 用法 + 故障排查
> - `docs/WEB.md` Web 模式 + Docker 部署（多架构镜像 + `proxy_buffering off` 等坑）
> - `docs/BOOK_SOURCES.md` 书源说明 + CF 绕过（`CloudflareBypassForScraping` 容器）
> - `docs/CHANGELOG.md` + `CHANGELOG_ALL.md` 两份变更日志
> - `Dockerfile`（多阶段 + tini + 非 root uid 1000）+ `.dockerignore`
> - `.github/workflows/docker-release.yml`（BuildKit 多架构推 `ghcr.io/...:latest`）
> - `src/util/tty.rs` 抽出 TTY 进度 helper；`src/cli/helpers.rs` 改名 `util.rs`

---

## [0.3.2] - 2026-06-24

> 4 个 commit（含 release 自身）

- `04fe52c` 2026-06-24 — **release**: v0.3.2
- `722f8e9` 2026-06-23 — **refactor**: simplify code across entire codebase (-151 lines)
- `0b3550c` 2026-06-22 — **refactor**: simplify code across parser, ops, and CLI modules
- `dd12cda` 2026-06-22 — **refactor(tracing)**: add TraceId chain tracing, clean up duplicate logs, remove file logging
- `f581f8a` 2026-06-22 — **refactor(crawler)**: box `Book` in `Progress::BookResolved` to shrink enum size

---

## [0.3.1] - 2026-06-23

> 1 个 commit

- `722f8e9` 2026-06-23 — **refactor**: simplify code across entire codebase (-151 lines)

> 注：v0.3.1 的 release commit 在 v0.3.2 历史里；上面 v0.3.2 的列表里 `722f8e9`
> 是该版本的 release 主体。

---

## [0.3.0] - 2026-06-22

> 0 个独立 commit（v0.3.0 的工作以 v0.2.6 → v0.3.0 范围内的 commit 为准，见下）

---

## [v0.2.6 → v0.3.0] 期间 (2026-06-19 ~ 2026-06-22)

- `a5d9d0f` 2026-06-21 — **feat(rule+parser)**: sync upstream Java changes & inlined quanben5 `@js:` handling
- `8f0341d` 2026-06-21 — **chore**: include bundle/rules in release artifacts and fix shuhaige backreference regex
- `206bc5b` 2026-06-21 — **chore**: update dependencies, docs, and add Dockerfile
- `906e25e` 2026-06-21 — **refactor(web)**: optimize code structure and config management
- `5ca0983` 2026-06-20 — **docs**: add acknowledgment to freeok/so-novel original project
- `4478f73` 2026-06-20 — **refactor**: P0+P1+P2 safety/code-quality optimizations
- `95d5690` 2026-06-20 — **fix(export)**: dedup final output filenames on collision
- `f53da17` 2026-06-20 — **refactor(logging)**: extract tracing init to `src/logging.rs`
- `5bc6d66` 2026-06-20 — **fix(app)**: Phase 2 quality fixes bundled
- `2e7036d` 2026-06-20 — **docs**: Phase 2+3+4 final summary + audit status (Phase 5)
- `92a7133` 2026-06-20 — **privacy**: truncate search keyword in logs (Phase 4.3)
- `71c93fb` 2026-06-20 — **tracing**: add chapter-level tracing to export + pagination (Phase 4.2)
- `24b9962` 2026-06-20 — **test(parser,export)**: add missing unit tests (Phase 4.1)
- `3094d6c` 2026-06-20 — **perf(crawler)**: cancel token uses Notify for <1ms response (Phase 3.6)
- `d54f576` 2026-06-20 — **perf(export)**: BufWriter for epub/pdf writes + filename dedup (Phase 3.3)
- `8c6db6d` 2026-06-20 — **perf(parser)**: cache Regex / Selector compilation per string (Phase 3.2)
- `be76b9e` 2026-06-20 — **perf(http)**: 跨任务复用 reqwest client 连接池 + TLS session cache (Phase 3.1)
- `35cb0f7` 2026-06-20 — **docs(tasks)**: mark Phase 2 checklist complete + write `review.md`
- `bc410c1` 2026-06-20 — **fix(app)**: surface tokio runtime build failures via Result
- `ee78f4b` 2026-06-20 — **feat(log)**: purge old log files (>30d) at startup
- `9c10c4c` 2026-06-20 — **perf(db)**: single-statement `delete_finished` via `json_extract`
- `418f79c` 2026-06-20 — **perf(db)**: wrap `seed_from_default` inserts in a single transaction
- `196d413` 2026-06-19 — **chore**: bundled changes for v0.2.6
- `242fd6e` 2026-06-19 — **refactor(pages)**: 抽 `SearchDelegate` + `LibraryDelegate` 到独立 `delegate.rs` (PR6)
- `b0cd230` 2026-06-19 — **refactor(sources)**: split `sources.rs` 688 → 5 files (PR4)
- `50ac993` 2026-06-19 — **refactor(tasks)**: split `tasks.rs` 824 → 6 files (PR3)
- `65d9ba0` 2026-06-19 — **refactor(library)**: split `library.rs` 832 → 5 files (PR2)
- `314250b` 2026-06-19 — **refactor(search)**: split `search.rs` 1517 → 7 files (PR1)
- `24e0803` 2026-06-19 — **refactor**: 提取 `format_local_unix_secs` + `SourceHealth::classify`（PR0 helpers）

---

## [0.2.5] - 2026-06-19

> 6 个 commit

- `da28758` 2026-06-19 — **release**: v0.2.5
- `477320d` 2026-06-19 — **chore**: bundled changes (assets + library/search/i18n/config + deps)
- `809a547` 2026-06-19 — **docs**: rewrite README + split DISCLAIMER + auto-version badge
- `ee00235` 2026-06-19 — **feat(gpui_app)**: swap sidebar and titlebar background colors
- `5245010` 2026-06-19 — **feat(sources)**: show latency in ms instead of raw HTTP status
- `536266e` 2026-06-19 — **refactor(gpui_app)**: split settings page monolith into `settings/` module

---

## [0.2.4] - 2026-06-18

> 1 个 commit

- `5535773` 2026-06-18 — **release**: v0.2.4 — PDF 导出重写 + AGPL-3.0 协议 + README 现代化
- `3f4866a` 2026-06-18 — **feat**: 简繁中文自动转换 + URL 链接 + 更新按钮 + 书库删除无闪

---

## [0.2.3] - 2026-06-18

> 0 个独立 commit（变更随 v0.2.4 release 一起发布）

---

## [0.2.2] - 2026-06-18

- `4ff3e02` 2026-06-18 — **release**: v0.2.3
- `b937724` 2026-06-18 — **ci(release)**: Linux aarch64 改用 `ubuntu-24.04-arm` 原生 runner
- `3fc5cd1` 2026-06-18 — **release**: v0.2.2
- `53858ae` 2026-06-18 — **chore(deps)**: Cargo.lock 同步 rfd async-std feature 变化
- `0b8b44c` 2026-06-18 — **fix(deps)**: rfd 0.15 切到 async-std feature 修 Linux build 7 个 ashpd 编译错误
- `a0b422e` 2026-06-18 — **ci(release)**: Linux aarch64 改用 `ubuntu-24.04-arm` 原生 runner

---

## [0.2.1] - 2026-06-18

- `be8af8a` 2026-06-18 — **release**: 0.2.1
- `336581f` 2026-06-18 — **ci(release)**: 删 Cross.toml + 修 Linux vulkan 包 + 去掉 macOS x86

---

## [0.2.0] - 2026-06-18

> 跨多个工作日的大版本：多平台打包 + UI 改版 + i18n 全面化 + 日志系统

- `e18e6b2` 2026-06-18 — **release**: 0.2.0 多平台打包 + UI 改版 + i18n 全面化 + 日志系统
- `2b26ee6` 2026-06-17 — **feat(ui)**: 书源/搜索页 Tag 化改造 + 搜索结果行重排 + Enter 搜索
- `17268d8` 2026-06-17 — **feat(ui)**: 持久化 Sidebar 折叠状态
- `43ecc83` 2026-06-17 — **refactor(notification)**: toast 字段 + 4 个 `show_toast_*` 替换为 gpui-component Notification

---

## 早期版本

> 以下 commit 在 v0.2.0 之前，跨多个内部里程碑（"下载弹窗 + 选章"、
> "设计系统统一"、"书源数据库迁移"、"依赖升级"等），保留作为完整历史。

- `3af6aed` 2026-06-16 — **feat(download)**: 选章下载 + 下载弹窗 + crawler 两阶段拆分
- `a2a26a9` 2026-06-15 — **refactor(design_system)**: 统一按钮/输入框/弹窗组件 + 迁移配置到 `~/.sonovel/`
- `3da0a23` 2026-06-15 — **feat(search)**: 流式搜索进度 + **ui(toggle)**: 增强 iOS 风格开关
- `c4e3ac6` 2026-06-15 — **refactor**: 拆分 crates 与 `app.rs`
- `1e9e121` 2026-06-14 — **ui(sources)**: 卡片化 + 测速/添加按钮 + 删除（含二次确认）
- `3176ea4` 2026-06-14 — **ui(library)**: 卡片化列表 + 时间对齐到书名下方
- `84e5bb2` 2026-06-14 — **feat**: `config.toml` + 书源入 `sonovel.db` + Linux 打包工具链
- `8b21c67` 2026-06-14 — **deps**: 升级一批依赖大版本并修复 API 变更
- `dba9299` 2026-06-14 — **ui**: 集成 `material_icons` 并替换 emoji/手绘图标
- `6bdf1f4` 2026-06-14 — **ui**: 优化搜索/导航交互与样式
- `015edc0` 2026-06-18 — **fix(fs)**: Linux `sanitize_filename` 保留扩展名点并清理反斜杠

---

## 元数据

- **总提交数**：72（含 release commit）
- **首个 commit**：早于 2026-06-14（项目从 `freeok/so-novel` Java 移植启动）
- **当前版本**：`v0.3.3`（Cargo.toml + git tag）
- **CHANGELOG 规范**：[Keep a Changelog 1.1.0](https://keepachangelog.com/zh-CN/1.1.0/)
- **版本号规范**：[Semantic Versioning 2.0](https://semver.org/lang/zh-CN/)
- **commit 类型**：`feat` / `fix` / `refactor` / `perf` / `test` / `docs` /
  `chore` / `ci` / `ui` / `release`（前 6 个走 Conventional Commits）

## 进一步阅读

- [CHANGELOG.md](./CHANGELOG.md) — 对外摘要
- [CLI.md](./CLI.md) — CLI 用法
- [README.md](../README.md) — 项目总览
