# Google Rust Style Guide 全仓一致性修订 — 设计

- 日期：2026-11-09
- 分支：`style/google-rust-guide`
- 依据：<https://google.github.io/styleguide/rustguide.html>
- 范围：`src/`（167 个 `.rs`，约 26,643 行）、`build.rs`、`rustfmt.toml`、`src/lib.rs` lint 配置、CI
- 深度：用户选定「全量严格（含所有权重构）」

## 1. 背景与目标

仓库已有较严格的工程规约（`#![deny(unsafe_code)]`、pedantic + nursery、零 unsafe、零生产
`unwrap`），但存在与 Google Rust Style Guide 不一致的导入组织、命名、文档、API 形态与
所有权使用。本任务按指南做一次全仓一致性修订，并建立可执行的 CI 强制，防止回退。

目标：

1. 所有 binding 规则零违规；
2. application code 的强推荐 API 规则在合理处落地；
3. 指南明确标注 `[chrome]` / `[Fuchsia]` 或不适用于应用二进制的规则不强行套用；
4. 框架强约束下无法消除的 `Rc`/`Arc` 以书面例外记录；
5. 行为保持不变（除第 7 节列出的契约收敛），现有测试全绿。

## 2. 基线（2026-11-09）

| 指标 | 数值 |
|---|---|
| `cargo test --all-targets` | 457 passed / 4 ignored / 0 failed |
| `cargo clippy --all-targets` | No issues found |
| `cargo fmt --check`（stable、现有配置） | 干净 |
| `rustc` / `cargo` / `rustfmt` | 1.98.1 / 1.98.1 / 1.9.0-stable |
| glob import 违规 | 0（67 处 `use super::*` 均在测试模块；5 处 `pub use sub::*` 在非叶模块） |
| `Rc`/`Arc`/`RefCell`/`Mutex`/`RwLock` | 约 110 行 / 31 文件 |
| 大型类型 `.clone()` | 约 60 处 |
| 公开项缺文档 | 118 / 424（不含 `pub mod`） |
| 生产 `catch_unwind` | 2 处（`src/parser/cache.rs:47,75`） |

## 3. 判定原则

指南区分 library code 与 application code。本项目是 `publish = false` 的应用二进制：

- 涉及 API 设计的规则按 **strong recommendation** 执行，优先简单直接，避免过度泛化；
- 指南自身的例外条款（尤其是 `Rc`/`Arc`、async、线程、测试框架）优先于机械套用；
- `[chrome]` / `[Fuchsia]` 标注的规则不适用于本项目；
- 注释与文档继续使用中文；指南只规范语法与完整性，不规定语言。

## 4. 规则处置矩阵

### 4.1 强制执行

| 指南条目 | 处置 |
|---|---|
| Use the appropriate formatting tool | `rustfmt.toml` 启用 `max_width=100`、`imports_granularity=Module`、`group_imports=StdExternalCrate`、`hex_literal_case=Lower`、`format_macro_matchers=true`、`format_code_in_doc_comments=true`；fmt 使用 nightly，构建/测试保持 stable |
| Don't use glob imports | 保持现状；仅测试 `use super::*`、非叶模块 `pub use sub::*` 例外 |
| A use statement should import items from one module | 由 `imports_granularity=Module` 强制；手工修正剩余嵌套 `use`（31 条） |
| Prefer locally-scoped trait imports | `Write`、`FromStr`、`SubscriberExt` 等移入使用处 |
| Qualify calls to top-level functions with a module name | 约 40 处首方自由函数改为 `module::function()` |
| Type names should be imported and used unqualified | 约 138 处模块限定类型改为模块顶部导入 |
| Name source files and leaf modules after the primary item | `cf.rs→cloudflare.rs`、`ua.rs→user_agent.rs`、`task_record.rs→download_task_record.rs`、`clients.rs→http_clients.rs` |
| Don't implement APIs in non-leaf modules | 拆分 9 个非叶模块，见 P2 |
| Naming methods / initialization functions / extension traits / avoid abbreviations | `*Cfg→*Config`、`*Ctx→*Context`、`t_*→toml_*`、`abs_url→absolute_url`、`random_ua→random_user_agent`、`with_defaults`、`from_cfg→from_config`、`from_record` 配 `From`、描述性生命周期；无扩展 trait，`Ext` 规则 N/A |
| When and how to write documentation | 去掉 `#![allow(missing_docs)]`；补齐 118 项；修 `js/mod.rs`、`parser/mod.rs` 反引号；`desktop/mod.rs` 的 doc-on-`use`；`themes/mod.rs` 分裂 `//!`；`logger.rs` panic 说明移入 `# Panics` |
| Unsafe code / No unstable features | 已满足，保持 `#![deny(unsafe_code)]`、无 `#![feature]` |
| Don't call `panic::catch_unwind` | 删除 2 处生产调用；测试用 scoped thread `join()` 制造 poison |
| Prefer `expect` over `unwrap` | 生产代码已合规；4 处文档示例 `.unwrap()` 改 `.expect("reason")` |
| Don't overuse type inference | 为 3 处 `.parse()` / `.into()` 补显式类型 |
| Don't use iterator combinator chains when direct iteration is clearer | 3 处：2 个 `for` 头组合链、1 个 5 步链 |
| Prefer `format!` / `write!` for concatenation and substitution | 无 `+` 拼接；2 处字面量 `.replace()` 改 `strip_prefix` / `strip_suffix` |
| Avoid Rc and Arc / Prefer borrows over deep copies | 按 P5 消除可消除项；框架强约束项记录例外 |

### 4.2 按应用代码强推荐执行

| 指南条目 | 处置 |
|---|---|
| Application code should almost never use `impl Into<T>` | 10 个 `AppError` 构造器、5 个 `AppModel::push_*`、10 处 UI builder、1 个私有 `set_item` 改为具体类型 |
| Prefer named fields over tuples | `LoadedTasks`、`ResolvedBook`、`GhProxy`；其余局部、已文档化的二元组保留 |
| Implement known std traits instead of custom methods | 6 个配置枚举补 `FromStr`，保留 `parse` 薄包装 |
| Pair important `From` impls with constructors | `FailureRecord::new`、`TraceId::new`、`ProxySignature::from_config` + `From<&AppConfig>` |
| Important traits guidance | `ConfigError` 补 `Copy/Clone/PartialEq`；`ClientOptions` 补 `Copy`；`FetchResponse` 补 `Debug/Clone`；`UpdateCheckResult` 收敛后补 `Debug/Clone` |
| Options structs should implement `Default` | 相关 options 结构体补齐 |
| Prefer enum over bool in APIs | `list_theme_names_by_mode(..., dark: bool)` → `ThemeVariant` |
| `#[must_use]` on functions / types | 按指南规则标注纯函数与关键返回值；移除 `clippy::must_use_candidate` allow |
| `#[non_exhaustive]` | N/A：应用二进制、内部类型 |
| Implement expected API sets together | 生产代码无 `len()` 单独存在的情况；保持 |
| Use const functions responsibly | 保留 `const fn`；不新增不稳定 const 能力 |

### 4.3 不适用

- `Prefer small crates [chrome]`
- `Use flat file layout for rust_library sources [chrome]`
- `Don't use async / .await [chrome]`
- `Don't start new threads through the Rust standard library [chrome]`
- `Use fuchsia_sync::Mutex [Fuchsia]`
- `Use the appropriate import mechanism [chrome]`
- `Use the right per-codebase test framework`
- `Do not access #[doc(hidden)] items`
- `Avoid overloading for non-mathematical purposes`（无运算符重载）

## 5. 阶段计划

每阶段完成后必须依次通过 `cargo +nightly fmt --check`、`cargo clippy --all-targets -- -D warnings`、
`cargo test --all-targets`，并单独提交。

### P0 — 准备
- 从 `master` 建 `style/google-rust-guide`。
- 记录基线指标（见第 2 节）到本 spec / `tasks/todo.md`。
- 确认 app 未运行（避免 Windows 链接冲突）。

### P1 — 格式化与导入机制
1. 更新 `rustfmt.toml`：启用稳定的既有选项 + 上述 nightly 选项；`wrap_comments` 保持关闭（CJK 风险）。
2. `cargo +nightly fmt --all`；若本机无 nightly，记录安装命令并以 `cargo fmt --all -- --config ...`
   做等价验证。
3. 拆分 31 条嵌套 `use`（rustfmt 自动完成后人工复核 `logger.rs`、`root.rs` 等 gpui 导入）。
4. 约 40 处自由函数改为模块限定调用，重点：`sanitize_filename`（6 文件）、`write_atomically`
   （2 文件）、`cover`/`summary`/`lock`/`encoding`/`ua`/`url_join` 组。
5. 约 138 处类型名去路径，重点：`reqwest::Client`、`gpui_kit::{SharedString, Div, FocusHandle, ViewElement}`、
   `scraper::ElementRef`、`image::*`、`std::time::*`、`serde_json::Value`、`zip::*`、`regex::Captures`、
   `encoding_rs::*`、`toml_edit::*`、`url::Url`。
6. 测试模块 `use super::*` 与 `ops/mod.rs` 的 `pub use` 保留。

### P2 — 模块结构
拆分非叶模块，`mod.rs` 只保留 `mod` 声明与 `pub use`：

| 现文件 | 拆分后 |
|---|---|
| `src/db/mod.rs` | `dao.rs`（`DaoError`、`write_atomically`、`From` impl）+ 保留声明 |
| `src/desktop/mod.rs` | `actions.rs`（`actions!` + `Eq` impl）、`run.rs`（`run()`） |
| `src/desktop/model/mod.rs` | `app_model.rs`（`AppModel` + 203 行 impl） |
| `src/config/mod.rs` | `global.rs`（`OnceLock`/`LazyLock`、`set_global`/`global`/`validate_global`） |
| `src/desktop/pages/search/mod.rs` | `search_page.rs`（705 行实现） |
| `src/desktop/pages/sources/mod.rs` | `sources_page.rs`（366 行） |
| `src/desktop/pages/library/mod.rs` | `library_page.rs`（304 行） |
| `src/desktop/pages/tasks/mod.rs` | `tasks_page.rs`（261 行） |
| `src/desktop/pages/settings/mod.rs` | `settings_page.rs`（342 行） |

叶子模块重命名：`http/cf.rs→http/cloudflare.rs`、`http/ua.rs→http/user_agent.rs`、
`models/task_record.rs→models/download_task_record.rs`、`http/clients.rs→http/http_clients.rs`。
同步更新 `mod` 声明、`pub use`、测试路径。

### P3 — 命名与文档
1. 类型重命名：`GlobalCfg→GlobalConfig`、`DownloadCfg→DownloadConfig`、`SourceCfg→SourceConfig`、
   `CrawlCfg→CrawlConfig`、`CookieCfg→CookieConfig`、`ProxyCfg→ProxyConfig`；`OpsCtx→OpsContext`、
   `PageCtx→PageContext`；serde rename 保持 JSON/TOML 兼容。
2. 函数/模块重命名：`t_item/t_table/t_str/t_bool/t_int/t_float→toml_*`、`abs_url→absolute_url`、
   `random_ua→random_user_agent`、`with_defaults→new`（或直接用 `Default`）、`from_cfg→from_config`、
   `from_record` 增补 `From` 实现。
3. 生命周期：`FetchRequest<'a>`/`HttpMethod<'a>`→`'request`/`'raw`；`Paginator<'b>`→`'pdf`；
   `OpsCtx<'a>`/`PageCtx<'a>`→`'app`/`'page`。
4. 文档：去掉 `#![allow(missing_docs)]`，补齐 118 项；按指南「不重复函数名」原则写有意义内容；
   修 2 处 Markdown、1 处 doc-on-`use`、1 处分裂 `//!`、2 处 `# Panics`、1 处 `# Errors` 措辞。
5. 6 个配置枚举补 `FromStr`；`ClientOptions` 等补 `Default`；`Source::from` 保留（构造函数命名合规）。

### P4 — API / 类型 / Trait
1. 去 `impl Into`：`AppError` 10 个构造器、`AppModel` 5 个 `push_*`、3 个 UI builder、私有 `set_item`。
2. 命名元组：`db::tasks_init::load_tasks_from_file`、`crawler::resolve::resolve_book`、
   `HttpClients::gh_proxy_pair`。
3. trait/derive：`ConfigError`、`ClientOptions`、`FetchResponse`、`UpdateCheckResult`；
   `FailureRecord::new`、`TraceId::new`。
4. `UpdateCheckResult` 的「两个互斥 `Option`」改为 `Result<Option<String>, UpdateError>`，
   同步更新 `core::update`、`desktop::model::ops::update`、设置页与测试。
5. `core::library::LibraryEntry::from_path` 保持 `Option`：审计确认它只有测试调用，
   真实库扫描（`desktop::model::library_state::scan_library_dir`）已经返回 `io::Result`；
   改动该函数不会影响生产路径，故不做无收益的契约变更。
6. `bool`→`ThemeVariant`；按 `clippy::must_use_candidate` 输出逐项标注 `#[must_use]`。
7. `len()`/`is_empty()` 已配对，保持；`#[non_exhaustive]` N/A。

### P5 — 所有权与借用
按审计报告的价值/风险顺序：

1. `resolve_book`：`retry_with_backoff` 无 `'static` 约束，闭包捕获引用，删除每轮
   `Rule`/`String`/`Client` 深拷贝。
2. `desktop/pages/sources`：`filtered_rules(&[Rule]) -> Vec<Rule>` 改为返回索引，仅克隆当前页；
   `health` 按页取用或 `Arc<HashMap<...>>`。
3. `src/http/clients.rs`：删除 `Arc<reqwest::Client>`（`reqwest::Client` 本身内部 Arc），
   `clients: RwLock<(Client, Client)>`、`gh_proxy: Mutex<(String, Client)>`；
   `gh_proxy_pair -> (String, Client)`；测试改用 generation 计数。
4. 下载路径：`Book`/`RuleChapter`/`Chapter` 冗余克隆；`Progress::BookResolved` 用
   `Arc<Book>` 或摘要字段；`Source` 校验 `chapter` 后复用 `Arc<Rule>`。
5. `Source { rule: Arc<Rule> }` + `AppModel.rules: Vec<Arc<Rule>>` + 核心/爬虫/ops 全链路；
   搜索/下载派发由整份 `Rule` 深拷贝变为引用计数。
6. 移除全局 `Arc<HttpClients>`：`AppContext.http: HttpClients`、`AppModel.http: HttpClients`、
   `OpsCtx.http: &HttpClients`；每个 `spawn` 前预计算 `reqwest::Client`。
7. 任务持久化：UI 线程序列化为 `String` 后移入 `spawn_blocking`；去掉整份 `Vec<DownloadTask>` 克隆。
8. 列表/搜索渲染：`Arc<SearchResult>` 页面项；`LibraryEntry` 借用渲染；`SearchState`/
   `SourcesState` 大 HashMap 用 `Arc` 快照。
9. `zhconv::convert_book_meta` 改原地版本；`parser/toc/single.rs` 的 `&Arc<Selector>` → `&Selector`。
10. 保留例外并写入一致性报告：GPUI `RenderImage`/`Rc` listener/`Rc<ThemeConfig>`、
    `tokio::spawn` + `Arc<Semaphore>`、`CancelToken`/`notify`、全局选择器/正则缓存、
    `ListCache`、PDF `RefCell`、测试脚手架。

### P6 — 实现细节
1. 删除 `parser/cache.rs` 两处生产 `catch_unwind`；测试 poison 改为 scoped thread `join()`。
2. 补 3 处显式类型；提取 3 处组合链；2 处 `replace`→`strip_*`。
3. `settings/mod.rs` 的 `macro_rules! make_state!` 改为局部函数。
4. 断言上下文：按「失败不自明才加」原则处理审计列出的 16 处代表性断言，不机械覆盖全部 610 条。
5. 复核文档示例中的 `.unwrap()` → `.expect("reason")`。

### P7 — lint / CI 收口
1. `src/lib.rs`：移除 `clippy::must_use_candidate`、`clippy::missing_errors_doc` allow；
   启用 `clippy::wildcard_imports`、`clippy::missing_panics_doc`；
   `clippy::unwrap_used` 非测试 deny（测试通过 `clippy.toml` 或模块 allow 放行）。
2. 新增 `ci.yml`：`fmt`（nightly）、`clippy --all-targets -- -D warnings`、`test --all-targets`；
   Linux 运行需安装与 release workflow 相同的 gpui 依赖。
3. 写 `docs/google-rust-style-conformance.md`：逐条规则 → 已应用 / 例外 / N/A，附例外理由。
4. 更新 `tasks/todo.md`（清单 + Review）与必要的 `tasks/lessons.md`。
5. 最终验证（第 6 节）。

## 6. 验收标准

每个阶段：

- `cargo +nightly fmt --check` 退出 0；
- `cargo clippy --all-targets -- -D warnings` 退出 0；
- `cargo test --all-targets` 与基线一致（457 passed / 4 ignored），已知 flake 以基线对照；
- 生产代码无新增 `unwrap`、无 `catch_unwind`、无 `unsafe`。

最终：

- `cargo build --release` 成功；
- `cargo check --no-default-features` 成功；
- `cargo check --no-default-features --features web`（环境有 bun 时；否则注明限制）；
- `Rc`/`Arc` 与大型 `.clone()` 数量下降，剩余项均在一致性报告中归类为框架必需或有意；
- 文档：`cargo doc --no-deps` 无 broken intra-doc link（现有 lint 保持）。

## 7. 有意的契约变更

1. `UpdateCheckResult`：`{ latest_version: Option<String>, error: Option<String> }` →
   `Result<Option<String>, UpdateError>`，消除「同时成功/同时失败」的不可表示状态。
2. `impl Into` 移除后，调用点需显式 `.into()`/`.to_string()`；这是 API 形态变化，不改变行为。
4. `Source { rule: Arc<Rule> }` 与 `AppModel.rules: Vec<Arc<Rule>>`：内部所有权变化，外部行为不变。

## 8. 例外清单（不修改，写入一致性报告）

| 例外 | 理由 |
|---|---|
| `Arc<RenderImage>`（`logo.rs`、`detail_dialog.rs`、`search/mod.rs`） | GPUI `ImageSource::Render` 即 `Arc<RenderImage>`，纹理跨帧共享 |
| `Rc<dyn Fn(...)>` listener（`pagination.rs`、`settings/mod.rs`、`settings/ctx.rs`） | GPUI 回调必须 `Fn + 'static`；每帧重建元素，render 闭包无法调用 `cx.listener` |
| `Rc<ThemeConfig>`（`themes/apply.rs`） | gpui-kit `ThemeRegistry` 返回并存储 `Rc<ThemeConfig>` |
| `tokio::spawn` + `Arc<Semaphore>`（`download.rs`、`search.rs`、`toc/single.rs`） | `tokio::sync::Semaphore` 非 `Clone`，`acquire_owned()` 为 `'static` 许可必须 `Arc`；改为 `buffer_unordered` 属跨执行器语义重构，指南「unreasonably complex」例外 |
| `CancelToken { Arc<AtomicBool>, Arc<Notify> }`、`Arc<dyn Fn() + Send + Sync>` | UI 与所有下载任务共享取消/唤醒信号 |
| `static OnceLock<Mutex<HashMap<_, Arc<Selector/Regex>>>>` | 跨线程共享解析缓存；`Arc` 是避免重复解析的句柄 |
| `ListCache` 的 `Arc<Vec<T>>` / `Arc<dyn Any>` | 专门用于消除每帧整表克隆；去掉会回归性能 |
| PDF `RefCell<HashMap<u32, f32>>` | 单线程同步导出，`char_w(&self)` 需要内部可变性 |
| 测试中的 `Rc<RefCell<_>>`、`Arc<Mutex<_>>`、`Arc<Barrier>` | 测试脚手架，无生产所有权影响 |

## 9. 风险与缓解

| 风险 | 缓解 |
|---|---|
| GPUI / tokio 的 `'static` 与共享约束 | 保留已列例外；每项改动先编译验证 |
| 全仓 diff 巨大、审查困难 | 按 P1–P7 分阶段提交，每阶段可独立回滚 |
| 重命名与模块拆分产生跨文件涟漪 | 编译器驱动迁移，阶段内先 `cargo check` 再测试 |
| nightly rustfmt 不可用 | CI 固定 nightly 仅用于 fmt；本地无 nightly 时用 `--config` 等价命令 |
| 已知测试 flake（i18n、library 排序） | 以基线 457/4 对照，失败先看用例名，不误判 |
| 无法目视验证 UI | 最终交付列出建议实机点击项；不声称已验证视觉 |
| `missing_docs` 产生噪声文档 | 按指南「不重复名称」原则写；确实无信息量的项用定向 `#[allow(missing_docs)]` + 理由 |

## 10. 交付物

1. 分支 `style/google-rust-guide`，P1–P7 分阶段提交。
2. 本设计文档（已提交）。
3. `tasks/todo.md`：可勾选实施清单 + Review 段。
4. `docs/google-rust-style-conformance.md`：规则处置与例外说明。
5. 更新后的 `rustfmt.toml`、`src/lib.rs` lint、`.github/workflows/ci.yml`。
6. 最终验证命令与结果记录。
