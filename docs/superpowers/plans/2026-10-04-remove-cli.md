# 移除 CLI 端实现 + 顶层目录重排 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把仓库从 GUI / CLI 两形态收敛为**纯桌面（GPUI）形态**，并按方案 B 重排顶层目录，不留下孤儿代码、失效文档、断链注释或已无消费者的依赖。

**Architecture:** 纯删除 + 目录迁移重构，没有新功能。改动分五层：编译面（`src/cli` / `src/startup` / `main.rs` / Cargo 依赖）、共享层孤儿（`core::bootstrap` 4 个 CLI-only 函数 + `utils::tty` + `logger::init_compat_legacy` + `i18n` 重复函数合并）、i18n（`Cli:` 词条段）、资源路径面（`bundle/` 拆解到 `assets/rules/` 与 `tests/fixtures/web/`）、外围文档链路。

**Tech Stack:** Rust 2024（单一 lib crate，非 workspace）、`cargo` 质量门（fmt / clippy `-D warnings` / test）、`rust-i18n`（编译期嵌入 `locales/app.yml`）、GPUI（gpui-kit 0.7）。

## Global Constraints

以下约束适用于**每一个** Task，逐字抄自设计文档 `docs/superpowers/specs/2026-10-04-remove-cli-design.md` 与项目规则：

- **不提交**：本项目 `tasks/lessons.md` L1 规定「只有用户明确说"提交"或"commit"时才执行 git commit」。**本计划的任何步骤都不含 `git commit`**。每个 Task 完成后停下，等待用户确认；全部完成后由用户决定是否提交。
- **不打 tag、不建分支**：可恢复性完全依赖 git history，不创建 `cli-archive` 之类的存档引用。
- **保留 `tokio`**：桌面 model / crawler / parser / http 大量在用，仅摘 `signal` feature（唯一消费者是已删的 CLI Ctrl-C）。
- **`bundle/web/` 的 7 个样例文件只挪位置、不删**（`chapter.html` / `cover.jpg` / `js/*.js`）—— 书源解析的真实参考，2 个单元测试在读。
- **保留** `docs/superpowers/`（历史规划 / 设计记录）、`locales/app.yml` 除 `Cli:` 外全部段、`scripts/`、`.githooks/`、`.github/workflows/release.yml`。
- **不碰 `src/desktop/` 分层**，唯一例外是 Task 2 把 `locale_for_gpui` 调用点改回 `locale_for`（一行 import + 一行调用 + 一段 doc）。
- **测试基线（2026-10-04 实测）**：`cargo test --lib` = **485 passed / 0 failed / 4 ignored**。
  最终目标 = **439 passed / 0 failed / 4 ignored**（减少 46：`cli` 33 + `core::bootstrap` 10 + `utils::tty` 2 + `logger` 1）。
- 所有命令在仓库根 `C:\Users\pc\Documents\GitHub\so-novel-rs` 执行。

### 关于 TDD

本次是**删除型重构**，没有"先写失败测试"的自然循环（被删代码的测试就是删掉的测试；目录迁移不改变行为）。替代的验证纪律是：

1. 每个 Task 结束都跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings`，保持 0 警告。
2. 删除型 Task 用**全仓 grep 断言 0 命中**证明没有残留引用。
3. 目录迁移 Task 用**未被删的既有测试**（`db::rules` 3 个、`js::runtime` 1 个、`parser::dom::selector` 1 个）在新路径下继续通过作为直接证据。
4. 编译期 `include_str!` / `include_bytes!` 是硬约束：路径漏改会直接编译失败，不会静默。

### 与设计文档的差异（已核实，计划按此执行）

| # | 设计文档写的 | 实际情况 | 处理 |
|---|---|---|---|
| 1 | 只改 `src/i18n.rs:18,32` 的 CLI 注释措辞 | `locale_for` 与 `locale_for_gpui` 是**逐字相同的两个函数**，拆分理由（「CLI 走前者、桌面走后者」）随 CLI 消失而失效；`locale_for` 删除 CLI 后只剩自身测试在调，成为孤儿 | **合并**：删 `locale_for_gpui`，`desktop::run` 改用 `locale_for`。合并后 `locale_for` 有真实消费者，无孤儿 |
| 2 | — | `src/cli/sources.rs` 另有 **4 个**测试（设计文档已在验证节计入 33） | 随 `src/cli/` 删除 |
| 3 | 只提 `tokio` 摘 `signal` | `fs` feature 在 `src/` 内无直接使用点（仅 `src/db/mod.rs:7` 注释提及），但可能由 reqwest 等传递依赖启用 | **只摘 `signal`**（确认 CLI-only）；`fs` 保留，避免无收益的依赖面震荡 |
| 4 | 未提 `src/config/mod.rs:34`、`src/logger.rs:7` | 这两处 doc 注释含 `startup` / `cli::run` 字样 | 一并改写（见 Task 2） |

---

## Task 1: 切断 CLI 入口与启动层（原子切换）

这一 Task 必须原子完成：只要 `src/cli` 或 `src/startup` 还在，`main.rs` / `lib.rs` / Cargo 就不能先改，否则 crate 编译不过。

**Files:**
- Delete: `src/cli/`（6 文件：`mod.rs` / `args.rs` / `search.rs` / `download.rs` / `sources.rs` / `tests.rs`）
- Delete: `src/startup/`（1 文件：`mod.rs`）
- Modify: `src/lib.rs:42,56`
- Modify: `src/main.rs`（整文件）
- Modify: `Cargo.toml:20,89,107-112`

**Interfaces:**
- Consumes: `so_novel_rs::logger::init()`（`src/logger.rs:50`，无参无返回）、`so_novel_rs::desktop::run() -> anyhow::Result<()>`（`src/desktop/mod.rs:77`）。
- Produces: crate 只暴露 `lib` + GUI `bin`，无 CLI 入口。后续 Task 依赖此状态。

- [ ] **Step 1: 记录改动前基线**

Run: `cargo test --lib 2>&1 | tail -3`
Expected: `test result: ok. 485 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; 4 ignored`（实际以你机器输出为准；把 `485` 记下来，Task 6 核对）。若数字不是 485，说明工作区与设计文档基线有偏差 —— **停下并告知用户**，不要继续。

- [ ] **Step 2: 删除 CLI 与启动层目录**

> ⚠️ 这一步会丢弃工作区里**未提交**的改动，其中包含上一个任务「CLI 后台任务增量反馈优化」对 `src/cli/{search,download}.rs` 与 `src/utils/tty.rs` 的全部编辑。用户已确认该任务作废。

Run:
```bash
rm -rf src/cli src/startup
ls src/cli src/startup 2>&1
```
Expected: `ls: cannot access 'src/cli': No such file or directory` + 同样一行 `src/startup`。

- [ ] **Step 3: `src/lib.rs` 去掉两个模块声明**

Modify `src/lib.rs`，删除这两行（`pub mod cli;` 在 L42、`pub mod startup;` 在 L56）：

```rust
pub mod cli;
```
```rust
pub mod startup;
```

删除后 `pub mod` 列表应为：`config / core / crawler / db / desktop / error / export / http / i18n / js / logger / models / parser / utils`（14 个）。

- [ ] **Step 4: `src/main.rs` 内联启动逻辑**

用以下内容整体替换 `src/main.rs`（当前 16 行）：

```rust
// Windows release 下走 GUI subsystem，避免启动时弹出控制台黑窗。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;

fn main() -> Result<()> {
    so_novel_rs::logger::init();
    so_novel_rs::desktop::run()
}
```

- [ ] **Step 5: `Cargo.toml` 摘除 CLI 专属依赖**

Modify `Cargo.toml`，三处：

(a) 删 `clap` 依赖及其上方注释（当前 L88-89）：
```toml
# CLI 子命令。
clap = { version = "4", features = ["derive"] }
```

(b) `tokio`（当前 L20）从 features 列表移除 `"signal"`：
```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "fs"] }
```

(c) 删整个 Windows console attach 依赖块（当前 L106-112）：
```toml
# 当前只保留 CLI 模式需要的 console attach feature（DWM 圆角 / 暗色标题栏未在 GPUI 路径启用）。
[target.'cfg(target_os = "windows")'.dependencies]
windows-sys = { version = "0.61", features = [
    # CLI 启动时把 stdio 附到父进程控制台（cmd / PowerShell），让 GUI subsystem 的 exe 也能输出。
    "Win32_System_Console",
] }
```

> **保留** 文件末尾的 `[target.'cfg(target_os = "windows")'.build-dependencies] winres`（exe 图标，与 CLI 无关）。

- [ ] **Step 6: 编译验证**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished \`dev\` profile`，无 `error`。若报 `unresolved import crate::cli` / `crate::startup` 之类，说明 Step 3-5 有遗漏，回查。

- [ ] **Step 7: 质量门**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```
Expected: fmt 无输出（0 diff）；clippy `Finished` 无 `warning`。若 clippy 报 unused import，通常是 Task 2 范围的孤儿暂时未被引用（`pub` 项不会报），确认不是 Step 3-5 遗漏即可。

- [ ] **Step 8: grep 断言 CLI 引用已断**

Run:
```bash
grep -rn 'crate::cli\|cli::run\|startup::dispatch\|startup::detect\|attach_parent_console' src/
grep -n 'clap\|windows-sys' Cargo.toml
```
Expected: 两条均无输出。若有命中，回 Step 3-5 补齐。

- [ ] **Step 9: 停下等用户确认**

不 commit。汇报：删了 7 个文件、`main.rs` 内联完成、Cargo 摘 2 依赖 + 1 feature、`cargo build` / fmt / clippy 全绿。等用户说继续再做 Task 2。

---

## Task 2: 清理 Rust 孤儿与失效措辞

**Files:**
- Modify: `src/core/bootstrap.rs`（整文件重写）
- Modify: `src/logger.rs:6-7,93-105,131-140`
- Delete: `src/utils/tty.rs`
- Modify: `src/utils/mod.rs:1-9,16`
- Modify: `src/i18n.rs:13-39`
- Modify: `src/desktop/mod.rs:54-59,124`
- Modify: `src/config/mod.rs:34`

**Interfaces:**
- Consumes: Task 1 的 GUI-only crate 状态。
- Produces: `core::bootstrap` 仅保留 `AppContext` + `load_context()`；`i18n::locale_for` 成为桌面唯一 locale 映射函数（`locale_for_gpui` 不存在）。

- [ ] **Step 1: 重写 `src/core/bootstrap.rs`**

删除 `effective_cfg` / `load_active_sources` / `validate_range` / `cli_load_paths_and_config` 四个函数与其 `mod tests`（10 个测试全针对被删函数）。用以下内容整体替换当前 259 行：

```rust
//! desktop 启动期的公共资源加载。
//!
//! `AppContext::load_context` **不**返回 `Result`：内部所有 IO 失败都吞掉 + warn，返回
//! 尽力凑齐的 `AppContext`（desktop 各处自行 `tracing::warn!` + 兜底默认，不在这里 panic）。

use std::sync::Arc;

use crate::config::{AppConfig, ConfigPaths};
use crate::db::{SourcesConfig, init_rules_dir, load_active_rules};
use crate::http::HttpClients;
use crate::models::Rule;

/// 启动期公共资源聚合（`paths` + `config` + `sources_config` + `rules` + `http`）。
/// desktop 拿它加 `tasks / runtime / wakeup` 拼 state。
pub struct AppContext {
    pub paths: ConfigPaths,
    pub config: AppConfig,
    pub sources_config: SourcesConfig,
    pub rules: Vec<Rule>,
    pub http: Arc<HttpClients>,
}

/// 启动期"凑齐所有公共资源"的统一入口。**不**返回 `Result`。
///
/// 所有失败一律 `tracing::warn!` + 兜底默认：`load_config` → 默认配置；首次写 config /
/// `init_rules_dir` / 首次写 `sources_config` → 仅 warn 不阻塞；`load_active_rules` → 空
/// Vec；`HttpClients::new` → 默认 client。
///
/// # Panics
///
/// 不会 panic：所有失败路径都 swallow 走兜底。
pub fn load_context() -> AppContext {
    let paths = ConfigPaths::discover();

    // config.toml
    let (config, config_err) = match crate::config::load_config(&paths.config_file) {
        Ok(c) => (c, None),
        Err(e) => {
            tracing::warn!("config load failed: {e:#}");
            (AppConfig::default(), Some(format!("{e:#}")))
        }
    };
    // 首次启动写出默认 config（让用户立刻能在项目根看到 config.toml 可编辑）
    if !paths.config_file.exists() {
        if let Err(e) = crate::config::save_config(&paths.config_file, &config) {
            tracing::warn!("写入默认 config.toml 失败: {e:#}");
        } else {
            tracing::info!("首次启动：已生成 {}", paths.config_file.display());
        }
    }
    // config_err 暂未暴露 —— desktop 之后如需在 UI 上提示，可加进 AppContext。
    let _ = config_err;

    // 规则目录（首次启动时复制默认规则文件）
    if let Err(e) = init_rules_dir(&paths.rules_dir) {
        tracing::warn!("规则目录初始化失败: {e:#}");
    }

    let sources_config = SourcesConfig::load(&paths.sources_config);
    if !paths.sources_config.exists()
        && let Err(e) = sources_config.save(&paths.sources_config)
    {
        tracing::warn!("写入默认 sources_config.json 失败: {e:#}");
    }

    let rules = match load_active_rules(&paths.rules_dir, &sources_config) {
        Ok(rs) => rs,
        Err(e) => {
            tracing::warn!("rules load failed: {e:#}");
            Vec::new()
        }
    };

    // 常见失败原因是 proxy URL 畸形：原始 cfg → 关闭 proxy → 空 stub 三步 fallback。
    let http = match HttpClients::new(&config) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::warn!("HttpClients init failed: {e:#}，尝试关闭 proxy 后重试");
            let mut cfg_no_proxy = config.clone();
            cfg_no_proxy.proxy.proxy_enabled = false;
            match HttpClients::new(&cfg_no_proxy) {
                Ok(c) => Arc::new(c),
                Err(e2) => {
                    tracing::error!("HttpClients init 重试仍失败: {e2:#}；fall back to empty stub");
                    Arc::new(HttpClients::empty())
                }
            }
        }
    };

    AppContext {
        paths,
        config,
        sources_config,
        rules,
        http,
    }
}
```

> `use anyhow::{Context, Result};` 整行删除（删掉 4 个函数后无任何 anyhow 使用点）。
> `ExportFormat` 从 config import 中移除。

- [ ] **Step 2: 编译 + clippy 验证 Step 1**

Run:
```bash
cargo build 2>&1 | tail -5
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
```
Expected: 均 `Finished` 无 warning。若报 `unused import: anyhow` / `ExportFormat`，说明还有残留，回查。

- [ ] **Step 3: 删除 `src/utils/tty.rs` 与模块声明**

Run: `rm src/utils/tty.rs`

Modify `src/utils/mod.rs`：删除 `pub mod tty;` 一行，并把模块头（当前 L1-9）替换为：

```rust
//! 通用工具集合：**纯函数** + **零业务依赖**，供 `crawler` / `desktop` 复用。
//!
//! 子模块：`formatting`（字符串 / 时间 / 大小）、`fs`（文件名、日志脱敏、绝对路径）、
//! `lang`（系统 locale）、`lock`（锁 poison 防护）、`system`（系统程序打开）、`time`
//! （unix 时间戳）、`zhconv`（简繁转换）。
//!
//! 边界：HTTP / 编码 / i18n **不**下沉到这里；唯一例外是 `zhconv::convert_book_meta`
//! 收 `Book` 入参，但只读字段。
```

- [ ] **Step 4: 删除 `logger::init_compat_legacy` 与其测试**

Modify `src/logger.rs`：

(a) 改写模块头 L6-7（当前含 `cli::run` 字样）：
```rust
//! - **`tracing_subscriber::init` 全局唯一，二次 init 会 panic**，须由 caller 自行保证：
//!   `main` 只在启动时调一次 `logger::init()`
```

(b) 删除整个 `init_compat_legacy` 函数及其上方 doc 注释（当前 L93-105）：
```rust
/// 旧 `init_tracing` 别名, 保留给 cli 启动期调用。二次 init 静默 no-op (而非 panic), 可放心多次调。
pub fn init_compat_legacy() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,so_novel_rs=debug"));

    let layer = fmt::layer().with_target(false);
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init();
}
```

(c) 删除测试函数 `init_compat_legacy_does_not_panic`（当前 L131-140 起）：
```rust
    #[test]
    fn init_compat_legacy_does_not_panic() {
        // set_default 限定在当前线程, 测试结束自动恢复。
        let _ = tracing_subscriber::registry()
            .with(EnvFilter::new("off"))
            .with(fmt::layer().with_target(false))
            .set_default();
        init_compat_legacy();
        init_compat_legacy();
    }
```

> `init_with_format`（L59）仍在用 `EnvFilter` / `fmt` / `SubscriberInitExt`，故 L11-12 的 import **全部保留**。

- [ ] **Step 5: 合并 `i18n` 的重复 locale 映射**

Modify `src/i18n.rs`：删除 `locale_for_gpui`（当前 L28-39，含 doc），并把 `locale_for` 的 doc（当前 L13-19）替换为：

```rust
/// 把 `Language` 映射到本项目 `app.yml` 用的 locale 标签（`gpui_kit::component` 接受同一套标签）。
///
/// **`Language → locale 字符串` 的唯一权威映射**。
/// `TraditionalChinese` → `"zh-TW"`，**不是** `gpui_kit::component` 旧版用的 `"zh-HK"`。
pub const fn locale_for(lang: Language) -> &'static str {
```

Modify `src/desktop/mod.rs`：

(a) L54-59 替换为：
```rust
/// 把 `AppConfig.language`（应用语言）映射到 `gpui_kit::component` 接受的 locale 字符串。
///
/// **只**对应"应用 UI 语言"（`Language`），跟"书源语言"（`LangType`）无关。
use crate::i18n::locale_for;
```

(b) L124 把 `locale_for_gpui(...)` 改为 `locale_for(...)`：
```rust
        gpui_kit::component::set_locale(locale_for(model.read(cx).config.global.language));
```

- [ ] **Step 6: 改写 `set_global` 的启动期注释**

Modify `src/config/mod.rs` L34：

```rust
/// 注入全局配置。仅在启动期（`main`）调一次；重复调用返回 `Err`，
```

- [ ] **Step 7: 编译 + clippy + 全测试验证**

Run:
```bash
cargo build 2>&1 | tail -5
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo test --lib 2>&1 | tail -3
```
Expected:
- build / clippy：`Finished` 无 warning。
- test：`439 passed; 0 failed; ...; 4 ignored`（相对 Task 1 基线再减 13 = bootstrap 10 + tty 2 + logger 1）。

- [ ] **Step 8: grep 断言孤儿已清**

Run:
```bash
grep -rn 'locale_for_gpui\|init_compat_legacy\|utils::tty\|cli_load_paths_and_config\|effective_cfg\|load_active_sources\|validate_range' src/
```
Expected: 无输出。

- [ ] **Step 9: 停下等用户确认**

不 commit。汇报：`bootstrap` 由 259 行降至 ~105 行、tty 模块删除、logger 孤儿删除、i18n 合并、测试 439 passed。

---

## Task 3: 删除 i18n 的 `Cli:` 段

**Files:**
- Modify: `locales/app.yml:1247-1473`

**Interfaces:**
- Consumes: Task 1-2 已删除所有 `t!("Cli.*")` 调用方（唯一调用点在 `src/cli/args.rs`）。
- Produces: `app.yml` 最后一个顶层段变为 `Search:`。

- [ ] **Step 1: 确认 `Cli:` 是文件尾段且无其他消费者**

Run:
```bash
grep -rn 'Cli\.\|"Cli' src/ | grep -v '^Binary'
grep -n '^Cli:' locales/app.yml
wc -l locales/app.yml
```
Expected: 第一条无输出（Task 1 已删调用方）；第二条 `1247:Cli:`；第三条 `1473 locales/app.yml`。若第一条有命中，**停下** —— 有残留调用方。

- [ ] **Step 2: 删除 `Cli:` 段**

从 `locales/app.yml` 删除 L1247 起至文件末尾（`Cli:` 到最后的 `sources_disable_id_help` 三语块）的全部内容，即删掉整个顶层 `Cli:` 段。删除后文件应以上一段 `Search:` 的最后一个 key 结尾。

- [ ] **Step 3: 验证 YAML 仍可被编译期解析 + 测试不变**

Run:
```bash
cargo build 2>&1 | tail -5
cargo test --lib 2>&1 | tail -3
tail -4 locales/app.yml
grep -n '^Cli:' locales/app.yml
```
Expected:
- build `Finished`（`rust_i18n::i18n!` 编译期解析 YAML，缩进/语法错会直接编译失败）。
- test 仍 `439 passed`。
- `grep` 无输出。

- [ ] **Step 4: 停下等用户确认**

不 commit。

---

## Task 4: 顶层目录重排 + 路径引用同步

**Files:**
- Move: `bundle/web/` → `tests/fixtures/web/`（7 文件）
- Move: `bundle/rules/` → `assets/rules/`（6 文件）
- Move: `screenshots/` → `docs/screenshots/`（4 png）
- Delete: `bundle/`（迁移后的空目录）
- Modify: `src/db/rules/constants.rs:19,22,26,30,34`
- Modify: `src/db/rules/loader.rs:167`
- Modify: `src/db/rules/init.rs:82`
- Modify: `src/js/runtime.rs:117-120`
- Modify: `src/parser/dom/selector.rs:421`
- Modify: `.github/workflows/release.yml:79,95,107`
- Modify: `scripts/package-linux.sh:6,74`
- Modify: `.gitignore:13-16`

**Interfaces:**
- Consumes: 无（与 Task 1-3 独立，但建议在其后做，便于定位失败）。
- Produces: `bundle/` 不再存在；规则数据在 `assets/rules/`、解析样例在 `tests/fixtures/web/`、截图在 `docs/screenshots/`。

- [ ] **Step 1: 迁移三个目录**

Run:
```bash
mkdir -p tests/fixtures
git mv bundle/web tests/fixtures/web
git mv bundle/rules assets/rules
rmdir bundle
git mv screenshots docs/screenshots
```
Expected: 无报错。若 `git mv` 报 "destination exists"，说明目标已存在，先 `ls` 排查，**不要**加 `-f` 硬覆盖。

- [ ] **Step 2: 验证迁移结果**

Run:
```bash
git ls-files tests/fixtures/web | wc -l
git ls-files assets/rules | wc -l
git ls-files docs/screenshots | wc -l
ls bundle 2>&1
```
Expected: `7`、`6`、`4`；`ls: cannot access 'bundle': No such file or directory`。

- [ ] **Step 3: 改编译期 `include_str!` 路径**

Modify `src/db/rules/constants.rs` 的 `BUNDLED_RULES`：把 5 处 `include_str!("../../../bundle/rules/X")` 全部改为 `include_str!("../../../assets/rules/X")`，即：

```rust
pub(super) const BUNDLED_RULES: &[(&str, &str)] = &[
    ("main.json", include_str!("../../../assets/rules/main.json")),
    (
        "cloudflare.json",
        include_str!("../../../assets/rules/cloudflare.json"),
    ),
    (
        "no-search.json",
        include_str!("../../../assets/rules/no-search.json"),
    ),
    (
        "rate-limit.json",
        include_str!("../../../assets/rules/rate-limit.json"),
    ),
    (
        "proxy-required.json",
        include_str!("../../../assets/rules/proxy-required.json"),
    ),
];
```

- [ ] **Step 4: 改 4 处测试 helper 路径**

Modify `src/db/rules/loader.rs:167`（`repo_rules_dir()` 内）：
```rust
    fn repo_rules_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("rules")
    }
```

Modify `src/db/rules/init.rs:82`（同名 helper，同样两行）：
```rust
    fn repo_rules_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("rules")
    }
```

Modify `src/js/runtime.rs:117-120`：
```rust
    fn repo_web() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("web")
    }
```

Modify `src/parser/dom/selector.rs:421`（`parses_real_chapter_html_resource` 内）：
```rust
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("web")
            .join("chapter.html");
```

- [ ] **Step 5: 编译 + 目录迁移的直接证据测试**

Run:
```bash
cargo build 2>&1 | tail -5
cargo test --lib db::rules 2>&1 | tail -3
cargo test --lib js::runtime 2>&1 | tail -3
cargo test --lib parser::dom::selector 2>&1 | tail -3
```
Expected:
- build `Finished`（若报 "couldn't read assets/rules/main.json"，说明 Step 1 迁移未生效，回查）。
- 三个过滤测试全部 `test result: ok`，合计覆盖 `db::rules` 3 + `js::runtime` 1 + `parser::dom::selector` 1 = 5 个读样例文件的测试 —— **这就是新路径正确的证据**。

- [ ] **Step 6: 改 CI 与打包脚本**

Modify `.github/workflows/release.yml`：

(a) L79 注释里 `bundle/rules/` → `assets/rules/`：
```yaml
      # ---- 打包：Linux 复用 scripts/package-linux.sh（HTML 模板 / logo 已由 include_*! 嵌入二进制，
      # assets/rules/ 随包携带）；macOS / Windows 在此就地打 ----
```

(b) macOS 段 L95 与 Windows 段 L107 两处同样的 `cp`：
```bash
          cp -r assets/rules "dist/$STAGE/rules"
```

Modify `scripts/package-linux.sh`：

(a) L6 注释 `bundle/rules/` → `assets/rules/`：
```bash
# 产物：dist/so-novel-rs-<version>-linux-<arch>.tar.gz（可执行文件 + README + assets/rules/）。
```

(b) L74：
```bash
cp -r assets/rules "$OUTDIR/rules"
```

- [ ] **Step 7: 清理 `.gitignore` 的 bundle 规则**

Modify `.gitignore`，删除这 4 行（当前 L13-16，`bundle/` 已不存在）：
```
/bundle/*.db
/bundle/*.db-journal
/bundle/*.db-wal
/bundle/*.db-shm
```

- [ ] **Step 8: grep 断言无 `bundle` 残留**

Run:
```bash
grep -rn 'bundle/' src/ scripts/ .github/ .gitignore docs/BOOK_SOURCES.md README.md Cargo.toml
```
Expected: 无输出。

- [ ] **Step 9: 全测试 + clippy**

Run:
```bash
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo test --lib 2>&1 | tail -3
```
Expected: clippy `Finished` 无 warning；test `439 passed; 0 failed; ...; 4 ignored`（与 Task 2 相同 —— 迁移不改测试数）。

- [ ] **Step 10: 停下等用户确认**

不 commit。汇报：`bundle/` 拆解完成（rules→assets/rules、web→tests/fixtures/web），截图入 docs/screenshots，路径引用 12 处同步，5 个样例消费测试在新路径通过。

---

## Task 5: 文档同步

**Files:**
- Modify: `README.md`（导航 L17、功能表 L46、结构树 L67-87、分层 L89、书源路径 L128/157/198、贡献 L199、删 CLI 节 L133-153）
- Modify: `docs/BOOK_SOURCES.md`（L3-4、L119、L147、L165-166、L178）
- Modify: `docs/CHANGELOG.md`（追加 `[Unreleased]` 条目）
- Delete: `docs/CLI.md`（若 Task 4 前仍在）

**Interfaces:**
- Consumes: Task 4 的最终目录布局。
- Produces: 文档与代码/目录一致，无断链。

- [ ] **Step 1: 删除 `docs/CLI.md`**

Run:
```bash
rm docs/CLI.md
ls docs/CLI.md 2>&1
```
Expected: `ls: cannot access 'docs/CLI.md': No such file or directory`。

- [ ] **Step 2: README 导航与功能表**

Modify `README.md` L17（删掉 ` · [CLI](#-cli-用法)` 片段）：
```markdown
[功能](#-功能) · [安装](#-安装) · [技术栈](#-技术栈) · [快速开始](#-快速开始) · [快捷键](#-快捷键) · [免责声明](./DISCLAIMER.md)
```

Modify `README.md` L46：删除整行 CLI 功能行：
```markdown
| 💻 **CLI 模式** | `search` / `download` / `sources` 子命令，`--json` 机器可读输出 |
```

- [ ] **Step 3: README 项目结构树与分层段**

Modify `README.md`，把 L67-87 的 fenced 结构树整体替换为：

```
so-novel-rs/
├── assets/                # 编译期嵌入的静态资源
│   ├── logo.*             # 图标（build.rs 嵌 exe 资源段 + 侧栏 logo）
│   ├── chapter_*.tmpl     # HTML / EPUB 导出模板
│   └── rules/             # 默认书源 JSON + 模板（首次启动复制到 ~/.sonovel/rules/）
├── docs/                  # 长文档 + 截图 + 历史设计记录
│   └── screenshots/
├── locales/app.yml        # i18n 翻译（zh-CN / zh-TW / en）
├── tests/fixtures/web/    # 书源解析样例（章节页 / 封面 / JS）
└── src/
    ├── main.rs / lib.rs   # 入口 + crate 根
    ├── core/              # 业务层（与 GUI 解耦）
    ├── desktop/           # GPUI 桌面 GUI（components / model / pages / themes/）
    ├── parser/            # HTML 解析（book / chapter / toc / dom 子模块）
    ├── crawler/           # 搜索 / 下载 / 重试 / 健康检测
    ├── export/            # EPUB / TXT / HTML / PDF（含 pdf/ 子模块）
    ├── http/ js/ db/      # HTTP 客户端 / boa_engine / 持久化
    ├── config/ models/    # config.toml 读写 / 数据模型
    ├── i18n.rs error.rs   # 翻译入口 + 顶层错误
    ├── logger.rs utils/   # tracing 初始化 + 工具函数
```

Modify `README.md` L89（分层段，去掉 `cli/`）：
```markdown
**分层**: `core/` 提供与 GUI 解耦的业务逻辑，`desktop/` 是 GPUI 渲染层，两者共享同一份核心代码。桌面端的文案与错误提示统一走 `rust_i18n` 的 `t!` 宏按全局 locale 翻译。
```

- [ ] **Step 4: README 删除 CLI 用法节**

Modify `README.md`：删除从 `### 💻 CLI 用法` 到 `📖 完整 CLI 用法…见 [docs/CLI.md](./docs/CLI.md)。` 的整节（当前 L133-153，含标题、说明、sh 代码块、help 说明、末行链接），及其前后各保留一个空行的区间。

- [ ] **Step 5: README 剩余路径与措辞**

Modify `README.md` 三处路径：

(a) L128：
```markdown
| `rules/` | 书源规则文件（JSON，首次启动从内置资源复制） |
```

(b) L157：
```markdown
仓库自带 6 套书源规则（位于 `assets/rules/`，首次运行复制到 `~/.sonovel/rules/`）：
```

(c) L198：
```markdown
* 新增书源 → 走 `assets/rules/` JSON,规则语法见 `rule-template.json5`(如存在)
```

(d) L199：
```markdown
* 业务函数返回错误 → 用 `AppResult<T>` + `?` 透传;边界才转 `anyhow`
```

- [ ] **Step 6: `docs/BOOK_SOURCES.md` 路径与链接**

Modify `docs/BOOK_SOURCES.md`：

(a) L3-4：
```markdown
本文档对应 `assets/rules/` 下的书源文件。**书源规则文件均位于
`assets/rules/xx.json`**（首次运行会复制到 `~/.sonovel/rules/`）。
```

(b) L119：
```markdown
`assets/rules/` 下的 5 个书源 JSON 都是**可选的活跃文件**。切换有 3 种方式：
```

(c) L147（去掉 CLI 参与写盘的表述）：
```markdown
> 这个文件由 GUI 书源管理页写入（用
> [`SourcesConfig::save`](../src/persistent/sources_config.rs) 原子写）。
```

(d) L165-166（相对链接同步）：
```markdown
- [`assets/rules/rule-template.json5`](../assets/rules/rule-template.json5) — 模板文件，含字段说明
- [`assets/rules/main.json`](../assets/rules/main.json) — 实际书源集，看真实例子
```

(e) L178：删除指向已删文件的链接行：
```markdown
- [CLI.md](./CLI.md) — CLI 用法
```

- [ ] **Step 7: `docs/CHANGELOG.md` 追加条目**

Modify `docs/CHANGELOG.md`：在现有 `### Removed` 段的 web 条目之后、`## [Unreleased]` 段内，追加以下 bullets；并把 web 条目末句 L21 改为顺序叙述。

把 L21 这行：
```markdown
仓库由此收敛为 **GUI + CLI** 两形态。`cargo test --lib` 由 510 passed 降至 487 passed。
```
替换为：
```markdown
仓库由此收敛为 **GUI + CLI** 两形态（CLI 端在后续提交中一并移除，见下）。`cargo test --lib` 由 510 passed 降至 487 passed。
```

紧接着追加：

```markdown
- **CLI 端整体移除**：`src/cli/`（6 文件）、`src/startup/`、`src/utils/tty.rs`、
  `docs/CLI.md`、`locales/app.yml` 的 `Cli:` 段；依赖删 `clap` 与 `windows-sys`
  （`Win32_System_Console`），`tokio` 摘 `signal` feature
- **孤儿清理**：`core::bootstrap` 删 `effective_cfg` / `load_active_sources` /
  `validate_range` / `cli_load_paths_and_config`（连带 10 个测试）；`logger` 删
  `init_compat_legacy`（连带 1 个测试）；`i18n` 合并重复的 `locale_for_gpui` 回 `locale_for`
- **顶层目录重排**：`bundle/rules/` → `assets/rules/`、`bundle/web/` →
  `tests/fixtures/web/`、`screenshots/` → `docs/screenshots/`，`bundle/` 目录消失

仓库由此收敛为**纯桌面（GPUI）形态**。`cargo test --lib` 由 485 passed 降至 439 passed。
```

- [ ] **Step 8: grep 断言文档无死引用**

Run:
```bash
grep -rn 'CLI\.md\|bundle/\|/cli/\|startup/' README.md docs/BOOK_SOURCES.md docs/CHANGELOG.md
```
Expected: 无输出。若 `docs/CHANGELOG.md` 因新增条目里的历史叙述命中，逐条判断是否合法（合法则改 grep 范围）。

- [ ] **Step 9: 停下等用户确认**

不 commit。汇报：README 去掉 CLI 节与三处路径、BOOK_SOURCES 路径 + 死链清理、CHANGELOG 追加本轮条目。

---

## Task 6: 全量验收

**Files:**
- Modify: `tasks/todo.md`（追加本轮 plan + Review 段；`tasks/` 已被 `.gitignore` L21 忽略，不进 commit）
- Modify: `tasks/lessons.md`（若有新教训）

**Interfaces:**
- Consumes: Task 1-5 全部完成。
- Produces: 可交付状态（等用户决定是否提交）。

- [ ] **Step 1: 质量门三连**

Run:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --lib 2>&1 | tail -3
```
Expected: fmt 0 diff；clippy `Finished` 无 warning；test `439 passed; 0 failed; ...; 4 ignored`。

- [ ] **Step 2: 依赖面验收**

Run:
```bash
grep -n 'clap\|windows-sys' Cargo.toml
cargo tree -e normal -i clap 2>&1 | head -3
```
Expected: 第一条无输出。第二条允许有输出（`clap` 可能作为 `gpui-kit` / `rfd` 的传递依赖仍存在于依赖树）——**只要 `Cargo.toml` 不再直接声明即可**。

- [ ] **Step 3: 代码 / 文档零残留验收**

Run:
```bash
grep -rn 'crate::cli\|cli::\|startup::\|utils::tty\|bundle/' src/ README.md docs/BOOK_SOURCES.md scripts/ .github/ locales/
ls bundle 2>&1
```
Expected: grep 无输出；`ls: cannot access 'bundle'`。

- [ ] **Step 4: 目录终态验收**

Run:
```bash
ls -d assets docs locales scripts src tests 2>&1
git ls-files tests/fixtures/web | wc -l
ls assets/rules
ls docs/screenshots
```
Expected: 6 个目录全存在；`7`；`assets/rules` 列 6 个文件（5 json + `rule-template.json5`）；`docs/screenshots` 列 4 png。

- [ ] **Step 5: release 构建验收**

Run: `cargo build --release 2>&1 | tail -5`
Expected: `Finished \`release\` profile`。这一步同时验证 `windows_subsystem` 属性在 release 下合法、`winres` 图标嵌入仍工作。

- [ ] **Step 6: 手动 GUI 冒烟（关键，验证桌面端未被误伤）**

Run: `cargo run`（release 构建产物亦可：`./target/release/so-novel-rs.exe`）

逐项确认：
1. 窗口正常打开，无黑窗 / 无 panic 对话框。
2. 侧栏 5 个页面（搜索 / 任务 / 书库 / 书源 / 设置）都能点开。
3. 书源页能列出书源（证明 `assets/rules/` 的新路径在 `load_context` → `init_rules_dir` 链路正常；首次运行会复制到 `~/.sonovel/rules/`）。
4. 搜索页输入关键词能出结果（证明 crawler / parser 未受影响）。

若窗口起不来或书源为空，**停下**并汇报 —— 最可能原因是 Task 4 的 `include_str!` / 迁移路径有误。

- [ ] **Step 7: 追加 `tasks/todo.md` Review + `tasks/lessons.md`**

按项目规约在 `tasks/todo.md` 末尾追加本轮 Review 段（已完成项 / 与计划的偏差 / 验证结果 / 未提交状态），如有新教训追加到 `tasks/lessons.md`。

- [ ] **Step 8: 停下等用户决定提交**

**不 commit**。汇报改动清单与验证证据，由用户说「提交」后再执行 `git add` + `git commit`。

---

## 完成标准

| 项 | 期望 |
|---|---|
| `cargo fmt --all -- --check` | 0 diff |
| `cargo clippy --all-targets -- -D warnings` | 0 warning |
| `cargo test --lib` | 439 passed / 0 failed / 4 ignored |
| `cargo build --release` | 成功 |
| `Cargo.toml` 直接依赖 | 无 `clap` / 无 `windows-sys` |
| `src/` 残留 | 无 `crate::cli` / `cli::` / `startup::` / `utils::tty` / `bundle/` |
| 顶层目录 | `assets docs locales scripts src tests`（`bundle/` 消失） |
| `tests/fixtures/web` | 7 个样例文件在位 |
| `assets/rules` | 6 个规则文件在位 |
| `docs/screenshots` | 4 png 在位 |
| GUI 冒烟 | 开窗 + 5 页可用 + 书源可列 |
| git 状态 | **未提交** —— 等用户指示 |
