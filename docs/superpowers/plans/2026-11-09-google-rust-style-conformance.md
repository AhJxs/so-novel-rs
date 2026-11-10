# Google Rust Style Guide 全仓修订 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 按 Google Rust Style Guide 对 `so-novel-rs` 做全仓一致性修订，覆盖格式化、导入组织、模块结构、命名、文档、API 设计、所有权与实现细节，并用 CI 固化。

**Architecture:** 按阶段推进：先做全仓机械格式化与导入整理，再拆分非叶模块、统一命名、补文档，随后落地 API/类型规则、所有权与深拷贝消除，最后收敛 lint 与 CI。每个任务保持行为不变，以现有 457 个测试为回归基线；仅 Task 10 有意收敛 `UpdateCheckResult` 契约。

**Tech Stack:** Rust 2024 edition、`gpui-kit 0.9`、tokio、smol、reqwest、scraper、boa、rustfmt（nightly 仅用于 fmt）、clippy、GitHub Actions。

**Spec:** `docs/superpowers/specs/2026-11-09-google-rust-style-conformance-design.md`

## Global Constraints

- 分支：`style/google-rust-guide`（已创建）。
- 构建/测试使用 stable `rustc 1.98.1`；格式化使用 nightly rustfmt（`cargo +nightly fmt`）。
- 每个任务结束必须通过：
  - `cargo +nightly fmt --all -- --check`
  - `cargo clippy --all-targets -- -D warnings`
  - `cargo test --all-targets`
- 基线：457 passed / 4 ignored / 0 failed；已知 flake：`i18n::tests::ts_for_locale_does_not_mutate_global`、`core::library::tests::list_skips_unsupported_extensions`。失败时先看用例名，并与基线对照，不误判。
- 行为保持不变，除 Task 10 的 `UpdateCheckResult` 契约收敛。
- 注释/文档保持中文；只修语法、完整性与一致性。
- 不新增 `unsafe`；生产代码不得出现 `panic::catch_unwind`；生产代码不新增 `unwrap()`。
- 指南明确例外（spec §8）不得硬拆：GPUI `Arc<RenderImage>` / `Rc` listener / `Rc<ThemeConfig>`、`tokio::spawn` + `Arc<Semaphore>`、`CancelToken` / `notify`、全局解析缓存、`ListCache`、PDF `RefCell`、测试脚手架。
- 每个任务单独提交；提交信息使用 Conventional Commits（`style:` / `refactor:` / `docs:` / `test:` / `ci:`）。
- 修改前确认 GUI 未运行：`tasklist | grep -i so-novel` 无输出。

---

## 文件结构总览（本计划创建/修改的关键文件）

**创建**
- `src/db/dao.rs`、`src/db/atomic_write.rs`
- `src/desktop/actions.rs`、`src/desktop/run.rs`
- `src/desktop/model/app_model.rs`
- `src/config/global.rs`
- `src/desktop/pages/search/search_page.rs`
- `src/desktop/pages/sources/sources_page.rs`
- `src/desktop/pages/library/library_page.rs`
- `src/desktop/pages/tasks/tasks_page.rs`
- `src/desktop/pages/settings/settings_page.rs`
- `docs/google-rust-style-conformance.md`
- `.github/workflows/ci.yml`
- `clippy.toml`

**修改**
- `rustfmt.toml`
- `src/lib.rs`
- `src/error.rs`
- `src/config/types.rs`
- `src/config/mod.rs`
- `src/core/{update.rs,search.rs,bootstrap.rs,sources.rs}`
- `src/crawler/{resolve.rs,download.rs,search.rs,health.rs,download_options.rs}`
- `src/http/{clients.rs→http_clients.rs,client.rs,cf.rs→cloudflare.rs,ua.rs→user_agent.rs,mod.rs}`
- `src/models/rule.rs`、`src/models/task_record.rs→download_task_record.rs`、`src/models/mod.rs`
- `src/parser/{cache.rs,filter.rs}`
- `src/utils/zhconv.rs`
- `src/desktop/{mod.rs,nav.rs,root.rs}`、`src/desktop/model/*`、`src/desktop/pages/*`

---

### Task 1: rustfmt 配置与全仓格式化

**Files:**
- Modify: `rustfmt.toml`
- Modify: 全仓 `.rs`（仅格式）

**Interfaces:**
- Consumes: 无
- Produces: 后续所有任务依赖的导入分组与格式基线

- [ ] **Step 1: 更新 `rustfmt.toml`**

```toml
# rustfmt 配置。fmt 需 nightly（imports_granularity / group_imports 等仍是 unstable），
# 构建与测试仍用 stable。CI 的 fmt 作业使用 `cargo +nightly fmt`。
edition = "2024"
max_width = 100
use_field_init_shorthand = true
use_try_shorthand = true

reorder_imports = true
imports_granularity = "Module"
group_imports = "StdExternalCrate"

hex_literal_case = "Lower"
format_macro_matchers = true
format_code_in_doc_comments = true

# 中文注释不重排：CJK 宽度与 rustfmt 的列宽计算不稳定。
# wrap_comments = true
# comment_width = 98
# normalize_comments = false

chain_width = 60
fn_params_layout = "Tall"
```

- [ ] **Step 2: 确认 nightly rustfmt**

Run: `rustup toolchain list | grep nightly || rustup toolchain install nightly --component rustfmt`
Expected: 列出或安装 `nightly-*-x86_64-pc-windows-msvc`。

- [ ] **Step 3: 全仓格式化**

Run: `cargo +nightly fmt --all`
Expected: 仅格式/导入顺序变化，无代码逻辑变化。

- [ ] **Step 4: 验证格式与编译**

Run:
```bash
cargo +nightly fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
```
Expected: fmt 退出 0；457 passed / 4 ignored。

- [ ] **Step 5: 提交**

```bash
git add -A
git commit -m "style: apply rustfmt with Google guide options"
```

---

### Task 2: 首方自由函数改为模块限定调用

**Files:**
- Modify: `src/export/render.rs`
- Modify: `src/http/client.rs`
- Modify: `src/db/tasks_init.rs`
- Modify: `src/config/toml_io.rs`
- Modify: `src/db/tasks.rs`
- Modify: `src/db/sources_config.rs`
- Modify: `src/crawler/search.rs`
- Modify: `src/core/sources.rs`
- Modify: `src/parser/filter.rs`
- Modify: `src/http/fetch.rs`
- Modify: `src/parser/book/meta.rs`
- Modify: `src/crawler/download.rs`
- Modify: `src/desktop/mod.rs`
- Modify: `src/export/{txt.rs,html.rs,epub.rs,md.rs,exporter.rs,pdf/document.rs}`

**Interfaces:**
- Consumes: Task 1 的导入分组
- Produces: 后续任务使用 `module::function()` 调用风格

- [ ] **Step 1: 逐文件改为模块限定**

变换规则：把 `use crate::foo::bar_fn;` 改为 `use crate::foo;`，调用点写 `foo::bar_fn(...)`；`use super::defaults::default_template_doc;` 改为 `use super::defaults;` + `defaults::default_template_doc(...)`。

必须处理的具体位置：

| 文件 | 自由函数 |
|---|---|
| `src/export/render.rs` | `filter_chapter`、`format_chapter` |
| `src/http/client.rs` | `resolve_proxy_url` |
| `src/db/tasks_init.rs` | `now_unix_secs` |
| `src/config/toml_io.rs` | `default_template_doc` |
| `src/db/tasks.rs`、`src/db/sources_config.rs` | `write_atomically` |
| `src/crawler/search.rs` | `search_one` |
| `src/core/sources.rs` | `load_active_rules` |
| `src/parser/filter.rs` | `clean_invisible_chars`、`remove_tags` |
| `src/http/fetch.rs` | `decode_response_bytes`、`random_user_agent`、`origin_or_self` |
| `src/parser/book/meta.rs` | `content_type_for`、`extract_cover_url`、`maybe_replace_cover` |
| `src/crawler/download.rs` | `exporter_for`、`write_single_chapter`、`build_book_dir_name`、`render_chapter` |
| `src/desktop/mod.rs` | `locale_for` |
| `src/export/*.rs`（6 文件） | `sanitize_filename` |

示例：

```rust
// before
use crate::utils::fs::sanitize_filename;
let name = sanitize_filename(title);

// after
use crate::utils::fs;
let name = fs::sanitize_filename(title);
```

- [ ] **Step 2: 编译并修正所有调用点**

Run: `cargo check --all-targets`
Expected: 编译通过；无未使用 import。

- [ ] **Step 3: 验证**

Run:
```bash
cargo +nightly fmt --all
cargo +nightly fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

- [ ] **Step 4: 提交**

```bash
git add -A
git commit -m "style: qualify first-party function calls with module names"
```

---

### Task 3: 类型名去模块路径

**Files:**
- Modify: `src/http/clients.rs`（`reqwest::Client`）
- Modify: `src/crawler/cover_updater.rs`（`scraper::ElementRef`）
- Modify: `src/desktop/logo.rs`（`image::ImageReader`、`image::Frame`、`gpui_kit::Pixels`）
- Modify: `src/desktop/components/pagination.rs`（`gpui_kit::ViewElement`）
- Modify: `src/desktop/root.rs`（`gpui_kit::FocusHandle`、`gpui_kit::Div`）
- Modify: `src/export/html.rs`（`zip::ZipWriter`）、`src/export/epub.rs`（`zip::ZipArchive`）
- Modify: `src/export/pdf/chapters.rs`（`regex::Captures`）
- Modify: `src/config/toml_io.rs`（`toml_edit::Item`、`toml_edit::Table`）
- Modify: `src/core/sources.rs`（`url::Url`）
- Modify: `src/core/update.rs`、`src/desktop/themes/embedded.rs`（`serde_json::Value`）
- Modify: 其余 40+ 文件中的 `std::time::*` / `gpui_kit::*` 限定类型

**Interfaces:**
- Consumes: Task 2
- Produces: 统一「类型不加模块路径」风格

- [ ] **Step 1: 模块顶部导入类型，代码中使用短名**

```rust
// before
fn client(&self) -> reqwest::Client { ... }
fn parse() -> serde_json::Value { ... }

// after
use reqwest::Client;
use serde_json::Value;

fn client(&self) -> Client { ... }
fn parse() -> Value { ... }
```

对 `std::time::Instant` / `SystemTime` / `Duration`，若文件内已导入则不重复；否则加 `use std::time::{Duration, Instant, SystemTime};` 并替换限定路径。

- [ ] **Step 2: 处理歧义**

若同名类型冲突（例如 `crate::error::Result` 与 `std::result::Result`），保留限定或使用别名：

```rust
use std::io::Error as IoError;
```

- [ ] **Step 3: 验证并提交**

Run:
```bash
cargo +nightly fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "style: import type names unqualified"
```

---

### Task 4: trait 导入下移到使用处

**Files:**
- Modify: `src/js/runtime.rs`
- Modify: `src/logger.rs`

**Interfaces:**
- Consumes: Task 3
- Produces: 文件级 trait 导入只保留“全文件高频使用”的情况

- [ ] **Step 1: `src/js/runtime.rs`**

删除模块顶部 `use std::fmt::Write as _;`，在真正调用 `write!`/`writeln!` 的函数内添加同名导入。

- [ ] **Step 2: `src/logger.rs`**

删除模块顶部 `use std::str::FromStr;`，并把实现写成 `impl std::str::FromStr for LogFormat`；`SubscriberExt` / `SubscriberInitExt` 若只在 `init_with_format` 使用，则移入该函数。

- [ ] **Step 3: 验证并提交**

```bash
cargo +nightly fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "style: scope trait imports to use sites"
```

---

### Task 5: 拆分非叶模块（db / desktop / model / config）

**Files:**
- Create: `src/db/dao.rs`、`src/db/atomic_write.rs`
- Modify: `src/db/mod.rs`
- Create: `src/desktop/actions.rs`、`src/desktop/run.rs`
- Modify: `src/desktop/mod.rs`
- Create: `src/desktop/model/app_model.rs`
- Modify: `src/desktop/model/mod.rs`
- Create: `src/config/global.rs`
- Modify: `src/config/mod.rs`

**Interfaces:**
- Consumes: Task 2–4
- Produces: 非叶模块只含 `mod` 声明与 `pub use`；公开路径保持不变（`crate::db::DaoError` 等继续可用）

- [ ] **Step 1: 拆 `src/db/mod.rs`**

`src/db/dao.rs` 接收：`DaoError` enum、`impl From<DaoError> for AppError`、`dao_error_*` 测试。
`src/db/atomic_write.rs` 接收：`write_atomically` 及其 3 个测试。
`src/db/mod.rs` 保留：

```rust
mod atomic_write;
mod dao;
mod rules;
mod sources_config;
mod tasks;
mod tasks_init;

pub use atomic_write::write_atomically;
pub use dao::DaoError;
pub use rules::{ /* 现有 re-export 原样保留 */ };
pub use sources_config::SourcesConfig;
pub use tasks::{load as load_tasks, save as save_tasks, save_with_trim};
pub use tasks_init::load_tasks_from_file;
```

调整 `use`：`dao.rs` 需要 `use super::RulesError;`，`atomic_write.rs` 使用 `use std::path::Path;`。

- [ ] **Step 2: 拆 `src/desktop/mod.rs`**

`src/desktop/actions.rs` 接收 `actions!(desktop, [...])` 与 9 个 `impl Eq for ...`。
`src/desktop/run.rs` 接收 `pub fn run() -> Result<()>` 及其文档。
`src/desktop/mod.rs` 保留：

```rust
mod actions;
mod drain_loop;
mod logo;
pub mod model;
mod nav;
mod notifications;
mod pages;
mod root;
pub mod themes;
pub mod components;

pub use actions::{Quit, ShowSearch, ShowTasks, ShowLibrary, ShowSources, ShowSettings, NextPage, PrevPage, ToggleSidebar};
pub use nav::{NavPage, register_key_bindings};
pub use root::RootView;
pub use run::run;
```

注意：`actions!` 生成的类型若在 `actions.rs` 中，`actions!` 的第一个参数仍写 `desktop`，以保持类型路径为 `desktop::X`。

- [ ] **Step 3: 拆 `src/desktop/model/mod.rs`**

`src/desktop/model/app_model.rs` 接收 `AppModel` 结构体、`impl AppModel`（约 203 行）及相关私有字段/方法。
`src/desktop/model/mod.rs` 保留所有 `mod` 声明与 `pub use`，并新增 `pub use app_model::AppModel;`；删除原 `pub struct AppModel` 与 impl。
若 `AppModel` 的私有字段被同模块其他文件通过 `super::AppModel` 访问，保持 `pub(crate)` 可见性不变。

- [ ] **Step 4: 拆 `src/config/mod.rs`**

`src/config/global.rs` 接收 `GLOBAL`、`GLOBAL_VIEW`、`set_global`、`global`、`validate_global`。
`src/config/mod.rs` 保留 `mod defaults; mod global; mod paths; mod toml_io; mod types;` 与原 `pub use`，新增 `pub use global::{global, set_global, validate_global};`。
`global.rs` 内部使用 `use super::{AppConfig, ConfigError};`。

- [ ] **Step 5: 验证并提交**

```bash
cargo +nightly fmt --all
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: split implementation out of non-leaf modules"
```

---

### Task 6: 拆分页面模块

**Files:**
- Create: `src/desktop/pages/search/search_page.rs`、`src/desktop/pages/sources/sources_page.rs`、`src/desktop/pages/library/library_page.rs`、`src/desktop/pages/tasks/tasks_page.rs`、`src/desktop/pages/settings/settings_page.rs`
- Modify: 对应的 `mod.rs`

**Interfaces:**
- Consumes: Task 5
- Produces: `SearchPage` / `SourcesPage` / `LibraryPage` / `TasksPage` / `SettingsPage` 的公开路径不变

- [ ] **Step 1: 每个页面按同一模式拆分**

以 `search` 为例：

```rust
// src/desktop/pages/search/mod.rs
//! 搜索页。
mod delegate;
mod search_page;
mod source_select;

pub use search_page::SearchPage;
```

`search_page.rs` 顶部保留原 `mod.rs` 的 `//!` 之外的 imports、`pub struct SearchPage`、`impl SearchPage` 与 `#[cfg(test)] mod tests`。其他页面同理。

- [ ] **Step 2: 修正可见性**

移动后 `pub(super)` 的 `super` 语义变化：把只供页面内部使用的项改为 `pub(super)`（现在指向页面模块）或 `pub(crate)`；编译器报错处逐一修正。
`settings/mod.rs` 的 `make_state!` 暂时原样移动（Task 16 再改为函数）。

- [ ] **Step 3: 验证并提交**

```bash
cargo +nightly fmt --all
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: move page implementations into leaf modules"
```

---

### Task 7: 叶子模块重命名

**Files:**
- Rename: `src/http/cf.rs` → `src/http/cloudflare.rs`
- Rename: `src/http/ua.rs` → `src/http/user_agent.rs`
- Rename: `src/models/task_record.rs` → `src/models/download_task_record.rs`
- Rename: `src/http/clients.rs` → `src/http/http_clients.rs`
- Modify: `src/http/mod.rs`、`src/models/mod.rs`、所有 `use`/路径引用

**Interfaces:**
- Consumes: Task 6
- Produces: 模块名与主类型一致；`HttpClients` 的公开路径仍为 `crate::http::HttpClients`

- [ ] **Step 1: `git mv` 重命名**

```bash
git mv src/http/cf.rs src/http/cloudflare.rs
git mv src/http/ua.rs src/http/user_agent.rs
git mv src/models/task_record.rs src/models/download_task_record.rs
git mv src/http/clients.rs src/http/http_clients.rs
```

- [ ] **Step 2: 更新 `mod` 与 `pub use`**

`src/http/mod.rs`：

```rust
mod cloudflare;
mod http_clients;
mod user_agent;

pub use http_clients::HttpClients;
pub use user_agent::random_user_agent;
```

`src/models/mod.rs`：`mod download_task_record;` + `pub use download_task_record::DownloadTaskRecord;`。
把 `cf::` / `ua::` / `task_record::` / `clients::` 的全仓引用改名；`random_ua` → `random_user_agent`（与 Task 8 一并处理也可）。

- [ ] **Step 3: 验证并提交**

```bash
cargo +nightly fmt --all
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: name leaf modules after their primary item"
```

---

### Task 8: 命名规范（配置类型、上下文、缩写、生命周期）

**Files:**
- Modify: `src/config/types.rs`、`src/config/mod.rs` 及所有引用（约 30 文件）
- Modify: `src/desktop/model/ops/mod.rs`、`src/desktop/pages/settings/ctx.rs`
- Modify: `src/config/toml_io.rs`、`src/http/url_join.rs`、`src/http/user_agent.rs`
- Modify: `src/http/http_clients.rs`（`from_cfg`）
- Modify: `src/core/download_task.rs`（`from_record`）
- Modify: `src/http/fetch.rs`、`src/export/pdf/document.rs`

**Interfaces:**
- Consumes: Task 7
- Produces: 新类型名/函数名；serde 字段名不变

- [ ] **Step 1: 类型重命名**

| 旧 | 新 |
|---|---|
| `GlobalCfg` | `GlobalConfig` |
| `DownloadCfg` | `DownloadConfig` |
| `SourceCfg` | `SourceConfig` |
| `CrawlCfg` | `CrawlConfig` |
| `CookieCfg` | `CookieConfig` |
| `ProxyCfg` | `ProxyConfig` |
| `OpsCtx<'a>` | `OpsContext<'a>` |
| `PageCtx<'a>` | `PageContext<'a>` |

这些是 Rust 类型名，不影响 serde 字段；全仓替换后 `cargo check` 驱动修正。

- [ ] **Step 2: 函数/模块重命名**

| 旧 | 新 |
|---|---|
| `t_item` / `t_table` / `t_str` / `t_bool` / `t_int` / `t_float` | `toml_item` / `toml_table` / `toml_str` / `toml_bool` / `toml_int` / `toml_float` |
| `abs_url` | `absolute_url` |
| `random_ua` | `random_user_agent` |
| `ProxySignature::from_cfg` | `ProxySignature::from_config`，并加 `impl From<&AppConfig> for ProxySignature` |
| `AppConfig::with_defaults` | `AppConfig::new`；`impl Default for AppConfig { fn default() -> Self { Self::new() } }` |
| `DownloadTask::from_record` | 保留方法，并加 `impl From<DownloadTaskRecord> for DownloadTask` |

- [ ] **Step 3: 描述性生命周期**

```rust
// src/http/fetch.rs
pub struct FetchRequest<'request> { ... }
pub enum HttpMethod<'raw> { ... }

// src/export/pdf/document.rs
struct Paginator<'pdf> { ... }

// src/desktop/model/ops/mod.rs
pub struct OpsContext<'app> { ... }

// src/desktop/pages/settings/ctx.rs
pub(super) struct PageContext<'page> { ... }
```

- [ ] **Step 4: 验证并提交**

```bash
cargo +nightly fmt --all
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: align names with Google Rust style"
```

---

### Task 9: 文档补全与语法修复

**Files:**
- Modify: `src/lib.rs`（移除 `#![allow(missing_docs)]`）
- Modify: 约 40 文件中的 118 个公开项
- Modify: `src/js/mod.rs`、`src/parser/mod.rs`、`src/desktop/run.rs`、`src/desktop/themes/mod.rs`
- Modify: `src/logger.rs`、`src/config/defaults.rs`
- Modify: `src/desktop/model/search_state.rs`、`src/models/content_type.rs`

**Interfaces:**
- Consumes: Task 8 的最终命名
- Produces: 所有公开项有 `///` 或定向 allow；模块顶部有 `//!`

- [ ] **Step 1: 移除 allow 并让 clippy 列出缺口**

```rust
// src/lib.rs
#![deny(unsafe_code)]
// 删除 #![allow(missing_docs)]
```

Run: `cargo clippy --lib -- -W missing_docs 2>&1 | grep 'missing documentation'`
Expected: 列出 118 个位置（排除 `pub mod`，其 `//!` 已满足）。

- [ ] **Step 2: 按类别补文档**

- `AppError` 构造器：每个一行，说明错误域，例如 `/// 配置加载/校验失败。`
- `desktop/themes/embedded.rs` 主题常量：`/// Catppuccin Latte 浅色主题定义。`
- `config/types.rs` 公开字段/枚举：说明取值与默认行为。
- `db/rules/constants.rs`：说明规则 JSON 字段含义。
- `models/rule.rs`：说明字段用途，不重复字段名。

无信息量的项使用定向 allow：

```rust
#[allow(missing_docs, reason = "self-explanatory newtype")]
pub struct TraceId(u64);
```

- [ ] **Step 3: 修语法问题**

- `src/js/mod.rs:1`：修正反引号配对，例：`` //! JavaScript 引擎（boa_engine），对应 `@js:` 后处理。``
- `src/parser/mod.rs:1-2`：把跨行反引号合并为单行或成对闭合。
- `src/desktop/run.rs`（原 `desktop/mod.rs:56-59`）：删除挂在 `use crate::i18n::locale_for;` 上的 `///`，改为普通 `//` 注释。
- `src/desktop/themes/mod.rs:6`：把第二个 `//!` 块并入文件顶部连续块。
- `src/logger.rs:47`：把 panic 说明从 `# Errors` 移到 `# Panics`。
- `src/config/defaults.rs:24`：为 `default_template_doc()` 增加 `# Panics`。
- `src/desktop/model/search_state.rs`、`src/models/content_type.rs`：文件顶部补 `//!`。

- [ ] **Step 4: 验证并提交**

```bash
cargo +nightly fmt --all
cargo clippy --lib -- -D warnings
cargo doc --no-deps
cargo test --all-targets
git add -A
git commit -m "docs: document public items and fix doc syntax"
```

---

### Task 10: 去掉 `impl Into` 参数

**Files:**
- Modify: `src/error.rs`
- Modify: `src/desktop/model/mod.rs`（`push_*`）
- Modify: `src/desktop/components/{status_badge.rs,page_header.rs,empty_state.rs}`
- Modify: `src/config/toml_io.rs`（私有 `set_item`）
- Modify: 所有调用点（约 50 处）

**Interfaces:**
- Consumes: Task 9
- Produces: `AppError::config(String)`、`AppModel::push_info(String)`、`StatusBadge::new(SharedString)` 等具体签名

- [ ] **Step 1: 改签名**

```rust
// src/error.rs
pub fn config(msg: String) -> Self { Self::Config(msg) }
// http / parse / db / js / business / invalid / not_found / conflict / internal 同理

// src/desktop/model/mod.rs
pub fn push_info(&mut self, message: String) { ... }
// push_success / push_warning / push_error / push_open_link 同理

// src/desktop/components/status_badge.rs
pub fn new(label: SharedString, ...) -> Self { ... }
```

- [ ] **Step 2: 编译驱动修调用点**

Run: `cargo check --all-targets`
对每个 `&str` 字面量加 `.to_string()`；对 `t!(...)` / `format!(...)` 结果保持原样；对 `SharedString` 调用点补 `.into()`/`SharedString::from(...)`。

- [ ] **Step 3: 验证并提交**

```bash
cargo +nightly fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: remove impl Into parameters from app APIs"
```

---

### Task 11: 元组返回改命名结构体、补标准 trait 与构造器

**Files:**
- Modify: `src/db/tasks_init.rs`
- Modify: `src/crawler/resolve.rs`
- Modify: `src/http/http_clients.rs`
- Modify: `src/config/types.rs`
- Modify: `src/http/client.rs`、`src/http/fetch.rs`
- Modify: `src/models/download_task_record.rs`、`src/desktop/model/trace.rs`
- Modify: `src/desktop/themes/apply.rs`
- Modify: `src/core/update.rs`

**Interfaces:**
- Consumes: Task 10
- Produces: `LoadedTasks { tasks, max_id }`、`ResolvedBook { book, chapters }`、`GhProxy { url, client }`；6 个枚举的 `FromStr`；`ThemeVariant` 枚举

- [ ] **Step 1: 命名结构体**

```rust
// src/db/tasks_init.rs
pub struct LoadedTasks {
    pub tasks: Vec<DownloadTask>,
    pub max_id: u64,
}
pub fn load_tasks_from_file(path: &Path) -> Result<LoadedTasks, DaoError> { ... }

// src/crawler/resolve.rs
pub struct ResolvedBook {
    pub book: Book,
    pub chapters: Vec<Chapter>,
}
pub async fn resolve_book(...) -> Result<ResolvedBook, CrawlerError> { ... }

// src/http/clients.rs
pub struct GhProxy {
    pub url: String,
    pub client: reqwest::Client,
}
pub fn gh_proxy_pair(&self) -> GhProxy { ... }
```

同步更新所有解构点（`let (book, toc) = ...` → `let ResolvedBook { book, chapters } = ...`）。

- [ ] **Step 2: `FromStr`**

```rust
// src/config/types.rs
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseConfigValueError;

impl std::fmt::Display for ParseConfigValueError { ... }

impl FromStr for LangType {
    type Err = ParseConfigValueError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "zh_CN" | "zh-CN" | "zh-Hans" | "zh_Hans" => Ok(Self::ZhCn),
            "zh_TW" | "zh-TW" => Ok(Self::ZhTw),
            "zh_Hant" | "zh-Hant" => Ok(Self::ZhHant),
            _ => Err(ParseConfigValueError),
        }
    }
}

impl LangType {
    pub fn parse(s: &str) -> Option<Self> { s.parse().ok() }
}
```

`Language` 同样返回 `Result`，`parse` 保留为 `Option` 包装。`ExportFormat` / `ThemeKind` / `ThemeDynMode` / `ProxyMode` 的 `Err` 用 `std::convert::Infallible`，`from_str` 委托现有 `parse`。

- [ ] **Step 3: derive / Default / 构造器**

```rust
// src/config/types.rs
#[derive(Debug, thiserror::Error, Copy, Clone, PartialEq)]
pub enum ConfigError { ... }

// src/http/client.rs
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientOptions { pub unsafe_ssl: bool }

// src/http/fetch.rs
#[derive(Debug, Clone)]
pub struct FetchResponse { ... }

// src/models/task_record.rs
impl FailureRecord {
    pub const fn new(index: u32, title: String, reason: String) -> Self { ... }
}

// src/desktop/model/trace.rs
impl TraceId {
    pub const fn new(value: u64) -> Self { Self(value) }
}
```

- [ ] **Step 4: `bool` → 枚举**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeVariant { Light, Dark }

pub fn list_theme_names_by_mode(cx: &App, variant: ThemeVariant) -> Vec<SharedString> { ... }
```

更新调用点：`list_theme_names_by_mode(cx, ThemeVariant::Dark)`。

- [ ] **Step 5: 验证并提交**

```bash
cargo +nightly fmt --all
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: apply app-code API design guidance"
```

---

### Task 12: `UpdateCheckResult` 收敛为 `Result`

**Files:**
- Modify: `src/core/update.rs`
- Modify: `src/desktop/model/update_state.rs`
- Modify: `src/desktop/model/ops/update.rs`
- Modify: `src/desktop/model/mod.rs`（re-export）
- Test: `src/core/update.rs`（内联测试）

**Interfaces:**
- Consumes: Task 11
- Produces: `pub type UpdateCheckResult = Result<Option<String>, UpdateError>;`

- [ ] **Step 1: 先改测试（失败）**

把 `classify_failed_takes_precedence_over_latest` 等测试改为：

```rust
#[test]
fn classify_failure_returns_failed() {
    let r: UpdateCheckResult = Err(UpdateError("boom".into()));
    assert!(matches!(classify(&r), UpdateOutcome::Failed(msg) if msg == "boom"));
}

#[test]
fn classify_new_version_returns_new_version() {
    let r: UpdateCheckResult = Ok(Some("v999.0.0".into()));
    assert!(matches!(classify(&r), UpdateOutcome::NewVersion(v) if v == "v999.0.0"));
}

#[test]
fn classify_missing_tag_returns_empty_failure() {
    let r: UpdateCheckResult = Ok(None);
    assert!(matches!(classify(&r), UpdateOutcome::Failed(msg) if msg == "(empty result)"));
}
```

Run: `cargo test --lib core::update`
Expected: 编译失败（`UpdateError` 未定义）——红。

- [ ] **Step 2: 实现类型与 `check_latest_release`**

```rust
/// 更新检查失败原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateError(pub String);

impl std::fmt::Display for UpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for UpdateError {}

pub type UpdateCheckResult = Result<Option<String>, UpdateError>;

pub fn classify(result: &UpdateCheckResult) -> UpdateOutcome {
    match result {
        Err(e) => UpdateOutcome::Failed(e.to_string()),
        Ok(Some(latest))
            if latest.trim_start_matches('v') == env!("CARGO_PKG_VERSION") =>
        {
            UpdateOutcome::UpToDate
        }
        Ok(Some(latest)) => UpdateOutcome::NewVersion(latest.clone()),
        Ok(None) => UpdateOutcome::Failed("(empty result)".into()),
    }
}
```

`check_latest_release` 的返回改为 `Ok(tag)` / `Err(UpdateError(format!("GitHub API 返回 HTTP {}", ...)))` 等；HTTP 非成功、读响应失败、请求失败都走 `Err`。

- [ ] **Step 3: 更新 `UpdateState::drain`**

```rust
let result = try_drain_all(&mut self.rx, |res: UpdateCheckResult| {
    self.checking = false;
    match &res {
        Ok(v) => {
            self.latest_version.clone_from(v);
            self.error = None;
        }
        Err(e) => {
            self.latest_version = None;
            self.error = Some(e.to_string());
        }
    }
    outcome = Some(classify(&res));
});
```

- [ ] **Step 4: 更新测试与 re-export**

同步 `src/desktop/model/update_state.rs` 的测试构造；`src/desktop/model/mod.rs` 的 re-export 增加 `UpdateError`。

- [ ] **Step 5: 验证并提交**

```bash
cargo +nightly fmt --all
cargo test --lib core::update
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: represent update check outcome as Result"
```

---

### Task 13: `catch_unwind` 与实现细节

**Files:**
- Modify: `src/parser/cache.rs`
- Modify: `src/parser/book/meta.rs`
- Modify: `src/crawler/cover_updater.rs`
- Modify: `src/desktop/pages/settings/settings_page.rs`
- Modify: `src/desktop/pages/search/search_page.rs`
- Modify: `src/js/runtime.rs`
- Modify: `src/config/defaults.rs`

**Interfaces:**
- Consumes: Task 12
- Produces: 生产代码无 `catch_unwind`；组合链移出 `for` 头

- [ ] **Step 1: 删除生产 `catch_unwind`**

```rust
// src/parser/cache.rs
let parsed = Selector::parse(sel)
    .map_err(|e| SelectError::BadSelector(format!("`{sel}`: {e:?}")))?;
...
let parsed = Regex::new(pat)?;
```

删除 `use std::panic::{AssertUnwindSafe, catch_unwind};`。

- [ ] **Step 2: 测试 poison 改用 scoped thread**

```rust
#[test]
fn lock_or_recover_unpoisoned_mutex_keeps_working() {
    let shared = Arc::new(Mutex::new(0));
    let cloned = Arc::clone(&shared);
    let _ = std::thread::spawn(move || {
        let _g = cloned.lock().expect("lock");
        panic!("simulated poison");
    })
    .join();
    // 线程 panic 后 Mutex 处于 poisoned 状态；lock_or_recover 应仍能取回旧值。
    {
        let mut g = lock_or_recover(&shared);
        *g = 42;
    }
    assert_eq!(*lock_or_recover(&shared), 42);
}
```

不再使用 `catch_unwind`；poison 由另一线程 panic 后 `join()` 制造。

- [ ] **Step 3: 类型标注与组合链**

```rust
// src/desktop/pages/search/search_page.rs
let new_source_id: Option<i32> = v.strip_prefix("rule:").and_then(|s| s.parse().ok());

// src/config/defaults.rs
let doc: DocumentMut = match template.parse() { Ok(doc) => doc, Err(e) => ... };
```

`src/js/runtime.rs:75` 的 for 头链改为：

```rust
for raw in input.lines() {
    let line = raw.trim();
    if line.is_empty() {
        continue;
    }
    ...
}
```

`src/desktop/pages/search/search_page.rs`（原 `mod.rs:284`）同样先 `let enabled: Vec<_> = ...collect();` 或在循环体 `if !r.is_search_enabled() { continue; }`。

- [ ] **Step 4: `macro_rules!` → 函数**

```rust
fn make_state(
    names: &[SharedString],
    current: &str,
    window: &mut Window,
    cx: &mut Context<SettingsPage>,
) -> Entity<SelectState<SearchableVec<SharedString>>> {
    let items: SearchableVec<SharedString> = names.to_vec().into();
    let sel = SharedString::from(current.to_string());
    let pos = <SearchableVec<SharedString> as SelectDelegate>::position(&items, &sel);
    cx.new(|cx| SelectState::new(items, pos, window, cx).searchable(true))
}
```

调用点：`make_state(&initial_names, &pref0.static_name, window, cx)`。

- [ ] **Step 5: 字面量替换**

```rust
// meta.rs
let author = author
    .strip_prefix("作者: ")
    .or_else(|| author.strip_prefix("作者:"))
    .unwrap_or(&author)
    .to_string();

// cover_updater.rs
let normalized = cover
    .strip_suffix("/150.webp")
    .or_else(|| cover.strip_suffix("/150"))
    .unwrap_or(&cover);
return Some(normalized.to_string());
```

- [ ] **Step 6: 验证并提交**

```bash
cargo +nightly fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: apply implementation-concern guidance"
```

---

### Task 14: 所有权 — `resolve_book`、`HttpClients`、`zhconv`

**Files:**
- Modify: `src/crawler/resolve.rs`
- Modify: `src/http/clients.rs`（及 Task 7 的 `http_clients.rs`）
- Modify: `src/utils/zhconv.rs`
- Modify: `src/parser/toc/single.rs`
- Modify: 所有 `Arc<HttpClients>` 调用点

**Interfaces:**
- Consumes: Task 13
- Produces: `resolve_book` 不再深拷贝 `Rule`/`String`；`HttpClients` 内部无冗余 `Arc<Client>`；`convert_book_meta_in_place(&mut Book, ...)`

- [ ] **Step 1: `resolve_book` 捕获引用**

```rust
pub async fn resolve_book(
    cfg: &AppConfig,
    client: &reqwest::Client,
    source: &Source,
    book_url: &str,
    cancel: &CancelToken,
) -> Result<ResolvedBook, CrawlerError> {
    let cf_bypass: Option<&str> = if cfg.global.cf_bypass.trim().is_empty() {
        None
    } else {
        Some(cfg.global.cf_bypass.as_str())
    };
    let qidian_cookie: Option<&str> = if cfg.cookie.qidian_cookie.trim().is_empty() {
        None
    } else {
        Some(cfg.cookie.qidian_cookie.as_str())
    };
    let rule: &Rule = &source.rule;
    let eff = &source.effective_crawl;
    let max_attempts = if cfg.crawl.enable_retry { eff.max_retries } else { 0 };

    let book = retry_with_backoff(
        |_attempt| async move {
            if cancel.is_cancelled() {
                return Err(BookError::Http("cancelled".to_string()));
            }
            parse_book_detail(client, rule, book_url, cf_bypass, qidian_cookie).await
        },
        max_attempts,
        |_attempt| {
            let dur = Duration::from_millis(crate::http::random_retry_interval_ms(eff));
            async move { tokio::time::sleep(dur).await }
        },
    )
    .await
    .map_err(CrawlerError::Book)?;
    ...
}
```

TOC 的 `retry_with_backoff` 同样改为捕获 `&rule` / `client` / `book_url` / `cf_bypass` / `qidian_cookie` / `cancel`。删除 `book_url_owned`、`cf_bypass_owned`、`qidian_cookie_owned` 与所有 `Arc` 导入（若不再使用）。

- [ ] **Step 2: 删除 `Arc<reqwest::Client>`**

```rust
pub struct HttpClients {
    clients: RwLock<(reqwest::Client, reqwest::Client)>,
    gh_proxy: Mutex<(String, reqwest::Client)>,
    proxy_signature: Mutex<ProxySignature>,
    #[cfg(test)]
    generation: std::sync::atomic::AtomicU64,
}

pub fn for_rule(&self, rule: &Rule) -> reqwest::Client { ... } // 内部 clone() 是 refcount bump
pub fn gh_proxy_pair(&self) -> GhProxy { ... }
```

`empty()` / `new()` 初始化 `generation: AtomicU64::new(0)`；`rebuild_proxy` 成功替换后 `fetch_add(1, Ordering::Relaxed)`。
删除 `safe_client_ptr`，测试改为：

```rust
fn generation(&self) -> u64 { self.generation.load(Ordering::Relaxed) }
```

断言 `rebuild_proxy` 后 generation +1，未变化时不变。

- [ ] **Step 3: `zhconv` 原地转换**

```rust
pub fn convert_book_meta_in_place(book: &mut Book, source_lang_raw: &str, target: &LangType) {
    let Some(source) = LangType::parse(source_lang_raw) else { return; };
    if source == *target { return; }
    book.book_name = convert_text(&book.book_name, target);
    book.author = convert_text(&book.author, target);
    let intro = book.intro.take();
    if let Some(intro) = intro {
        book.intro = Some(convert_html_body(&intro, target));
    }
}
```

`resolve_book` 调用点：`let mut book = ...; convert_book_meta_in_place(&mut book, &source.rule.language, &target_lang);`。
保留 `convert_book_meta` 作为测试/兼容包装：

```rust
pub fn convert_book_meta(book: &Book, source_lang_raw: &str, target: &LangType) -> Book {
    let mut out = book.clone();
    convert_book_meta_in_place(&mut out, source_lang_raw, target);
    out
}
```

- [ ] **Step 4: `&Arc<Selector>` → `&Selector`**

`src/parser/toc/single.rs` 的 `parse_items_from_fragment(frag: &Html, sel: &Arc<Selector>, ...)` 改为 `sel: &Selector`；调用点传 `&item_selector`（deref coercion）。

- [ ] **Step 5: 验证并提交**

```bash
cargo +nightly fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: remove avoidable clones in resolve and http clients"
```

---

### Task 15: 所有权 — `Source { Arc<Rule> }` 与全局 `Arc<HttpClients>`

**Files:**
- Modify: `src/models/rule.rs`
- Modify: `src/core/bootstrap.rs`
- Modify: `src/core/search.rs`
- Modify: `src/desktop/model/mod.rs`
- Modify: `src/desktop/model/{sources.rs,ops/search.rs,ops/download.rs,ops/sources.rs,ops/update.rs}`
- Modify: `src/crawler/{search.rs,health.rs}`
- Modify: `src/desktop/pages/{search/mod.rs,sources/mod.rs}`
- Modify: `src/core/sources.rs`

**Interfaces:**
- Consumes: Task 14
- Produces: `Source { rule: Arc<Rule>, effective_crawl }`；`AppModel.rules: Vec<Arc<Rule>>`；`AppContext.http: HttpClients`、`AppModel.http: HttpClients`、`OpsContext.http: &HttpClients`

- [ ] **Step 1: `Source` 持有 `Arc<Rule>`**

```rust
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Source {
    pub rule: Arc<Rule>,
    pub effective_crawl: EffectiveCrawl,
}

impl Source {
    pub fn from(rule: Arc<Rule>, cfg: &AppConfig) -> Self {
        let effective_crawl = EffectiveCrawl::derive(cfg, &rule);
        Self { rule, effective_crawl }
    }
}
```

`AppModel.rules: Vec<Arc<Rule>>`；构造时 `let rules = load_active_rules(...).into_iter().map(Arc::new).collect();`。
`core::search::select_sources(rules: &[Arc<Rule>], ...) -> Vec<Source>` 用 `Arc::clone(rule)`。
所有 `source.rule.clone()` 变为 `Arc::clone(&source.rule)`；字段访问经 `Deref` 不变。

- [ ] **Step 2: 移除全局 `Arc<HttpClients>`**

```rust
// core/bootstrap.rs
pub struct AppContext {
    pub http: HttpClients, // 原 Arc<HttpClients>
}

// desktop/model/mod.rs
pub struct AppModel {
    pub http: HttpClients, // 原 Arc<HttpClients>
    ...
}
```

`OpsContext<'a>` 用 `pub http: &'a HttpClients`。
每个 `runtime.spawn` 之前取出任务真正需要的 owned client：

```rust
let client = http.for_rule(&source.rule);
let runtime = ...;
runtime.spawn(async move {
    let result = download_chapters(&cfg, &client, &source, &book, chapters, opts).await;
    ...
});
```

`crawler::search::spawn_search_tasks(http: &HttpClients, ...)`：在 `set.spawn` 前 `let client = http.for_rule(&src.rule);`，把 `reqwest::Client` 移入任务。
`crawler::health::check_sources_health` 同理：调用方预计算 `Vec<(Rule, reqwest::Client)>`。

更新检查也必须在 `spawn` 前取出 owned 值。把 `core::update::check_latest_release` 改为：

```rust
pub async fn check_latest_release(
    gh_proxy: GhProxy,
    safe_client: reqwest::Client,
) -> UpdateCheckResult
```

`spawn_update_check` 中：

```rust
let gh_proxy = http.gh_proxy_pair();
let safe_client = http.for_rule(&Rule { ignore_ssl: false, ..Rule::default() });
runtime.spawn(async move {
    let result = check_latest_release(gh_proxy, safe_client).await;
    let _ = tx.send(result);
    wakeup.notify();
});
```

同步更新 `src/desktop/model/update_state.rs` 的 re-export 与测试调用。

- [ ] **Step 3: 验证并提交**

```bash
cargo +nightly fmt --all
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: share rules by Arc and remove global HttpClients Arc"
```

---

### Task 16: 所有权 — 下载路径、任务持久化、列表渲染

**Files:**
- Modify: `src/crawler/download.rs`
- Modify: `src/crawler/progress.rs`
- Modify: `src/core/download_task.rs`
- Modify: `src/desktop/model/ops/download.rs`
- Modify: `src/desktop/model/{tasks.rs,download.rs,events.rs,persistence.rs}`
- Modify: `src/desktop/pages/search/{search_page.rs,delegate.rs,result_row.rs}`
- Modify: `src/desktop/pages/library/{library_page.rs,delegate.rs}`
- Modify: `src/desktop/model/{sources_state.rs,search_state.rs}`
- Modify: `src/desktop/pages/sources/sources_page.rs`

**Interfaces:**
- Consumes: Task 15
- Produces: `Progress::BookResolved { book: Arc<Book> }`；`DownloadTask.book_meta: Option<Arc<Book>>`；`page_items: Vec<(usize, Arc<SearchResult>)>`；`filtered_rule_indices -> Vec<usize>`

- [ ] **Step 1: 下载路径去冗余拷贝**

```rust
// crawler/download.rs
let rule = Arc::clone(&source.rule);
let rule_chapter = rule
    .chapter
    .as_ref()
    .ok_or(CrawlerError::Toc(TocError::TocRuleMissing))?;
...
let chapter_ref: &Chapter = &chapter;
let fetch_future = retry_with_backoff(
    |_attempt| {
        let rule = Arc::clone(&rule);
        async move { parse_chapter(&client, &rule, chapter_ref, cf.as_deref()).await }
    },
    ...
);
```

`download_book`：

```rust
let book = Arc::new(book);
let _ = progress.send(Progress::BookResolved {
    book: Arc::clone(&book),
    total_chapters: toc.len(),
});
...
download_chapters(cfg, client, source, &book, toc, opts).await
```

- [ ] **Step 2: `BookResolved` 用 `Arc<Book>`**

```rust
// crawler/progress.rs
BookResolved { book: Arc<Book>, total_chapters: usize }

// core/download_task.rs
pub book_meta: Option<Arc<Book>>,
...
self.book_meta = Some(book);
```

`to_record` / `from_record` / `TaskSummary` / `summary.rs` / `row.rs` 中按需 `Arc::clone` 或 `as_deref().cloned()`；持久化只在写盘时深拷贝一次。
`desktop/model/ops/download.rs` 的 `book_for_meta` 与 `Box::new(book.clone())` 改为单个 `Arc<Book>`，两处共享。

- [ ] **Step 3: 任务持久化不克隆整表**

```rust
// desktop/model/tasks.rs
let json = crate::db::to_json_string(&self.tasks)?;
runtime.spawn_blocking(move || crate::db::write_atomically(&path, json.as_bytes()));
```

若没有 `to_json_string`，在 `db` 加 `pub fn to_json_string<T: Serialize>(value: &T) -> Result<String, serde_json::Error>`；写入逻辑仍在 `spawn_blocking` 内。

- [ ] **Step 4: 搜索/库列表按页借或 `Arc`**

```rust
// search page
let page_items: Vec<(usize, Arc<SearchResult>)> = ...; // 仅 Arc::clone 当前页
// result_row / delegate
fn render_row(index: usize, result: &Arc<SearchResult>, ...) { ... }
```

`filtered_rules` 改为：

```rust
pub fn filtered_rule_indices(&self, rules: &[Rule]) -> Vec<usize> { ... }
```

页面只对当前页索引 `rules[i].clone()` 或 `Arc::clone(&model.rules[i])`。

`library/delegate.rs`：`let (global_index, entry) = self.page_items.get(ix.row)?;` 直接借用，不 clone。

`SourcesState.health` / `SearchState.source_status` 改为 `Arc<HashMap<...>>` 快照，渲染时 `Arc::clone`。

- [ ] **Step 5: 验证并提交**

```bash
cargo +nightly fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git add -A
git commit -m "refactor: eliminate per-render deep clones and share book metadata"
```

---

### Task 17: 断言上下文与文档示例

**Files:**
- Modify: `src/core/{async_progress.rs,download_task.rs,library.rs,search.rs,sources.rs}`
- Modify: `src/crawler/{retry.rs,search.rs,health.rs}`
- Modify: `src/db/atomic_write.rs`
- Modify: `src/parser/cache.rs`
- Modify: `src/config/tests.rs`
- Modify: `src/db/rules/apply_default.rs`、`src/db/tasks.rs`、`src/utils/lock.rs`、`src/parser/dom/selector.rs`

**Interfaces:**
- Consumes: Task 16
- Produces: 非自明断言带消息；文档示例用 `expect`

- [ ] **Step 1: 给非自明断言加消息**

```rust
assert!(slot.is_none(), "Empty 分支必须清空 rx slot");
assert!(slot.is_some(), "Continue 必须把 rx 放回 slot");
assert!(any, "有待处理事件时 drain 不应返回 false");
assert!(sources.is_empty(), "禁用书源后结果应为空");
assert!(result.is_err(), "非法规则应返回错误: {result:?}");
assert!(
    !Arc::ptr_eq(&a, &b),
    "不同选择器字符串不得共享同一 Arc"
);
assert!(
    (loaded.global.font_size - 20.0).abs() < f32::EPSILON,
    "round-trip 后 font_size 应为 20.0，实际 {}",
    loaded.global.font_size
);
```

对其余断言按「失败时看不出原因才加」判断；不自明的 `assert_eq!` 增加字段名/期望值。

- [ ] **Step 2: 文档示例改 `expect`**

```rust
let g = mutex_or("counter", &m).expect("lock available");
let rule = rule.book.as_ref().expect("rule has book");
let state = state.tasks.lock().expect("tasks lock not poisoned");
let out = select_and_invoke_js(...).expect("example selector resolves");
```

- [ ] **Step 3: 验证并提交**

```bash
cargo +nightly fmt --all
cargo test --all-targets
git add -A
git commit -m "test: add context to non-obvious assertions"
```

---

### Task 18: lint 配置与 CI

**Files:**
- Modify: `src/lib.rs`
- Create: `clippy.toml`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: Task 17
- Produces: CI 强制 fmt/clippy/test

- [ ] **Step 1: 收紧 `src/lib.rs` lint**

```rust
#![deny(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![warn(clippy::expect_used, clippy::panic)]
#![warn(clippy::todo, clippy::unimplemented)]
#![warn(clippy::pedantic, clippy::nursery)]
#![warn(clippy::wildcard_imports)]
#![warn(clippy::missing_panics_doc)]
// 测试模块普遍使用 `use super::*`；只在整个测试构建中放行，不影响生产代码。
#![cfg_attr(test, allow(clippy::wildcard_imports))]
#![allow(
    clippy::module_name_repetitions,
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_sign_loss,
    clippy::struct_excessive_bools,
    clippy::too_many_lines,
    clippy::result_large_err
)]
```

删除 `clippy::must_use_candidate`、`clippy::missing_errors_doc` allow，按 clippy 输出为纯函数/关键返回值补 `#[must_use]`，为返回 `Result` 的公开函数补 `# Errors`。
测试模块已有的 `#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]` 保留。

- [ ] **Step 2: `clippy.toml`**

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-panic-in-tests = true
```

- [ ] **Step 3: 新增 CI**

```yaml
name: ci

on:
  push:
    branches: [master]
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  fmt:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@nightly
        with:
          components: rustfmt
      - run: cargo +nightly fmt --all -- --check

  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy
      - name: Install Linux build deps
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev \
            libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
            libwayland-dev libgl1-mesa-dev libegl1-mesa-dev \
            libglib2.0-dev libssl-dev mesa-vulkan-drivers
          sudo apt-get install -y vulkan-utility-libraries-dev || true
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --all-targets
```

- [ ] **Step 4: 全量验证**

```bash
cargo +nightly fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo build --release
cargo check --no-default-features
```

Expected: 全部退出 0。若 `--features web` 需要 `bun` 且环境缺失，记录为环境限制，不伪造通过。

- [ ] **Step 5: 提交**

```bash
git add -A
git commit -m "ci: enforce rustfmt, clippy, and tests"
```

---

### Task 19: 一致性报告与任务收尾

**Files:**
- Create: `docs/google-rust-style-conformance.md`
- Modify: `tasks/todo.md`
- Modify: `tasks/lessons.md`（仅当本任务产生新教训）

**Interfaces:**
- Consumes: Task 18
- Produces: 规则 → 处置的可审计清单；最终指标

- [ ] **Step 1: 写一致性报告**

至少包含：

1. 基线 vs 最终指标：测试数、clippy、`Rc/Arc` 行数、大型 `.clone()` 数、未文档化公开项数。
2. 逐条规则表：`已应用` / `例外` / `N/A`，并链接到具体文件。
3. 例外清单（复制 spec §8）及理由。
4. 未验证项：UI 外观、跨平台行为、`--features web` 环境限制。
5. 建议实机点击项：搜索、下载指定范围、书源健康检查、设置页主题/代理、更新检查。

- [ ] **Step 2: 更新 `tasks/todo.md`**

按项目既有格式追加本次任务的清单与 Review 段，引用 spec / plan 路径、每阶段提交号、最终验证命令与结果。

- [ ] **Step 3: 最终验证**

```bash
cargo +nightly fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
git status --short
```

Expected: 工作树干净；所有命令退出 0。

- [ ] **Step 4: 提交**

```bash
git add -A
git commit -m "docs: record Google Rust style conformance"
```

---

## Self-Review

**Spec coverage:** spec §4.1 的每条强制规则映射到 Task 1–9、13；§4.2 强推荐映射到 Task 10–12；§5 P5 所有权映射到 Task 14–16；P6 映射到 Task 13、17；P7 映射到 Task 18–19。§4.3 N/A 与 §8 例外在 Task 19 报告中记录。

**Placeholder scan:** 所有任务包含具体文件、变换规则、代码片段与命令；无 TBD/TODO。批量文档与断言任务以「类别 + 示例 + clippy 驱动」定义，未逐个列出 118 项/610 条断言，这是有意的批量操作边界。

**Type consistency:** `ResolvedBook`、`LoadedTasks`、`GhProxy`、`UpdateError`、`UpdateCheckResult`、`Source { Arc<Rule> }`、`AppModel.rules: Vec<Arc<Rule>>`、`HttpClients`（非 Arc）、`convert_book_meta_in_place`、`filtered_rule_indices` 在各任务中名称与签名一致。

**已知顺序依赖:** Task 7 重命名 `http/clients.rs` 后，Task 14 的文件名按 `http_clients.rs` 处理；Task 8 的 `random_ua → random_user_agent` 与 Task 7 合并时避免重复改名。
