# Google Rust 风格一致性报告

- Spec：`docs/superpowers/specs/2026-11-09-google-rust-style-conformance-design.md`
- Plan：`docs/superpowers/plans/2026-11-09-google-rust-style-conformance.md`
- 分支：`style/google-rust-guide`（基线 `master`，21 个提交，`165 files changed, +6612/−3805`）
- 判定原则：本项目是 `publish = false` 的应用二进制；API 设计类规则按 **strong
  recommendation** 执行，指南自身的 `Rc`/`Arc`、async、测试框架例外优先。

## 1. 基线 vs 最终指标

| 指标 | 基线（2026-11-09） | 最终 |
|---|---|---|
| `cargo test --all-targets` | 457 passed / 4 ignored / 0 failed | 457 passed / 4 ignored / 0 failed |
| `cargo test --doc` | 3 passed / 29 ignored | 3 passed / 29 ignored |
| `cargo clippy --all-targets -- -D warnings` | 干净 | 干净 |
| `cargo +nightly fmt --all -- --check` | 干净（stable、旧配置） | 干净（nightly、Google 选项） |
| `cargo build --release` | 通过 | 通过（Windows，8m04s） |
| `cargo check --no-default-features` | 通过 | 通过 |
| `cargo doc --no-deps` 警告 | 0（有 11 条旧警告，Task 9 清除） | 0 |
| 生产 `catch_unwind` | 2（`parser/cache.rs`） | 0 |
| 生产 `unwrap()` | 0 | 0（`clippy::unwrap_used` 在非测试代码为 **deny**） |
| 公开项缺文档 | 118 / 424 | 0 |
| `#[must_use]` 标注 | 0 | 105 处 |
| `.clone()` 调用点（`src/` 全仓） | 329 | 293（−36，−10.9%） |
| `Rc<` 使用点 | 8 | 8（全部为框架/测试例外，见 §3） |
| `Arc` / `Rc` / `RefCell` / `Mutex` / `RwLock` 行数 | 61 | 87 |

**关于最后一行**：该数字用同一命令（`git grep -hE "Rc<|Arc<|RefCell<|Mutex<|RwLock<" master|HEAD -- src | wc -l`）
在基线提交与最终提交上测得；spec §2 写的「约 110 行」是另一种口径，不与本行直接可比。
数字上升是 **P5 的预期结果**：把「每帧/每任务深拷贝整张 `Rule` / `Book` / 搜索结果」换成
引用计数共享后，`Arc` 出现次数增加，而 `.clone()` 与大型结构体拷贝下降。剩余 `Arc`
逐项见 §3；`Rc` 数量未变。

## 2. 逐条规则处置

提交号可在 `git log --oneline master..style/google-rust-guide` 查到。

### 2.1 强制执行（spec §4.1）

| 指南条目 | 处置 | 证据 |
|---|---|---|
| Use the appropriate formatting tool | 已应用 | `rustfmt.toml`（`max_width=100`、`imports_granularity=Module`、`group_imports=StdExternalCrate`、`format_code_in_doc_comments=true` 等）；`f46d2ac`；CI 固定 nightly（见 §5） |
| Don't use glob imports | 已应用（测试例外） | 生产代码无 glob；`src/lib.rs` 启用 `clippy::wildcard_imports`，仅 `cfg(test)` 放行 `use super::*`；非叶模块 `pub use sub::*` 保留 |
| A `use` statement should import items from one module | 已应用 | `imports_granularity=Module` + 手工拆分；`27a3a9f`（19 文件） |
| Prefer locally-scoped trait imports | 已应用 | `logger.rs` / `js/runtime.rs` 的 `FromStr` / `SubscriberExt` / `fmt::Write`；`87e1082` |
| Qualify calls to top-level functions with a module name | 已应用 | 首方自由函数改 `module::function()`；`27a3a9f` |
| Type names should be imported and used unqualified | 已应用 | `95ce903`（39 文件，+158/−125）；范围限于 brief 列出的类型 + `gpui_kit`/`std::time` |
| Name source files and leaf modules after the primary item | 已应用 | `http/cf.rs→cloudflare.rs`、`http/ua.rs→user_agent.rs`、`models/task_record.rs→download_task_record.rs`、`http/clients.rs→http_clients.rs`；`83cc439` |
| Don't implement APIs in non-leaf modules | 已应用 | `db/dao.rs`、`desktop/{actions,run}.rs`、`desktop/model/app_model.rs`、`config/global.rs`、5 个 `*_page.rs`；`cadbd09`、`2f9e2f1` |
| Naming methods / init fns / extension traits / avoid abbreviations | 已应用 | `*Cfg→*Config`、`OpsCtx→OpsContext`、`PageCtx→PageContext`、`t_*→toml_*`、`abs_url→absolute_url`、`random_ua→random_user_agent`、`from_cfg→from_config`、`with_defaults→new`、生命周期具名化；`df76a3e` |
| When and how to write documentation | 已应用 | 去掉 `#![allow(missing_docs)]`，补齐 424 个公开项中的 118 个缺口；修 Markdown / doc-on-`use` / 分裂 `//!` / `# Panics`；`1b8f066` |
| Unsafe code / No unstable features | 已应用 | `#![deny(unsafe_code)]`；无 `#![feature]`；`src/lib.rs` |
| Don't call `panic::catch_unwind` | 已应用 | 删除 `parser/cache.rs` 两处；poison 测试改 scoped thread `join()`；`a6ccb55` |
| Prefer `expect` over `unwrap` | 已应用 | 生产代码 0 `unwrap`（deny）；4 处文档示例改 `.expect("reason")`：`utils/lock.rs`、`parser/dom/selector.rs`、`db/rules/apply_default.rs`、`db/tasks.rs`；`fb8a986` |
| Don't overuse type inference | 已应用 | `config/defaults.rs`、`pages/search/search_page.rs`、`pages/settings/settings_page.rs`、`js/runtime.rs` 补显式类型；`a6ccb55` |
| Don't use iterator combinator chains when direct iteration is clearer | 已应用 | `pages/search/search_page.rs`、`pages/settings/settings_page.rs` 的 2 个 `for` 头组合链；`a6ccb55` |
| Prefer `format!` / `write!` for concatenation and substitution | **例外（1 项）** | 2 处字面量 `.replace()` 未改 `strip_*`：旧语义是全局替换，`strip_*` 会改变 `张三 作者: 李四`、`…/150.webp?foo` 的结果（`a6ccb55` 记录，见 §3） |
| Avoid `Rc` and `Arc` / Prefer borrows over deep copies | 已应用（框架例外见 §3） | `699e07f`（resolve/http client）、`6b96d9b`（`Arc<Rule>` 全链路、去 `Arc<HttpClients>`）、`1c4454e`（`Arc<Book>`、搜索/书源列表按页共享、任务持久化不克隆整表） |

### 2.2 应用代码强推荐（spec §4.2）

| 指南条目 | 处置 | 证据 |
|---|---|---|
| Almost never use `impl Into<T>` | 已应用 | 19 个签名改为具体类型（`AppError` 构造器、`AppModel::push_*`、UI builder、私有 `set_item`）；`eb6a180` |
| Prefer named fields over tuples | 已应用 | `LoadedTasks`、`ResolvedBook`、`GhProxy`；`428dfbe` |
| Implement known std traits instead of custom methods | 已应用 | 6 个配置枚举补 `FromStr`，`parse` 保留薄包装；`428dfbe` |
| Pair important `From` impls with constructors | 已应用 | `FailureRecord::new`、`TraceId::new`、`ProxySignature::from_config` + `From<&AppConfig>`；`428dfbe` |
| Important traits guidance | 已应用 | `ConfigError`（`Copy/Clone/PartialEq`，无 `Eq`——含 `f64`）、`ClientOptions`、`FetchResponse`、`UpdateCheckResult`；`428dfbe`、`f801827` |
| Options structs should implement `Default` | 已应用 | 相关 options 结构体已实现；`428dfbe` |
| Prefer enum over bool in APIs | 已应用 | `list_theme_names_by_mode(..., dark: bool)` → `ThemeVariant`；`428dfbe` |
| `#[must_use]` on functions / types | 已应用 | 105 处标注；移除 `clippy::must_use_candidate` allow；`f759651` |
| `#[non_exhaustive]` | N/A | 应用二进制、内部类型 |
| Implement expected API sets together | N/A / 保持 | 无单独存在的 `len()` |
| Use const functions responsibly | 已应用 | Task 10 补 13 个 `const fn`；Task 11 为 `new` 补 `const`；未新增不稳定 const 能力 |

### 2.3 不适用（spec §4.3）

`Prefer small crates`、`flat file layout`、`Don't use async/.await`、`Don't start new
threads through std`、`fuchsia_sync::Mutex`、`Use the appropriate import mechanism`、
`Use the right per-codebase test framework`、`Do not access #[doc(hidden)] items`、
`Avoid overloading` —— 均为 `[chrome]` / `[Fuchsia]` 标注或本仓库无对应结构，未执行。

## 3. 例外清单

spec §8 所列、**未修改**的框架强制项：

| 例外 | 理由 |
|---|---|
| `Arc<RenderImage>`（`desktop/components/logo.rs`、`pages/search/detail_dialog.rs`） | GPUI `ImageSource::Render` 即 `Arc<RenderImage>`，纹理跨帧共享 |
| `Rc<dyn Fn(...)>` listener（`components/pagination.rs`、`pages/settings/settings_page.rs` 等） | GPUI 回调必须 `Fn + 'static`；每帧重建元素，render 闭包无法调用 `cx.listener` |
| `Rc<ThemeConfig>`（`themes/apply.rs`） | gpui-kit `ThemeRegistry` 返回并存储 `Rc<ThemeConfig>` |
| `tokio::spawn` + `Arc<Semaphore>`（`crawler/download.rs`、`crawler/search.rs`、`parser/toc/single.rs`） | `Semaphore` 非 `Clone`，`acquire_owned()` 的 `'static` 许可必须 `Arc`；改 `buffer_unordered` 属跨执行器语义重构 |
| `CancelToken { Arc<AtomicBool>, Arc<Notify> }`、`Arc<dyn Fn() + Send + Sync>` | UI 与所有下载任务共享取消/唤醒信号 |
| `static OnceLock<Mutex<HashMap<_, Arc<Selector/Regex>>>>`（`parser/cache.rs`） | 跨线程共享解析缓存；`Arc` 是避免重复解析的句柄 |
| `ListCache` 的 `Arc<Vec<T>>` / `Arc<dyn Any>`（`desktop/model/list_cache.rs`） | 专门用于消除每帧整表克隆 |
| PDF `RefCell<HashMap<u32, f32>>`（`export/pdf/document.rs`） | 单线程同步导出，`char_w(&self)` 需要内部可变性 |
| 测试中的 `Rc<RefCell<_>>`、`Arc<Mutex<_>>`、`Arc<Barrier>` | 测试脚手架，无生产所有权影响 |

本任务新增／确认的「有意的偏离与遗留项」：

| 项 | 说明 |
|---|---|
| `.replace` → `strip_*` 未执行 | 旧实现是**全局**替换（对齐 Java `replaceAll`）；`strip_prefix/suffix` 对 `张三 作者: 李四` 这类输入会保留标记，属行为变更。`a6ccb55` |
| `retry_with_backoff` 每次重试 `Arc::clone(&chapter)` | 该 helper 的 `Op: FnMut(u32) -> OpFut` 要求返回**同一个** future 类型，闭包无法返回借用捕获环境的不同生命周期 future；用 `Arc<Chapter>` 共享等价于「不深拷贝」且保持签名。`1c4454e` |
| `Arc<Rule>` / `Arc<Book>` / `Arc<SearchResult>` 使用点增加 | 这是 P5 的目的：用引用计数替换深拷贝；`Rc` 未增加 |
| 书库页仍按当前页克隆 `LibraryEntry` | 计划只要求 delegate 借用渲染；page 侧仍从 `Arc<Vec<LibraryEntry>>` 复制当前页条目（`library_page.rs`），后续可改为「delegate 持 `Arc<Vec>` + 页区间」 |
| `SearchState.source_status` / `SourcesState.health` 用 `Arc` 快照 | 渲染时只 refcount bump；drain 用 `Arc::make_mut` 写回 |
| `format!`/`write!` 其余项 | 无 `+` 拼接；无新增例外 |
| CI 固定 nightly-2026-11-08 | 可复现优先；见 §5 |

## 4. 未验证项

诚实声明：以下内容本次**没有**证据。

1. **UI 外观与交互**：未启动应用点击验证；改动集中在所有权与 lint，渲染结构未变，但
   GPUI 行为无法用 `cargo test` 覆盖。
2. **跨平台**：只在本机 Windows 上完成 fmt / clippy / test / release build；Linux 与
   macOS 的编译、CI 安装的系统依赖未实际跑过。CI 的 Linux 依赖清单抄自既有 release
   workflow，未在本环境验证。
3. **CI workflow 本身**：首次运行（run 42743463426）已验证 `actions/checkout@v6`、
   `nightly-2026-11-08` 与 Linux 依赖清单都可用，`fmt` 通过；`test` 因 Rust 1.99 的
   新 lint 失败，已在 `366aa79` 修复。**修复后的运行尚未观察到**——那次运行才是本报告
   「clippy 干净」在 CI 侧的唯一证据。
4. **`--features web`**：当前 `Cargo.toml` 已无 `web` feature（构建脚本与 `web-ui/`
   均不存在），该项检查 N/A；spec 中的环境限制条目不再适用。
5. **网络相关测试**：健康检查用 RFC 5737 保留地址、搜索聚合测试不发真实请求；真实书源
   的端到端下载未验证。
6. **rustfmt 版本差异**：本地验证用的是本机 nightly，不是 CI 固定的
   `nightly-2026-11-08`；若两者 rustfmt 行为不同，CI fmt 可能要求一次重排。

## 5. 最终验证命令与结果

```text
cargo +nightly fmt --all -- --check         # 0
cargo clippy --all-targets -- -D warnings   # No issues found
cargo test --all-targets                    # 457 passed / 4 ignored / 0 failed
cargo test --doc                            # 3 passed / 29 ignored
cargo check --no-default-features           # 0
cargo build --release                       # 0（Windows，8m04s）
cargo doc --no-deps                         # 0 warnings
```

CI（`.github/workflows/ci.yml`）：

- `fmt`：nightly-2026-11-08 + rustfmt，`cargo +nightly-2026-11-08 fmt --all -- --check`；
- `test`：固定 clippy 1.99.0，安装 Linux gpui 依赖后跑 `cargo clippy --all-targets -- -D warnings`
  与 `cargo test --all-targets`。

**首次运行结果**（`f85ea36`，run 42743463426）：`fmt` 通过；`test` 在 clippy 阶段失败——
Rust 1.99 比本机的 1.98 多报四类 lint（`must_use_candidate`（非 Windows 分支的
`system_proxy::detect`）、`suboptimal_flops`、`uninlined_format_args`（仅 Unix 分支）、
`assert_is_empty`（仅测试代码）×5）。已在 `366aa79` 修掉，并把 test job 的工具链固定到
1.99.0（与 fmt job 固定 nightly 同理：pedantic/nursery 每随 stable 升级新增 lint，
不固定就无法复现）。修复后的 CI 运行尚待触发。

## 6. 建议实机点击项

1. 搜索页：聚合搜索、单源搜索、切书源、结果过滤（`search_filter`）。
2. 下载指定范围：搜索结果 →「选章」→ TOC 预取 → 起止章节下载。
3. 书源管理：健康检查（触发 `Arc<HashMap>` 快照路径）、启用/禁用、导入/删除书源文件。
4. 设置页：改字体/主题、切代理模式（触发 `rebuild_proxy`）、改语言。
5. 版本更新检查（设置页「检查更新」）。
6. 任务页：运行中任务的进度条、取消、结束后持久化（重启后历史还在）。

## 7. 提交索引

| 阶段 | 提交 |
|---|---|
| 设计 / 计划 | `657c3e3`、`886ac77`、`e9973c5` |
| P1 格式化与导入 | `f46d2ac`、`27a3a9f`、`95ce903`、`87e1082` |
| P2 模块结构 | `cadbd09`、`2f9e2f1`、`83cc439` |
| P3 命名与文档 | `df76a3e`、`1b8f066` |
| P4 API / 类型 / Trait | `eb6a180`、`428dfbe`、`f801827` |
| P5 所有权与借用 | `699e07f`、`6b96d9b`、`1c4454e` |
| P6 实现细节 | `a6ccb55`、`fb8a986` |
| P7 lint / CI / 报告 | `f759651`、本文件提交 |
