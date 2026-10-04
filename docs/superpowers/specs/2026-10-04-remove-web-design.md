# 移除 Web 端实现 — 设计文档

- 日期：2026-10-04
- 状态：已确认（范围 / feature 收敛 / 可恢复性 / 注释边界 4 项决策均获用户批准）

## 背景与目标

项目当前是三形态：GUI（gpui-kit 桌面）、Web（axum 服务 + React 前端）、CLI。
Web 端维护成本高于实际价值（独立前端 monorepo、独立错误码体系与 i18n 段、
Docker 构建链路、一套 SSE→轮询任务模型），决定整体移除。

目标：仓库收敛为 **GUI + CLI** 两形态，且不留下孤儿代码、失效文档、
断链注释或已无消费者的构建配置。

## 已确认决策

| 决策点 | 结论 |
|---|---|
| 删除范围 | **全套移除**：Rust web 服务 + web-ui 前端 + web feature + Docker 全套 + docs/WEB.md + README/i18n 相关段落 |
| `bundle/web/` | **保留** —— 书源解析样例，零代码引用，调规则时的真实 HTML/JS 参考 |
| `gui` feature | **一并去掉**：删掉 web 后该 feature 只剩单值，其 cfg 双分支是纯复杂度 |
| 可恢复性 | **不打 tag / 分支** —— git history 永久保留，无需额外噪音 |
| `src/models/*` 注释 | **只改措辞，不动字段名** —— 字段已被桌面端消费，改名是纯 churn |

## 一、直接删除的代码

| 目标 | 规模 |
|---|---|
| `src/web/**` | 15 文件（`mod` / `routes` / `handlers`×9 / `error` / `error_code` / `locale` / `tests`） |
| `src/startup/web.rs` | 1 文件 |
| `web-ui/**` | 86 文件（Turborepo monorepo：`apps/web` + `packages/ui` + 锁文件 + 配置） |

合计 102 个被 git 跟踪的文件。

## 二、Cargo 与 feature 收敛

### Cargo.toml

- 删整个 `[features]` 段（`default = ["gui"]` / `gui` / `web`）
- `gpui-kit`、`rfd` 由 `optional = true` 转必选依赖
- 删除依赖：`axum`、`axum_session`、`tower-http`、`rust-embed`、`mime_guess`、
  `async-stream`、`futures`
- 删除 dev-dep：`tower`（`axum::Router` 的 `oneshot` 测试专用）
- **保留** `tokio` —— crawler / parser / CLI / 桌面 model 都在用（20+ 文件），
  与 web 无关

### build.rs

删 `CARGO_FEATURE_WEB` 分支、`SO_NOVEL_SKIP_WEB_BUILD` 逻辑、`FRONTEND_DIST`
常量、`run_bun_build()` 函数、5 行 web-ui 的 `rerun-if-changed`。
保留 Windows 图标资源段（`winres` + `assets/logo.ico`）。

### cfg 门控收敛

| 位置 | 改动 |
|---|---|
| `src/lib.rs:59` | 删 `#[cfg(feature = "web")] pub mod web;` |
| `src/lib.rs:47` | `pub mod desktop;` 去掉 `#[cfg(feature = "gui")]` |
| `src/main.rs:7-10` | `windows_subsystem` 的 cfg 条件去掉 `feature = "gui"` |
| `src/i18n.rs:46-48` | `gui` / `not(gui)` 双分支合并为单实现 |
| `src/desktop/model/events.rs` | 4 处 `#[cfg(feature = "gui")]` 去掉 |
| `src/startup/mod.rs:72,78` | `run_gui` 的 `not(feature = "gui")` bail 版本删除 |

## 三、启动层

`src/startup/mod.rs`：

- `LaunchMode` 删除 `Web { host, port }` 变体，只剩 `Cli` / `Gui`
- `detect()` 删除 `--web` argv 分支与 `SO_NOVEL_WEB` env 分支，
  收敛为「除 binary name 外有 arg → `Cli`，否则 `Gui`」
- `dispatch()` 删除 Web 分支；`attach_parent_console()` 仍在 Cli 分支调用
  （Windows GUI subsystem exe 的输出依赖它）
- 随之失效的 `--host` / `--port` 参数解析（原在 `startup/web.rs`）一并删除

## 四、连带孤儿清理

删除 web 后失去唯一消费者的代码：

| 位置 | 处理 | 说明 |
|---|---|---|
| `core::library::extension_to_content_type` | 删 | 当前已无调用方（web 也没用） |
| `core::library::list_library_entries` | 删 | 仅 web `handlers/library.rs` 用 |
| `core::library::OpenFileError` | 删 | 仅 web 用 |
| `core::library::safe_file_path` | 删 | 仅 web 用 |
| `core::library::{LibraryEntry, SUPPORTED_LIBRARY_EXTS, from_path}` | **保留** | 桌面书库页在用 |
| `core::sources::find_rule_by_id_cloned` | 删 | 仅 web 用 |
| `core::sources::find_rule_by_url` | 删 | 当前无任何调用方 |
| `core::sources` 其余函数 | 保留 | CLI / 桌面在用 |
| `src/i18n.rs` `WEB_ERROR_KEYS` 常量 | 删 | 附 2 个测试一并删 |
| `locales/app.yml` `WebErrors:` 段 | 删 | 约 40 key × 3 语言 |

> **注**：本项目是 lib crate（`src/lib.rs`），`pub` 孤儿**不会**触发 `dead_code`
> 警告。上述清理是必要的可读性维护，不是编译前置条件 —— 但留着就是给后来者的错误地图。

### 注释与文档链接修正

`rustdoc::broken_intra_doc_links` 是 warn 级，而质量门跑
`clippy --all-targets -- -D warnings`，因此指向 `crate::web::*` 的
intra-doc link 不修会直接挂门：

- `src/crawler/resolve.rs:22` —— `[crate::web::error::WebError]` 链接改写
- `src/error.rs:3-6` —— 模块头提 `WebError` 的措辞
- `src/core/library.rs:1-8` —— 模块头「Web `handlers/library.rs` 与桌面…」改写
- `src/models/{book,chapter,mod}.rs` —— 提 web-ui DTO 的措辞改写（不动字段名）
- `src/config/toml_io.rs:197` —— `web::handlers::settings` 引用改写

## 五、构建与部署链路

**删除**：

- `Dockerfile` —— 唯一用途是构建 `--no-default-features --features web` 的
  web-only 二进制
- `docker-compose.yml`
- `.dockerignore`
- `.github/workflows/docker-release.yml` —— 发布 ghcr 镜像，服务对象即 web

**保留**：

- `.github/workflows/release.yml` —— 走 `cargo build --release`（default
  features），本来就不含 web，无需改动
- `scripts/package-linux.sh`、`scripts/install-hooks.sh`、`.githooks/pre-commit`

## 六、文档

| 文件 | 改动 |
|---|---|
| `docs/WEB.md` | 整文件删除（455 行） |
| `README.md` | 删「🌐 Web 模式」（L175-194）「🐳 Docker 部署」（L196-209）两节；结构树删 `web-ui/`（L74）与 `src/web/`（L81）行；L79「桌面 / Web / CLI 三端共享」改两端；L91「分层」段整段改写（去掉 `web/` 是 axum API 层与 `WebError` per-request locale 的描述）；L237「边界(CLI / Web)才转 anyhow」改「边界(CLI)」；L25 截图说明里「随 Web 前端迁移 shadcn 失效」的过时理由改写。顶部导航（L17）本就无 Web 锚点，不动 |
| `docs/CLI.md` | 删 line 9 的 `so-novel-rs --web` 表格行；删 line 277 关于 `SO_NOVEL_WEB` 的说明 |
| `docs/BOOK_SOURCES.md` | 删 line 179 指向 `WEB.md` 的链接 |
| `docs/CHANGELOG.md` | 新增 `[Unreleased] → Removed` 条目记录本次移除 |
| `docs/CHANGELOG_ALL.md` | **不动** —— 历史记录 |
| `tasks/todo.md` / `tasks/lessons.md` | 按项目规约在实现后追加 review / lesson |

## 七、明确保留

- `bundle/web/`（`chapter.html` / `cover.jpg` / `js/*.js`）—— 书源解析样例
- CLI 模式全部：`src/cli/**`、`search` / `download` / `sources` 子命令
- `src/core/` 其余共享层：`bootstrap` / `search` / `config_helpers` /
  `download_task` / `async_progress` / `update` / `sources`（部分）
- `src/desktop/**`、`src/crawler/**`、`src/parser/**`、`src/export/**`
- `screenshots/`（桌面截图）

## 八、验证方式

实现完成后按顺序跑：

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
cargo tree -e normal | grep -i "axum\|rust-embed"   # 应无输出
```

预期：`cargo test --lib` 由 **370 passed / 4 ignored** 降至 **368 passed**
（减少的是 `src/i18n.rs` 里 2 个非 feature-gated 的 `WebErrors` 翻译测试）。
原 `cargo test --lib --features web` 的 382 = 370 + 12 个 web API 测试，
该命令连同 feature 一起不复存在。fmt / clippy 保持 0 警告。

## 风险与对策

| 风险 | 对策 |
|---|---|
| 漏删 cfg 门控导致 `--no-default-features` 类构建残留 | 全仓 grep `feature = "web"` / `feature = "gui"` / `SO_NOVEL_WEB` / `web-ui`，确认 0 命中 |
| intra-doc link 断裂挂 clippy gate | 全仓 grep `crate::web` 逐一改写 |
| 删除的 core 函数其实有隐藏调用方 | 每个候选函数先 grep 全仓调用方，再删 |
| `bundle/web` 被误删 | 删除清单逐条对照本文档「明确保留」节 |
