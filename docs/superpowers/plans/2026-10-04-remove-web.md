# 移除 Web 端实现 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把仓库从 GUI / Web / CLI 三形态收敛为 **GUI + CLI** 两形态，不留下孤儿代码、失效文档、断链注释或已无消费者的构建配置。

**Architecture:** 这是一次**纯删除 + 收敛**重构，没有新功能。改动分四层：编译面（feature 系统 / 依赖 / cfg 门控）、共享层孤儿（`core::library` / `core::sources` 中仅 web 消费的 helper）、i18n（`WebErrors` 词条与常量）、外围链路（前端 monorepo / Docker / 文档）。

**Tech Stack:** Rust 2024（单一 lib crate，非 workspace）、`cargo` 质量门（fmt / clippy `-D warnings` / test）、`rust-i18n`（编译期嵌入 `locales/app.yml`）。

## Global Constraints

以下约束适用于**每一个** Task，逐字抄自设计文档 `docs/superpowers/specs/2026-10-04-remove-web-design.md` 与项目规则：

- **不提交**：本项目 `tasks/lessons.md` L1 规定「只有用户明确说"提交"或"commit"时才执行 git commit」。**本计划的任何步骤都不含 `git commit`**。每个 Task 完成后停下，等待用户确认；全部完成后由用户决定是否提交。
- **不打 tag、不建分支**：可恢复性完全依赖 git history，不创建 `web-archive` 之类的存档引用。
- **`bundle/web/` 必须保留**（7 个 git 跟踪文件：章节页 HTML / 封面 / JS 样例）—— 书源解析的真实参考，零代码引用。删除时**不要**碰它。
- **`src/models/*.rs` 只改注释措辞，不重命名字段**：字段已被桌面端与磁盘上的既有 JSON 消费，改名是纯 churn。
- **保留 `tokio`**：crawler / parser / CLI / 桌面 model 共 20+ 文件在用，与 web 无关。
- **保留 `.github/workflows/release.yml`、`scripts/package-linux.sh`、`scripts/install-hooks.sh`**：与 web 无关，不需要任何改动。
- **测试基线（2026-10-04 实测）**：`cargo test --lib` = **510 passed / 0 failed / 4 ignored**。
  最终目标 = **487 passed / 4 ignored**（减少 23 个：i18n 2 + `core::sources` 5 + `core::library` 11 + `core::config_helpers` 5）。
- 所有命令在仓库根 `C:\Users\pc\Documents\GitHub\so-novel-rs` 执行。

### 与设计文档的五处差异（已核实，计划按此执行）

| # | 设计文档写的 | 实际情况 | 处理 |
|---|---|---|---|
| 1 | `cargo test --lib` 由 370 降至 368 | 实测基线是 **510** / 4 ignored，不是 370 | 目标改为 510 → 487 |
| 2 | 只删 `core::library` / `core::sources` 的函数 | 这些函数在**同文件内有单元测试**（`find_rule_by_id_cloned_returns_owned`、`list_skips_subdirectories` 等 16 个） | 函数与其测试同一 Task 一起删，`empty_inputs_are_safe` 需改一行 |
| 3 | 未提及 `i18n::ts_for_locale` | 删 Web 后它只剩测试在调（`src/web/handlers/*` 是原消费者） | **保留**（`pub` API，测试仍覆盖，符合"最小影响"）；只重写其文档注释，去掉 web 特有理由 |
| 4 | `web-ui/**` 86 文件 | `git ls-files web-ui` 实测 **80** 个跟踪文件（另加未跟踪的 `node_modules/` / `dist/`） | 按 80 跟踪 + 清理构建残留执行 |
| 5 | 未提及 `core::config_helpers::validate_download_path` | 实测**零调用方**（web 的 settings handler 当时就没调它，自己内联了校验）；其错误短码 `download_path_empty` / `download_path_not_dir` 正是已删的 `WebErrors` key | 执行中发现，**一并删除**（+ 5 个测试）；终态 492 → 487 |

> **关于差异 4 的补充**：`web-ui/` 在磁盘上还留着未跟踪的 `node_modules/` 与 `dist/`，`git rm` 不会删除它们，Task 5 Step 2 单独处理 —— 那里有不可逆的 `rm -rf`，执行前必须按步骤先 `ls` 确认。

### 关于 TDD

本次是删除型重构，没有"先写失败测试"的自然循环（被删代码的测试就是删掉的测试）。替代的验证纪律是：

1. **改动前记录基线数字**（见 Global Constraints）。
2. **每个 Task 有唯一的机器可判定的验收命令 + 期望输出**。
3. **对"不该再存在的字符串"用 grep 做缺席断言**（grep 应无输出）。
4. 按 Task 顺序执行 —— 每个 Task 结束时仓库都必须能 `cargo check` / `cargo test --lib` 通过。

---

## File Structure

**删除（git 跟踪文件共 101 个）**

| 路径 | 数量 |
|---|---|
| `src/web/**` | 15 |
| `src/startup/web.rs` | 1 |
| `web-ui/**` | 80 |
| `Dockerfile` / `docker-compose.yml` / `.dockerignore` | 3 |
| `.github/workflows/docker-release.yml` | 1 |
| `docs/WEB.md` | 1 |

**修改**

| 路径 | 责任 |
|---|---|
| `Cargo.toml` | 删 `[features]` 段、web 依赖、`tower` dev-dep；`gpui-kit` / `rfd` 转必选 |
| `build.rs` | 删前端构建分支，只留 Windows 图标 |
| `src/lib.rs` / `src/main.rs` | 删 cfg 门控 |
| `src/startup/mod.rs` | `LaunchMode` 收敛为 2 变体，`detect` / `dispatch` 重写 |
| `src/desktop/model/events.rs` | 删 5 处 `#[cfg(feature = "gui")]` |
| `src/core/library.rs` | 删 5 个 web-only 项 + 11 个测试 |
| `src/core/sources.rs` | 删 2 个 web-only 项 + 5 个测试 |
| `src/i18n.rs` | `TStr` 单实现；删 `WEB_ERROR_KEYS` + 2 测试；3 处注释 |
| `locales/app.yml` | 删 `WebErrors:` 段；改 `Cli.about_short` / `about_long` |
| `src/cli/args.rs` | `ABOUT_SHORT` / `ABOUT_LONG` 文案去 WEB |
| 其余 11 个 `.rs` | 注释 / rustdoc 措辞（Task 4） |
| `README.md` / `docs/CLI.md` / `docs/BOOK_SOURCES.md` / `docs/CHANGELOG.md` | 文档同步 |

**保留不动（明确）**：`bundle/web/`、`bundle/rules/`、`screenshots/`、`assets/`、`src/cli/**`、`src/desktop/**`、`src/crawler/**`、`src/parser/**`、`src/export/**`、`src/db/**`、`docs/CHANGELOG_ALL.md`、`docs/superpowers/specs/2026-09-08-web-shadcn-redesign-design.md`（历史设计记录）。

---

## Task 1: 切断 web 编译面（原子切换）

删除 `src/web/**` 与 feature 系统必须**在同一步完成** —— 只删一半会立刻破坏编译，所以这是一个不可再分的 Task。

**Files:**
- Delete: `src/web/`（整目录，15 文件）、`src/startup/web.rs`
- Modify: `Cargo.toml`、`build.rs`、`src/lib.rs`、`src/main.rs`、`src/startup/mod.rs`、`src/desktop/model/events.rs`、`src/i18n.rs`

**Interfaces:**
- Consumes: 无（首个 Task）
- Produces: 一个不再有 `web` / `gui` feature、不再有 `crate::web` 模块的 crate。后续 Task 依赖的事实：`LaunchMode` 只有 `Cli` / `Gui` 两个变体；`crate::i18n::TStr` 恒为 `gpui_kit::SharedString`。

- [x] **Step 1: 记录基线**

Run:
```sh
cargo test --lib 2>&1 | tail -3
```
Expected: `test result: ok. 510 passed; 0 failed; 4 ignored; ...`

- [x] **Step 2: 删除 web 服务模块与启动包装**

Run:
```sh
git rm -r src/web
git rm src/startup/web.rs
```
Expected: `rm 'src/web/...'` 共 15 行 + `rm 'src/startup/web.rs'`。

- [x] **Step 3: 改写 `Cargo.toml`**

删除整个 `[features]` 段（当前 L14-31，含 `default = ["gui"]` 与其上方全部注释），即从 `[features]` 到 `]`（`web` 数组收尾）整块。

`[dependencies]` 内两处改为必选（去掉 `, optional = true`）：
```toml
gpui-kit = "0.7.0"
```
```toml
rfd = { version = "^0.17.2", default-features = false, features = ["xdg-portal"] }
```

删除以下 7 个依赖及其上方注释（当前 L110-120 整块）：
```toml
# Web 前端资源嵌入（web feature 专属）。
# `include-exclude` 按 #[folder] 的 .gitignore 规则过滤（跳过 .map / node_modules / src 等）。
rust-embed = { version = "8", features = ["include-exclude"], optional = true }
# MIME 推导，给 rust-embed 的静态文件设置 Content-Type。
mime_guess = { version = "2", optional = true }
# Web 服务：axum + tower-http CORS。（web feature 专属）
axum = { version = "0.8", features = ["macros"], optional = true }
axum_session = { version = "0.20", optional = true }
async-stream = { version = "0.3", optional = true }
futures = { version = "0.3", optional = true }
tower-http = { version = "0.7", features = ["cors"], optional = true }
```

`[dev-dependencies]` 删除 `tower` 及其上方注释：
```toml
# axum Router 在测试里走 tower::ServiceExt::oneshot。
tower = { version = "0.5", features = ["util"] }
```

**保留** `tokio`、`[target.'cfg(target_os = "windows")'.dependencies]` 与 `build-dependencies`。

- [x] **Step 4: 改写 `build.rs`**

整文件替换为：

```rust
//! Build script: Windows icon resource.

fn main() {
    // ── Windows icon resource ────────────────────────────────────────────
    println!("cargo:rerun-if-changed=assets/logo.ico");

    #[cfg(target_os = "windows")]
    {
        let ico = std::path::Path::new("assets").join("logo.ico");
        if ico.exists() {
            let mut res = winres::WindowsResource::new();
            // 局部 `#[allow]` 而非 crate-level 抑制，避免误伤业务代码。
            #[allow(clippy::expect_used)]
            let icon_str = ico.to_str().expect("ico path is valid utf-8");
            res.set_icon(icon_str);
            if let Err(e) = res.compile() {
                println!("cargo:warning=embed icon failed: {e}");
            }
        } else {
            println!("cargo:warning=assets/logo.ico not found, skip exe icon embed");
        }
    }
}
```

（删掉了 `use std::process::Command;`、`FRONTEND_DIST`、5 行 `rerun-if-changed`、`CARGO_FEATURE_WEB` 分支、`run_bun_build()`。）

- [x] **Step 5: `src/lib.rs` 去掉两处 cfg 门控**

把
```rust
#[cfg(feature = "gui")]
pub mod desktop;
```
改为
```rust
pub mod desktop;
```

删除整行
```rust
#[cfg(feature = "web")]
```
（紧邻其下的 `pub mod web;` 已随 `git rm` 一并消失 —— 若该行仍在，说明 `git rm` 未生效，回 Step 2 重做）。

- [x] **Step 6: `src/main.rs` 去掉 `feature = "gui"` 条件并更新注释**

整文件替换为：

```rust
// Windows release 下走 GUI subsystem，避免 GUI 启动时弹出控制台黑窗。
// CLI 模式通过 `startup::attach_parent_console` 挂载到父进程控制台
// （详见 `startup::dispatch`）。
//
// 进程入口的所有职责（mode 判定 / console attach / tracing init / dispatch）
// 都委托给 `so_novel_rs::startup`，本文件只负责 argv 收集。
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    so_novel_rs::startup::dispatch(so_novel_rs::startup::detect(&args))
}
```

- [x] **Step 7: 重写 `src/startup/mod.rs`**

整文件替换为（`attach_parent_console` 两个平台版本原样保留）：

```rust
//! 进程启动层：从 argv 判定走 GUI / CLI 模式，各自转交给 `desktop::run` / `cli::run`。
//!
//! 两个顺序约束，错则行为可见地坏：
//! - CLI 必须先 `attach_parent_console()` 再分发：release 是 GUI subsystem exe，不 attach 则
//!   stdio 关到 NUL，用户看不到任何输出。
//! - GUI **不**能 attach：Explorer 双击时 `AllocConsole` fallback 会弹黑窗。

use anyhow::Result;

/// 两种启动模式。
///
/// 无字段的纯判别式 enum，`Copy` 是语义正确的选择（也让 `dispatch` 直接收值）。
#[derive(Debug, Clone, Copy)]
pub enum LaunchMode {
    /// CLI 子命令模式（`so-novel-rs search ...` / `download ...` / `sources ...`）。
    Cli,
    /// GPUI 桌面客户端模式（无任何参数时）。
    Gui,
}

/// 从 argv 判定启动模式：除 binary name 外还有 arg → `Cli`，否则 `Gui`。
pub const fn detect(args: &[String]) -> LaunchMode {
    if args.len() > 1 {
        return LaunchMode::Cli;
    }
    LaunchMode::Gui
}

/// 把当前进程附加到父进程控制台（仅 Windows）；`AttachConsole` 失败时
/// （双击 / GUI shell 启动，父进程无控制台）回退 `AllocConsole()`，确保
/// CLI 仍有 stdio。debug build 本身是 console subsystem，静默成功。
#[cfg(target_os = "windows")]
#[allow(unsafe_code)] // SAFETY: Windows 控制台附着是 OS 层 FFI, 唯一可行的接入点
pub fn attach_parent_console() {
    unsafe {
        use windows_sys::Win32::System::Console::{
            ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole,
        };
        // SAFETY: `AttachConsole` / `AllocConsole` 是 Win32 控制台管理 API,
        // 接受简单整数参数 (`ATTACH_PARENT_PROCESS` = `u32::MAX`), 无指针/句柄, 不会越界。
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            AllocConsole();
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn attach_parent_console() {}

/// 调度器：按 `LaunchMode` 分发到对应模式。
///
/// CLI 先 `attach_parent_console()` 再分发。CLI **不**调 `logger::init`（由 `cli::run`
/// 自己在 `--verbose` 时决定）；全局 `tracing_subscriber::registry().init()` 二次调用会 panic。
pub fn dispatch(mode: LaunchMode) -> Result<()> {
    match mode {
        LaunchMode::Cli => {
            attach_parent_console();
            crate::cli::run()
        }
        LaunchMode::Gui => {
            crate::logger::init();
            crate::desktop::run()
        }
    }
}
```

（`pub fn run_gui()` 一并删除 —— 它的一行转发体只为 `not(feature = "gui")` 的 bail 分支而存在，feature 没了就该内联进 `dispatch`。）

> **`Clone, Copy` + `const fn` 是执行时必须补的：** 旧 `LaunchMode` 的 `Web { host: String, port: u16 }` 变体带了堆数据，掩盖了两条 clippy lint；收敛成两个无字段变体后暴露：
> `needless_pass_by_value`（`dispatch(mode: LaunchMode)` 未被消费）与 `missing_const_for_fn`（`detect`）。
> 修法是 clippy 自己给的 `help: or consider marking this type as Copy` 与 `this could be a const fn` —— 无字段判别式 enum 本就该 `Copy`，`detect` 只调 `slice::len` 本就该 `const`。**不**改 `dispatch` 签名为 `&LaunchMode`（那会为一个纯判别式 enum 平白引入借用）。

- [x] **Step 8: `src/desktop/model/events.rs` 去掉 5 处 cfg**

删 L10 整行注释：
```rust
// `drain` 函数体要用这两条, 跟 `drain` 一起 gate, 否则 web-only 构建触发 unused_imports。
```

删 L11 与 L13 两行 `#[cfg(feature = "gui")]`，保留其下的 `use super::AppModel;` / `use super::UpdateOutcome;`。

删 L58 的 `#[cfg(feature = "gui")]`（`pub fn drain` 上方）。

删 L165 整行注释与其下 L166 的 cfg：
```rust
// 跟 `drain` 一起 gate, 否则 web-only 构建会触发 unused import warning。
#[cfg(feature = "gui")]
```

L169 改为：
```rust
#[cfg(test)]
mod tests {
```

- [x] **Step 9: `src/i18n.rs` 合并 `TStr` 双分支**

把
```rust
/// 翻译返回类型别名：gui feature 下为 `gpui_kit::SharedString`（`Arc<str>` 语义，clone 零 alloc），
/// 非 gui 构建（如 web-only Docker）为 `String`。两种构建下调用方都可直接 `.into()`。
#[cfg(feature = "gui")]
pub type TStr = gpui_kit::SharedString;
#[cfg(not(feature = "gui"))]
pub type TStr = String;
```
改为
```rust
/// 翻译返回类型别名：`gpui_kit::SharedString`（`Arc<str>` 语义，clone 零 alloc）。
pub type TStr = gpui_kit::SharedString;
```

- [x] **Step 10: 验收 —— 编译 + feature 缺席断言**

Run:
```sh
cargo check --all-targets 2>&1 | tail -20
```
Expected: `Finished` 且 **0 warning / 0 error**。

Run:
```sh
grep -rn 'feature = "web"\|feature = "gui"\|CARGO_FEATURE_WEB\|SO_NOVEL_WEB' src/ build.rs Cargo.toml || echo "CLEAN"
```
Expected: 只打印 `CLEAN`（`src/`、`build.rs`、`Cargo.toml` 内 0 命中）。

Run:
```sh
cargo test --lib 2>&1 | tail -3
```
Expected: `test result: ok. 510 passed; 0 failed; 4 ignored; ...`（本 Task 不动任何测试；i18n 的 web 词条测试此时仍通过，因为 `locales/app.yml` 还没改）。

Run:
```sh
cargo tree -e normal --depth 1 2>/dev/null | grep -iE 'axum|rust-embed|mime_guess|tower-http|async-stream' || echo "CLEAN"
```
Expected: `CLEAN`。

> `--depth 1` 是必须的：`rust-embed` 由 `gpui-kit-assets` 传递引入、`tower-http` 由 `reqwest`
> 传递引入，两者都不是我们的直接依赖 —— 去掉 `--depth 1` 会误报（执行时实测确认）。
> 要证明"我们自己的依赖表干净"，看直接依赖层即可。

---

## Task 2: 清理共享层孤儿（`core::library` / `core::sources` / `core::config_helpers`）

**Files:**
- Modify: `src/core/library.rs`（整文件重写）
- Modify: `src/core/sources.rs`
- Modify: `src/core/config_helpers.rs`
- Test: 同上三个文件内的 `mod tests`

**Interfaces:**
- Consumes: Task 1 产生的无-feature crate。
- Produces: `core::library` 仍导出 `SUPPORTED_LIBRARY_EXTS: &[&str]`、`LibraryEntry { filename, ext, modified_unix, size_bytes }`、`LibraryEntry::from_path(&Path) -> Option<Self>`。`core::sources` 仍导出 `rule_key` / `disabled_url_key` / `find_rule_by_id` / `parse_rules_bytes` / `load_active` / `match_source_by_url`。

> **已核实**：全仓 grep 确认 `extension_to_content_type` / `list_library_entries` / `safe_file_path` / `open_download_file` / `OpenFileError` / `find_rule_by_id_cloned` / `find_rule_by_url` 的调用方**只在 `src/web/` 内**（已随 Task 1 删除）或本文件测试内。删除安全。

- [x] **Step 1: 整文件重写 `src/core/library.rs`**

```rust
//! 下载文件元数据 + 扩展名常量（GUI / CLI 共享）。
//!
//! 桌面 `model/library_state.rs::scan_library_dir` 与 CLI 导出后的"扫下载目录 / 列条目 /
//! 算 ext"共用这里的常量，各自维护一份白名单字面量容易漏改一边。

use std::path::Path;
use std::time::SystemTime;

use serde::Serialize;

/// GUI + CLI 共用的下载文件扩展名白名单（**单一事实来源**）。
/// 桌面 `scan_library_dir` 引用这里，新增 / 删除格式只改这一处。
pub const SUPPORTED_LIBRARY_EXTS: &[&str] = &["epub", "txt", "html", "zip", "pdf", "md"];

/// 单条 library 条目。
#[derive(Debug, Clone, Serialize)]
pub struct LibraryEntry {
    pub filename: String,
    pub ext: String,
    pub modified_unix: i64,
    pub size_bytes: u64,
}

impl LibraryEntry {
    /// 从一条候选文件路径构造 entry；过滤一步到位，调用方拿到的 Vec 已经是"该展示的"。
    ///
    /// 返回 `None`：路径不是 regular file、扩展名不在 [`SUPPORTED_LIBRARY_EXTS`] 白名单（或解析不出）、
    /// 元数据读不出（permission / IO 错误）。
    pub fn from_path(path: &Path) -> Option<Self> {
        if !path.is_file() {
            return None;
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)?;
        if !SUPPORTED_LIBRARY_EXTS.contains(&ext.as_str()) {
            return None;
        }
        let filename = path.file_name().and_then(|s| s.to_str())?.to_string();
        let meta = path.metadata().ok()?;
        let size_bytes = meta.len();
        let modified_unix = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs().cast_signed());
        Some(Self {
            filename,
            ext,
            modified_unix,
            size_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn supported_exts_contains_expected() {
        for ext in ["epub", "txt", "html", "zip", "pdf", "md"] {
            assert!(
                SUPPORTED_LIBRARY_EXTS.contains(&ext),
                "{ext} should be in SUPPORTED_LIBRARY_EXTS"
            );
        }
    }

    #[test]
    fn supported_exts_length_is_six() {
        // 锁死长度 —— 防新加 / 删除扩展名时漏改下游消费方的分页 / 计数逻辑。
        assert_eq!(SUPPORTED_LIBRARY_EXTS.len(), 6);
    }

    fn touch(path: &Path, content: &[u8]) {
        std::fs::write(path, content).expect("write");
    }

    fn temp_dir(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "sonovel-core-library-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).expect("mkdir");
        p
    }

    #[test]
    fn from_path_returns_none_for_directory() {
        let dir = temp_dir("dir");
        assert!(LibraryEntry::from_path(&dir).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_returns_none_for_unsupported_ext() {
        let dir = temp_dir("unsupported");
        let f = dir.join("book.docx");
        touch(&f, b"x");
        assert!(LibraryEntry::from_path(&f).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_returns_none_for_no_extension() {
        let dir = temp_dir("noext");
        let f = dir.join("README");
        touch(&f, b"x");
        assert!(LibraryEntry::from_path(&f).is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_returns_entry_for_supported_file() {
        let dir = temp_dir("ok");
        let f = dir.join("book.epub");
        touch(&f, b"epub-body");
        let entry = LibraryEntry::from_path(&f).expect("entry");
        assert_eq!(entry.filename, "book.epub");
        assert_eq!(entry.ext, "epub");
        assert_eq!(entry.size_bytes, b"epub-body".len() as u64);
        assert!(entry.modified_unix > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_normalizes_extension_case() {
        let dir = temp_dir("case");
        let f = dir.join("book.EPUB");
        touch(&f, b"x");
        let entry = LibraryEntry::from_path(&f).expect("entry");
        assert_eq!(entry.ext, "epub", "ext must be lowercase");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_path_returns_none_for_missing_file() {
        let p = std::path::PathBuf::from("/nonexistent/path/never-exists.epub");
        assert!(LibraryEntry::from_path(&p).is_none());
    }
}
```

本步删掉的项：`extension_to_content_type`、`list_library_entries`、`OpenFileError`（含 `Display` impl）、`safe_file_path`、`open_download_file`，以及测试 `content_type_known_extensions` / `content_type_case_insensitive` / `content_type_unknown_returns_none` / `list_returns_empty_for_missing_dir` / `list_skips_unsupported_extensions` / `list_sorts_by_mtime_descending` / `list_skips_subdirectories` / `safe_file_path_returns_path_for_existing_file` / `safe_file_path_rejects_traversal` / `safe_file_path_not_found_for_missing` / `open_file_error_display`（共 11 个）。

- [x] **Step 2: `src/core/sources.rs` — 改模块头**

把第 1 行与第 3-4 行改为（第 3 行开头补 `//!` 缩进风格保持原样）：

```rust
//! CLI / desktop 共用的书源（Rule）查找 + 解析 + URL 键规范化。
//!
//! 原先 desktop / cli / db 多处各自写同一套 `iter().find(|r| r.id == id)` 或 `r.url.trim().to_lowercase()`
//! 的重复；抽到这里后调用方只用 `find_rule_by_id` / `rule_key` / `disabled_url_key`。
```

（第 4 行原文含 `find_rule_by_url`，必须去掉。）

- [x] **Step 3: `src/core/sources.rs` — 删两个函数**

删除 L34-37（含其上方 L34 的 doc 行）：
```rust
/// 在规则列表里按 ID 找（返回 owned `Rule`，用于跨锁边界 / `Send`）。
pub fn find_rule_by_id_cloned(rules: &[Rule], id: i32) -> Option<Rule> {
    rules.iter().find(|r| r.id == id).cloned()
}
```

删除 L39-44 整块：
```rust
/// 在规则列表里按 URL 键（`disabled_url_key` 归一）找；找不到返回 `None`。
/// 规则 URL 的前后空白 / 大小写不一致不影响匹配。
pub fn find_rule_by_url<'a>(rules: &'a [Rule], url: &str) -> Option<&'a Rule> {
    let key = disabled_url_key(url);
    rules.iter().find(|r| rule_key(r) == key)
}
```

保留 `rule_key`、`disabled_url_key`、`find_rule_by_id`、`parse_rules_bytes`、`load_active`、`match_source_by_url`。

- [x] **Step 4: `src/core/sources.rs` — 删对应测试并修一处断言**

删除这 5 个测试函数（连同其上 doc/空行）：
`find_rule_by_id_cloned_returns_owned`、`find_rule_by_url_matches_case_insensitively`、`find_rule_by_url_matches_with_surrounding_whitespace`、`find_rule_by_url_returns_none_when_missing`、`find_rule_by_url_does_not_falsely_match_disabled`。

修改 `empty_inputs_are_safe`，删掉引用已删函数的那一行：
```rust
    #[test]
    fn empty_inputs_are_safe() {
        assert_eq!(rule_key(&Rule::default()), "");
        assert_eq!(disabled_url_key(""), "");
        assert!(find_rule_by_id(&[], 0).is_none());
        assert!(match_source_by_url(&[], "https://x").is_none());
        assert!(parse_rules_bytes(b"", Path::new("empty.json")).is_err());
    }
```

- [x] **Step 5: `src/core/config_helpers.rs` — 删零调用方的 `validate_download_path` + 5 个测试**

> 执行中发现（计划初稿漏了）：`validate_download_path` 全仓**零调用方**。web 的 settings handler 当时也没调它 —— 自己内联了校验。它的错误短码 `download_path_empty` / `download_path_not_dir` 正是已删的 `WebErrors` key，属 web 残留。

删除函数本体（含其 doc）：
```rust
/// 校验 `download_path`：非空 + 路径存在 + 是目录。
///
/// 返回的错误字符串是稳定契约，调用方靠它映射错误码：空 → `"download_path_empty"`、
/// 不存在 → `"download_path_not_found"`、是文件不是目录 → `"download_path_not_dir"`。
/// 返回 `Result<(), String>` 而非 anyhow：web handler 需要稳定短码做 i18n 键，anyhow 的 `{e:#}` 会泄露内部路径。
pub fn validate_download_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("download_path_empty".to_string());
    }
    let p = std::path::Path::new(trimmed);
    if !p.exists() {
        return Err("download_path_not_found".to_string());
    }
    if !p.is_dir() {
        return Err("download_path_not_dir".to_string());
    }
    Ok(())
}
```

删除 `mod tests` 里的 `── validate_download_path ──` 分节共 5 个测试：
`validate_download_path_empty_string_rejected`、`validate_download_path_whitespace_only_rejected`、`validate_download_path_nonexistent_rejected`、`validate_download_path_existing_file_rejected_as_not_dir`、`validate_download_path_existing_dir_accepted`。

模块头同步改为：
```rust
//! `AppConfig` 的"空字符串视作 None"helper。
//!
//! CLI / desktop 读 `cf_bypass` / `qidian_cookie` 时统一走"trim 后空 → None"
//! 语义，集中在这里，避免每个调用方各写一份判断。
```

保留 `cf_bypass`（`core::search` 再导出）与 `qidian_cookie`（`desktop/model/ops/search.rs:199` 在用）及其 6 个测试。

- [x] **Step 6: 验收 —— 测试数降到 489**

Run:
```sh
cargo test --lib 2>&1 | tail -3
```
Expected: `test result: ok. 489 passed; 0 failed; 4 ignored; ...`（510 − 21：`core::library` 11 + `core::sources` 5 + `core::config_helpers` 5）

Run:
```sh
cargo clippy --all-targets -- -D warnings 2>&1 | tail -10
```
Expected: `Finished`，0 warning。

---

## Task 3: 删除 `WebErrors` i18n 段

**Files:**
- Modify: `locales/app.yml`（删 L1478-1687）
- Modify: `src/i18n.rs`（删 `WEB_ERROR_KEYS` + 2 个测试）

**Interfaces:**
- Consumes: Task 1/2 的产物。
- Produces: `locales/app.yml` 最后一个顶层段变为 `Cli:`；`src/i18n.rs` 的测试模块只剩 `ts_and_ts_fmt_work` / `ts_for_locale_*` / `locale_for_matches_app_yml_locale_tags` / `url_download_*`。保留 `ts_for_locale` 函数本身（`pub` API，其 4 个测试继续跑）。

- [x] **Step 1: 删 `src/i18n.rs` 的 `WEB_ERROR_KEYS` 常量与两个测试**

删除从 L209 的文档行
```rust
    /// `WebErrors` 全部 key：与 `src/web/error_code.rs::ErrorCode` 1:1 + handler 散落字符串。
```
起，到 L264 的
```rust
    ];
```
为止的整块常量定义，以及紧随其后的两个测试函数 `web_errors_translated_in_all_three_locales`（L266-275）与 `web_errors_en_zh_cn_zh_tw_differ`（L277-294）。

保留其下的 `URL_DOWNLOAD_KEYS` 常量与 `url_download_translated_in_all_three_locales` 测试。

- [x] **Step 2: 验收 —— 测试数降到 487（此时 yml 还没删，测试已不引用它）**

Run:
```sh
cargo test --lib 2>&1 | tail -3
```
Expected: `test result: ok. 487 passed; 0 failed; 4 ignored; ...`

- [x] **Step 3: 删 `locales/app.yml` 的 `WebErrors:` 段**

删除 L1478（属于 `Cli:` 段末尾之后的空行）到文件末尾 L1687 的全部内容。删除块开头是：
```yaml

# Web API 错误消息（per-request locale）。
#
# 与 `src/web/error_code.rs::ErrorCode` 1:1 对应（38 原有 + 3 新增 3004/3005/3006）
# + handler 中散落的内联字符串（6 个 success body / 资源标识 / HTTP status）。
# 长度 ≤ 30 字（避免泄漏内部细节），由 `WebError::IntoResponse` 序列化进
# `{ error: { code, message } }` body。3 种 locale 同步维护。
WebErrors:
```
结尾是最后一条：
```yaml
  source_test_http_status:
    en: ...
    zh-CN: ...
    zh-TW: ...
```

删除后文件应以 `Cli:` 段的最后一条 key 收尾（`sources_disable_id_help` 的三个 locale 行），总行数从 1687 变为 1477。

- [x] **Step 4: 验收 —— YAML 仍可被 `rust-i18n` 解析、测试数不变**

Run:
```sh
cargo test --lib 2>&1 | tail -3
```
Expected: `test result: ok. 487 passed; 0 failed; 4 ignored; ...`

Run:
```sh
grep -rn 'WebErrors' src/ locales/ || echo "CLEAN"
```
Expected: `CLEAN`。

Run:
```sh
tail -4 locales/app.yml
```
Expected: 输出 `sources_disable_id_help` 的 `en` / `zh-CN` / `zh-TW` 三行（无 `WebErrors`、无空尾行堆叠）。

---

## Task 4: 清理 Web 措辞注释与 rustdoc 断链

**Files:**
- Modify: `src/crawler/resolve.rs`、`src/error.rs`、`src/core/mod.rs`、`src/core/async_progress.rs`、`src/core/bootstrap.rs`、`src/core/search.rs`、`src/core/update.rs`、`src/db/tasks.rs`、`src/logger.rs`、`src/cli/mod.rs`、`src/cli/args.rs`、`src/models/book.rs`、`src/models/chapter.rs`、`src/models/mod.rs`、`src/config/toml_io.rs`、`src/i18n.rs`、`src/utils/mod.rs`、`src/utils/lock.rs`、`locales/app.yml`

**Interfaces:**
- Consumes: Task 1-3。
- Produces: `cargo clippy --all-targets -- -D warnings` 与 `cargo doc` 均 0 警告（`rustdoc::broken_intra_doc_links` 是 warn 级，`-D warnings` 会把它升级为错误）。

> 这些改动**全部是注释 / 文档字符串**，不改任何行为。字段名一律不动（Global Constraints）。

- [x] **Step 1: 修 rustdoc 断链（唯一的编译级问题）**

`src/crawler/resolve.rs:22`：
```rust
/// 边界层 ([`crate::web::error::WebError`]) 收口映射。
```
改为
```rust
/// 边界层 (CLI 顶层 / 桌面 UI) 各自按需映射到自己的展示错误。
```

`src/config/toml_io.rs:197`：
```rust
/// 设置页保存路径见 `web::handlers::settings` / `desktop::model::ops::settings`。
```
改为
```rust
/// 设置页保存路径见 `desktop::model::ops::settings`。
```

- [x] **Step 2: `src/error.rs` 模块头去掉 `WebError`**

L3：
```rust
//! 设计：各业务域错误 (`ExportError` / `WebError` / `SearchError` / ...) 保留在自己模块里，
```
改为
```rust
//! 设计：各业务域错误 (`ExportError` / `SearchError` / ...) 保留在自己模块里，
```

L5-6：
```rust
//! 自动归一，调用方一个 `?` 即可透传；边界层 (`main.rs` / CLI 顶层 / HTTP handler)
//! 仍可保留或转回自己的边界错误类型 (e.g. `WebError` 的 `IntoResponse`)。
```
改为
```rust
//! 自动归一，调用方一个 `?` 即可透传；边界层 (`main.rs` / CLI 顶层)
//! 仍可保留或转回自己的边界错误类型。
```

- [x] **Step 3: `src/core/mod.rs` 模块头「三端」→「两端」**

L1-8 整块替换为：
```rust
//! 共享逻辑（CLI / desktop 都可能用到的核心类型与工具）。
//!
//! 只放两端都用到的代码：当前唯一居民 [`DownloadTask`]（下载任务的运行时表达，
//! 含后台进度接收端 + 取消令牌，两端直接构造 / 消费同一实例）。
//!
//! 不放这里：GUI 专属的 `AppModel` / `UIEvent` / `list_cache`（留 `desktop/model/`）、
//! 仅某端使用的持久化 record、通用引擎（留 `crawler` / `parser` / `db` 等）。
//! 原则：同一概念在两个前端各写一份时，抽到这里作为共享契约层。
```

- [x] **Step 4: `src/core/search.rs` 与 `src/core/update.rs`**

`src/core/search.rs` L1：
```rust
//! 三端共用的"搜索前准备"逻辑：选书源 / 算 `cf_bypass` / 算 limit。
```
改为
```rust
//! CLI / desktop 共用的"搜索前准备"逻辑：选书源 / 算 `cf_bypass` / 算 limit。
```

`src/core/search.rs` L3：
```rust
//! 之前 cli / web / desktop 三处近乎字面量重复同一套实现，抽出来后调用方收敛为
```
改为
```rust
//! 之前 cli / desktop 两处近乎字面量重复同一套实现，抽出来后调用方收敛为
```

`src/core/search.rs` L6 中的 `（web / desktop 直接传全量，` 改为 `（desktop 直接传全量，`。

`src/core/search.rs` L44：
```rust
/// web query param 的校验属于 HTTP 层，仍在 web handler 里做。
```
改为
```rust
/// query param 的校验属于各前端自己的输入层，不在此处做。
```

`src/core/update.rs` L1：
```rust
//! 三端共享的版本更新检查。
```
改为
```rust
//! CLI / desktop 共享的版本更新检查。
```

`src/core/update.rs` L4 中的 `；当前只有 desktop 用，web 想做 `GET /api/update`、CLI 想做 `sonovel update`` 改为 `；当前只有 desktop 用，CLI 想做 `sonovel update``（保留该行原有的续行结构，只删 web 那半句）。

- [x] **Step 5: 其余「三端」/「Web 路径」措辞**

`src/db/tasks.rs:31`：
```rust
/// [`DownloadTaskRecord`] → 写盘; cli / web / desktop 三端共用, 调用方不再手动 `.map(to_record)`。
```
改为
```rust
/// [`DownloadTaskRecord`] → 写盘; cli / desktop 共用, 调用方不再手动 `.map(to_record)`。
```

`src/logger.rs:7`：
```rust
//!   `cli::run`（`--verbose`）与 `startup::dispatch`（Web / Gui 路径）已分流、各自只调一次
```
改为
```rust
//!   `cli::run`（`--verbose`）与 `startup::dispatch`（Gui 路径）已分流、各自只调一次
```

`src/cli/mod.rs:117`：
```rust
    // 加载 config + 首次启动写默认 + 初始化规则目录 (三端 startup 兜底矩阵见 `core::bootstrap`)。
```
改为
```rust
    // 加载 config + 首次启动写默认 + 初始化规则目录 (startup 兜底矩阵见 `core::bootstrap`)。
```

> 以下 4 个文件计划初稿漏列，执行时由全仓 grep（含大小写不敏感）补齐。

`src/core/async_progress.rs:1`（实测调用方只有 `src/desktop/model/*`，故写「桌面 UI」而非「两端」）：
```rust
//! 三端共用的 "mpsc 接收端排空" helper。
```
改为
```rust
//! 桌面 UI 共用的 "mpsc 接收端排空" helper。
```

`src/core/bootstrap.rs:1`：
```rust
//! 三端共用的启动期公共资源加载 + 几样从 `cli/util.rs` 搬来的薄壳。
```
改为
```rust
//! CLI / desktop 共用的启动期公共资源加载 + 几样从 `cli/util.rs` 搬来的薄壳。
```

`src/core/bootstrap.rs:3-5`：
```rust
//! `AppContext::load_context` **不**返回 `Result`：三端容错策略不一致（desktop 各处
//! `tracing::warn!` + 兜底默认、web 走 `unwrap_or_default()`、cli 用 `anyhow::Result`
//! 但也不想在这里 panic），所以内部所有 IO 失败都吞掉 + warn，返回尽力凑齐的 `AppContext`。
```
改为
```rust
//! `AppContext::load_context` **不**返回 `Result`：两端容错策略不一致（desktop 各处
//! `tracing::warn!` + 兜底默认、cli 用 `anyhow::Result`
//! 但也不想在这里 panic），所以内部所有 IO 失败都吞掉 + warn，返回尽力凑齐的 `AppContext`。
```

`src/core/bootstrap.rs:8`：`放这里供 desktop / web 将来复用。` → `放这里供 desktop 将来复用。`

`src/utils/mod.rs:7` 中的 `（三端共享，抽进来会污染其他端）` 改为 `（两端共享，抽进来会污染其他端）`。

`src/utils/lock.rs:4`（这是 Task 7 grep 漏掉、但第 2 条大小写不敏感 grep 会命中的行）：
```rust
//! web handler 走 axum 专用的 `(StatusCode, String)` 形态（`src/web/handlers/lock.rs`）；
```
改为
```rust
//! 调用方按需映射到自己的边界错误形态；
```

- [x] **Step 6: `src/models/*.rs` 注释去 web-ui（**不动字段名**）**

`src/models/book.rs:3`：
```rust
//! DTO (Web API `/book` 端点); 字段名沿用 camelCase, 与 web-ui 前端对齐。
```
改为
```rust
//! 序列化格式 (JSON); 字段名沿用 camelCase, 与既有规则文件 / 任务记录保持一致。
```

`src/models/chapter.rs:1`：
```rust
//! 章节模型: 下载任务的核心数据单元, 兼 PO (落盘到 `chapters/`) 与 DTO (Web SSE / API 响应)。
```
改为
```rust
//! 章节模型: 下载任务的核心数据单元, 兼 PO (落盘到 `chapters/`) 与前后端传输格式。
```

`src/models/mod.rs:3-4`：
```rust
//! **不严格区分** DTO/PO/Param/Resp: 业务侧只有一种持久化格式 (JSON), 字段命名已由 web-ui 前端
//! 定型。实际做法: PO + DTO 同体 (`Book` / `Chapter` / `SearchResult`, 用 `#[serde(rename)]`
```
改为
```rust
//! **不严格区分** DTO/PO/Param/Resp: 业务侧只有一种持久化格式 (JSON), 字段命名为兼容既有
//! 规则文件与任务记录而定型。实际做法: PO + DTO 同体 (`Book` / `Chapter` / `SearchResult`, 用 `#[serde(rename)]`
```

- [x] **Step 7: `src/i18n.rs` 三处注释**

L17-22（`locale_for` 文档）：
```rust
/// **`Language → locale 字符串` 的唯一权威映射**，也是 web 前端
/// `web-ui/src/i18n/locales/{en,zh-CN,zh-TW}.json` 文件名的来源（前后端 locale tag 统一）。
/// `TraditionalChinese` → `"zh-TW"`，**不是** `gpui_kit::component` 用的 `"zh-HK"`。
///
/// CLI / web-only 构建不依赖 `desktop`，但也要按 `config.toml` 的 language 切帮助语言，
/// 所以本函数必须留在 cfg gate 之外的 crate root。
```
改为
```rust
/// **`Language → locale 字符串` 的唯一权威映射**。
/// `TraditionalChinese` → `"zh-TW"`，**不是** `gpui_kit::component` 用的 `"zh-HK"`。
///
/// CLI 不依赖 `desktop`，但也要按 `config.toml` 的 language 切帮助语言，
/// 所以本函数留在 crate root。
```

L33-35（`locale_for_gpui` 文档末行）：
```rust
/// web 路径走 [`locale_for`]，桌面路径走本函数（调用点只有 `src/desktop/mod.rs::run` 一行）。
```
改为
```rust
/// CLI 路径走 [`locale_for`]，桌面路径走本函数（调用点只有 `src/desktop/mod.rs::run` 一行）。
```

L106-110（`ts_for_locale` 文档，函数体**不动**）：
```rust
/// 翻译查找的 per-request 变体 —— 显式传 locale，**不**读 / 不写全局 `rust_i18n::locale()`。
///
/// Web handler 入口拿到 `Locale` extractor 后，闭包里所有翻译都走这里，保证并发请求
/// 各自用自己的 locale、互不干扰。等价于 [`ts`]，但每次调用都做 yaml lookup
/// （不进 `TS_CACHE`：缓存按全局 locale 组织，per-request 命中率低且易出错）。
```
改为
```rust
/// 翻译查找的显式 locale 变体 —— **不**读 / 不写全局 `rust_i18n::locale()`。
///
/// 给需要在一进程内按不同 locale 取词的调用方（批量导出 / 测试断言）。等价于 [`ts`]，
/// 但每次调用都做 yaml lookup（不进 `TS_CACHE`：缓存按全局 locale 组织，多 locale 下易出错）。
```

- [x] **Step 8: `src/cli/args.rs` 帮助文案去「WEB 三模」**

L9-10：
```rust
/// 顶层 `so-novel-rs` 描述（短）。clap 在 usage 行尾 / 简略模式用。
const ABOUT_SHORT: &str = "So Novel — 简繁小说批量下载（CLI / GUI / WEB 三模）";
```
改为
```rust
/// 顶层 `so-novel-rs` 描述（短）。clap 在 usage 行尾 / 简略模式用。
const ABOUT_SHORT: &str = "So Novel — 简繁小说批量下载（CLI / GUI 双模）";
```

L12-22（`ABOUT_LONG`）整块改为：
```rust
/// 顶层 `so-novel-rs` 描述（长）。`--help` 全文模式用。
const ABOUT_LONG: &str = "\
So Novel — 简繁小说批量下载（CLI / GUI 双模）

不传任何子命令 → 启动 GPUI 桌面客户端；
带子命令 → 走 CLI 模式，复用同一份 parser / crawler / export。

全局 flag（-v / -q）对所有子命令生效：
  -v, --verbose  打开内部 tracing 日志（默认静默）
  -q, --quiet    抑制逐章进度与失败源 dump，脚本管道友好";
```

- [x] **Step 9: `locales/app.yml` 的 `Cli.about_short` / `Cli.about_long` 三语同步**

`about_short` 三条改为：
```yaml
  about_short:
    en: "So Novel — batch novel downloader (CLI / GUI)"
    zh-CN: "So Novel — 简繁小说批量下载（CLI / GUI 双模）"
    zh-TW: "So Novel — 簡繁小說批量下載（CLI / GUI 雙模）"
```

`about_long` 三条：删除 `With --web / --host / --port → start the Web server.` / `带 --web / --host / --port → 启动 Web 服务（GUI 模式）；` / `帶 --web / --host / --port → 啟動 Web 服務（GUI 模式）；` 各一行，并把标题行的 `(CLI / GUI / WEB)` / `（CLI / GUI / WEB 三模）` / `（CLI / GUI / WEB 三模）` 改成与 `about_short` 一致的 `(CLI / GUI)` / `（CLI / GUI 双模）` / `（CLI / GUI 雙模）`。

`about_long` 的 zh-CN 改完后应为：
```yaml
    zh-CN: |
      So Novel — 简繁小说批量下载（CLI / GUI 双模）

      不传任何子命令 → 启动 GPUI 桌面客户端；
      带子命令 → 走 CLI 模式，复用同一份 parser / crawler / export。

      全局 flag（-v / -q）对所有子命令生效：
        -v, --verbose  打开内部 tracing 日志（默认静默）
        -q, --quiet    抑制逐章进度与失败源 dump，脚本管道友好
```
en / zh-TW 同构（en 无 `；` 结尾，zh-TW 用繁体字与「全域 / 預設靜默 / 管線友善」等既有词汇）。

- [x] **Step 10: 验收 —— clippy 与 doc 0 警告**

Run:
```sh
cargo clippy --all-targets -- -D warnings 2>&1 | tail -10
```
Expected: `Finished`，0 warning / 0 error。

Run:
```sh
cargo doc --no-deps 2>&1 | grep -i 'warning\|error' || echo "CLEAN"
```
Expected: `CLEAN`（确认无 `broken_intra_doc_links`）。

Run:
```sh
grep -rn 'crate::web\|web-ui\|WebError\|web::handlers\|Web handler\|三端\|三模' src/ locales/ || echo "CLEAN"
```
Expected: `CLEAN`。

Run:
```sh
cargo test --lib 2>&1 | tail -3
```
Expected: `test result: ok. 487 passed; 0 failed; 4 ignored; ...`

---

## Task 5: 删除前端 monorepo 与 Docker 链路

**Files:**
- Delete: `web-ui/`（80 个跟踪文件）、`Dockerfile`、`docker-compose.yml`、`.dockerignore`、`.github/workflows/docker-release.yml`

**Interfaces:**
- Consumes: Task 1（`build.rs` 已不再引用 `web-ui/`；Task 1 之前删这里会让 `build.rs` 的 `rerun-if-changed` 指向不存在路径）。
- Produces: 仓库无前端工程、无容器构建链路。`release.yml` 与 `scripts/package-linux.sh` 不受影响（前者走 `cargo build --release`，后者走 cargo / cross）。

- [x] **Step 1: 删除 git 跟踪的前端与 Docker 文件**

Run:
```sh
git rm -r web-ui
git rm Dockerfile docker-compose.yml .dockerignore .github/workflows/docker-release.yml
```
Expected: `rm 'web-ui/...'` 共 80 行 + 4 行单文件。

- [x] **Step 2: 清掉未被 git 跟踪的构建残留**

`web-ui/node_modules/` 与 `web-ui/dist/` 未被 git 跟踪（`.gitignore` 未列，但 `git ls-files web-ui/dist` 返回 0），`git rm` 不会动它们，目录会残留。

**先确认残留内容：**
```sh
git status --short web-ui 2>/dev/null; ls web-ui 2>/dev/null || echo "web-ui 目录已不存在"
```
若 `web-ui/` 已不存在 → 跳过本步。若仍存在且**只剩** `node_modules` / `dist` 这类可再生构建产物：

```sh
rm -rf web-ui
```

> ⚠️ 这是本计划唯一的不可逆 `rm -rf`。执行前先跑上面的 `ls` 确认里面没有未跟踪的源文件（若看到 `.ts` / `.tsx` / `package.json` 之类的源码，**停手**，改为先 `git status --short web-ui` 查清为什么它们没被跟踪，再决定）。

Run:
```sh
ls web-ui 2>/dev/null || echo "web-ui gone"
```
Expected: `web-ui gone`。

- [x] **Step 3: 验收 —— 无残留引用、release 链路完好**

Run:
```sh
grep -rn 'web-ui\|web_ui\|bun\b' --include='*.rs' --include='*.toml' --include='*.yml' --include='*.sh' . 2>/dev/null | grep -v '^./target/' || echo "CLEAN"
```
Expected: `CLEAN`。

Run:
```sh
ls .github/workflows && grep -c 'cargo build --release' .github/workflows/release.yml
```
Expected: 只列出 `release.yml`；`grep -c` 输出 ≥ 1（release 走 cargo，未受影响）。

Run:
```sh
cargo check --all-targets 2>&1 | tail -5
```
Expected: `Finished`，0 warning。

---

## Task 6: 同步文档

**Files:**
- Delete: `docs/WEB.md`
- Modify: `README.md`、`docs/CLI.md`、`docs/BOOK_SOURCES.md`、`docs/CHANGELOG.md`

**Interfaces:**
- Consumes: Task 1-5 的最终代码形态。
- Produces: 文档与代码一致；无指向已删文件的链接。

- [x] **Step 1: 删除 `docs/WEB.md`**

Run:
```sh
git rm docs/WEB.md
```
Expected: `rm 'docs/WEB.md'`（455 行）。

- [x] **Step 2: `README.md` —— 改 3 处措辞**

L25（截图说明，去掉已经站不住的理由 —— 截的是桌面端）：
```markdown
> ⚠️ 旧截图已随 Web 前端迁移 shadcn 失效，待重新截图替换。
```
改为
```markdown
> ⚠️ 截图为旧版界面，待重新截图替换。
```

L79（结构树）：
```
    ├── core/              # 业务层（桌面 / Web / CLI 三端共享）
```
改为
```
    ├── core/              # 业务层（桌面 / CLI 两端共享）
```

L91（分层段）整行改为：
```markdown
**分层**: `core/` 提供与 GUI 解耦的业务逻辑,`desktop/` 是 GPUI 渲染层,`cli/` 是命令行入口,两端共享同一份核心代码。桌面端的文案与错误提示统一走 `i18n::ts*` 按全局 locale 翻译。
```

L234（贡献段的 clippy 命令 —— `--all-features` 在 feature 系统删除后与默认等价，改为 `--all-targets`）：
```markdown
* 提交前跑 `cargo fmt --all -- --check` + `cargo clippy --all-features --all-targets -- -D warnings` + `cargo test --lib`
```
改为
```markdown
* 提交前跑 `cargo fmt --all -- --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test --lib`
```

L237：
```markdown
* 业务函数返回错误 → 用 `AppResult<T>` + `?` 透传;边界(CLI / Web)才转 `anyhow`
```
改为
```markdown
* 业务函数返回错误 → 用 `AppResult<T>` + `?` 透传;边界(CLI)才转 `anyhow`
```

- [x] **Step 3: `README.md` —— 删结构树两行**

删除 L74：
```
├── web-ui/                # Turborepo + Bun monorepo（apps/web + packages/ui，shadcn base-nova）
```
删除 L81：
```
    ├── web/               # Web 服务（axum + 任务轮询）
```

- [x] **Step 4: `README.md` —— 删「🌐 Web 模式」与「🐳 Docker 部署」两节**

删除 L175-210 整块（保留 L174 空行与 L211 的 `### 📦 打包` 之间恰好一个空行）：

```markdown
### 🌐 Web 模式

启动 Web 服务器，通过浏览器访问：

```sh
# 先构建（web 是可选 feature，默认构建不含）
cargo build --features web
./target/debug/so-novel-rs --web --host 0.0.0.0 --port 9000

# 或一步构建并启动
cargo run --features web -- --web

# 环境变量（Docker 友好）
SO_NOVEL_WEB=1 ./target/debug/so-novel-rs
```

浏览器打开 `http://localhost:8080` 即可使用。支持手机、平板、桌面多端响应式。

> **默认绑定 `127.0.0.1:8080`，仅本机访问。** 如果需要在局域网或 Docker
> 容器中对外服务，显式传 `--host 0.0.0.0`。

### 🐳 Docker 部署

```sh
# 构建镜像
docker build -t so-novel .

# 运行（挂载数据目录）
docker run -d -p 8080:8080 -v so-novel-data:/home/so-novel/.sonovel --name so-novel so-novel

# 自定义端口
docker run -d -p 9000:8080 -e SO_NOVEL_WEB=1 so-novel
```

`config.toml` 存放在 `/home/so-novel/.sonovel/config.toml`（容器内），数据目录与 Dockerfile 的非 root 用户保持一致。
```

- [x] **Step 5: `docs/CLI.md` —— 改 3 处**

L3：
```markdown
`so-novel-rs` 是 **CLI / GUI / Web 三模** 程序。`main.rs` 根据参数分发：
```
改为
```markdown
`so-novel-rs` 是 **CLI / GUI 双模** 程序。`main.rs` 根据参数分发：
```

L9（删整行表格行）：
```markdown
| `so-novel-rs --web` | 启动 Web 服务器（[Web 模式](../README.md#-web-模式)） |
```

L178（`enable` / `disable` 行为说明，去掉已不存在的类型名）：
```markdown
- **写盘**：原子写到 `~/.sonovel/sources_config.json`，GUI 侧
  `WebState::sources_config` 也会读到（共享同一文件）。
```
改为
```markdown
- **写盘**：原子写到 `~/.sonovel/sources_config.json`，GUI 侧启动时
  也会读到（共享同一文件）。
```

L277：
```markdown
CLI 不读 `SO_NOVEL_WEB`（那是 Web 模式的开关）。其他可调项都在
```
改为
```markdown
CLI 的可调项都在
```

- [x] **Step 6: `docs/BOOK_SOURCES.md` 删 `WEB.md` 链接**

L179 整行删除：
```markdown
- [WEB.md](./WEB.md) — Web / Docker 部署
```

- [x] **Step 7: `docs/CHANGELOG.md` 加 `[Unreleased]` 条目**

在 `# Changelog` 之后、`## [0.4.0] - 2026-09-04` 之前插入：

```markdown
## [Unreleased]

### Removed

- **Web 端整体移除**：`src/web/`（axum 服务，15 文件）、`web-ui/`（Turborepo + Bun
  monorepo，80 文件）、`src/startup/web.rs`、`Dockerfile` / `docker-compose.yml` /
  `.dockerignore` / `.github/workflows/docker-release.yml`、`docs/WEB.md`、
  `locales/app.yml` 的 `WebErrors:` 段
- **feature 系统取消**：删 `[features]` 段（`default` / `gui` / `web`），`gpui-kit` 与
  `rfd` 转为必选依赖，`src/` 内全部 `#[cfg(feature = ...)]` 门控移除
- **依赖清理**：删 `axum` / `axum_session` / `tower-http` / `rust-embed` /
  `mime_guess` / `async-stream` / `futures`，dev-dep 删 `tower`
- **孤儿代码**：`core::library` 删 `extension_to_content_type` / `list_library_entries` /
  `safe_file_path` / `open_download_file` / `OpenFileError`；`core::sources` 删
  `find_rule_by_id_cloned` / `find_rule_by_url`；连带删除其 16 个单元测试

仓库由此收敛为 **GUI + CLI** 两形态。`cargo test --lib` 由 510 passed 降至 487 passed。

```

- [x] **Step 8: 验收 —— 无死链**

Run:
```sh
grep -rn 'WEB.md\|--web\|SO_NOVEL_WEB\|docker\|web-ui\|Web 模式\|Web 服务' README.md docs/*.md || echo "CLEAN"
```
Expected: `CLEAN`。

Run:
```sh
head -20 docs/CHANGELOG.md
```
Expected: `# Changelog` → 空行 → `## [Unreleased]` → `### Removed` → 各条目 → `## [0.4.0] - 2026-09-04`。

Run:
```sh
grep -n '打包\|## 📥 安装\|## 🤝 贡献' README.md
```
Expected: 三节标题仍存在且顺序正确（确认删块没有吃掉相邻标题）。

---

## Task 7: 全量验收

**Files:**
- Modify: `tasks/todo.md`（追加 review 段）、`tasks/lessons.md`（退役/更新已失效的 lesson）

**Interfaces:**
- Consumes: Task 1-6 全部产物。
- Produces: 一个通过全部质量门、且 grep 全净的仓库。

> `tasks/` 在 `.gitignore` 中（L21），是本地工作流文件，不进 commit。

- [x] **Step 1: 跑完整质量门**

Run:
```sh
cargo fmt --all -- --check
```
Expected: 无输出（exit 0）。

Run:
```sh
cargo clippy --all-targets -- -D warnings 2>&1 | tail -10
```
Expected: `Finished`，0 warning。

Run:
```sh
cargo test --lib 2>&1 | tail -3
```
Expected: `test result: ok. 487 passed; 0 failed; 4 ignored; ...`

- [x] **Step 2: 全仓缺席断言**

Run:
```sh
grep -rn 'feature = "web"\|feature = "gui"\|CARGO_FEATURE_WEB\|SO_NOVEL_WEB\|no-default-features' src/ build.rs Cargo.toml .github/ scripts/ || echo "CLEAN"
```
Expected: `CLEAN`。

Run:
```sh
grep -rn 'axum\|rust-embed\|mime_guess\|tower-http\|async-stream\|axum_session' src/ build.rs Cargo.toml || echo "CLEAN"
```
Expected: `CLEAN`。

Run:
```sh
grep -rni 'web-ui\|WebError\|crate::web\|Web handler\|Web UI\|web::' src/ locales/ README.md docs/CLI.md docs/BOOK_SOURCES.md docs/CHANGELOG.md || echo "CLEAN"`
```
Expected: `CLEAN`。

Run:
```sh
cargo tree -e normal --depth 1 2>/dev/null | grep -iE 'axum|rust-embed|mime_guess|tower-http|async-stream' || echo "CLEAN"
```
Expected: `CLEAN`。

- [x] **Step 3: 保留物完整性检查（对照设计文档「明确保留」节）**

Run:
```sh
ls bundle/web/ && ls bundle/rules/ && ls screenshots/ | head -3
```
Expected: `bundle/web/` 仍有 `chapter.html` / `cover.jpg` / `js/`（7 个跟踪文件）；`bundle/rules/` 与 `screenshots/` 完好。

Run:
```sh
git ls-files bundle/web | wc -l && git ls-files src/cli | wc -l && git ls-files src/desktop | wc -l
```
Expected: `bundle/web` = 7；`src/cli` 与 `src/desktop` 数量与改动前一致（未被误删）。

- [x] **Step 4: 冒烟测试（CLI 与 GUI 都还能起）**

Run:
```sh
cargo run --quiet -- --version
```
Expected: 输出 `so-novel-rs <version>`（`0.4.0`），exit 0 —— 证明 CLI 分支可用。

Run:
```sh
cargo run --quiet -- sources list --json | head -5
```
Expected: 打印书源 JSON 数组片段，exit 0 —— 证明 `core::sources` 清理后 CLI 仍正常。

Run:
```sh
cargo run --quiet -- --help | head -20
```
Expected: 输出中文帮助，且**不含** `--web` / `--host` / `--port` / `WEB 三模` 字样 —— 证明 Task 4 Step 8-9 的文案改动生效。

> GUI 分支需要图形环境，无法在此自动断言。**必须人工确认**：双击 / 运行 `cargo run`（无参数）能正常打开桌面窗口。若不通过，检查 Task 1 Step 7 的 `dispatch` 是否把 `crate::logger::init()` 与 `crate::desktop::run()` 都保留在 `Gui` 臂。

- [x] **Step 5: 追加 `tasks/todo.md` review 段**

在 `tasks/todo.md` 末尾追加：

```markdown
## Review — 移除 Web 端实现（2026-10-04）

- 计划：`docs/superpowers/plans/2026-10-04-remove-web.md`
- 设计：`docs/superpowers/specs/2026-10-04-remove-web-design.md`
- 删除 git 跟踪文件 101 个：`src/web/` 15 + `src/startup/web.rs` 1 + `web-ui/` 80 +
  Docker 三件 + `docker-release.yml` + `docs/WEB.md`
- `cargo test --lib`：510 → 487 passed（-23：i18n 2 + `core::sources` 5 + `core::library` 11 + `core::config_helpers` 5）；4 ignored 不变
- feature 系统整体取消；`gpui-kit` / `rfd` 转必选
- 与设计文档的差异见计划文件顶部「与设计文档的五处差异」表
- 未提交（`tasks/lessons.md` L1：用户未说「提交」）
```

- [x] **Step 6: 更新 `tasks/lessons.md`（退役已失效的 lesson）**

本次移除让多条 lesson 的适用面归零或收窄。逐条处理：

- **L3**（`cargo check --no-default-features --features web` 是 dead_code 真实场景）：**改写**为一般化的表述 —— feature 门控已取消，但"给 GUI-only 函数加 cfg gate 时用 `feature = "gui"` 而非 `not(feature = "web")`"这条推理仍值得留作方法论（cfg 维度要跟实际调用方走）。在规则开头加一句 `（2026-10-04：feature 系统已随 Web 端移除而取消，本条降级为历史方法论）`。
- **L2**（HeroUI v3 API）、**L4**/**L5**/**L7**（Dockerfile 的 dash / PIPESTATUS / SHELL 坑）、**L6**（frontend `node_modules` 残缺）：**删除**。前置条件（`web-ui/` 与 `Dockerfile`）已不存在，留着是给后来者的错误地图。
- 新增 **L8**：

```markdown
## L8 — 删除子系统时，同文件内的单元测试也是「孤儿」，必须一起删

**触发：** 移除 Web 端时，设计文档只列了 `core::library` / `core::sources` 里要删的函数，没提这些函数在**同文件内**有单元测试（2026-10-04）

**规则：** 规划删除时，对每个待删 `pub` 项，除了 grep 调用方，还要 grep **同文件测试模块内的引用**。`extra` 教训：`empty_inputs_are_safe` 这类"聚合断言"测试会同时引用要保留和要删的函数 —— 它是**改**不是**删**。

**Why：** 本项目是 lib crate，孤儿的 `pub` 项不触发 `dead_code`，编译器不会提醒你测试还在引用它；只删函数会直接编译失败，或（若测试被 feature-gate 掉）留下永久红。
```

Run:
```sh
grep -c '^## L' tasks/lessons.md
```
Expected: `6`（原 7 条：L1 + L3 保留 + L8 新增 = 3… 实际应为 L1 / L3 / L8 三条主体，但编号不重排时保留原编号，故计数为 3 个 `## L` 段落）。以实际编辑结果为准，关键是 L2 / L4 / L5 / L6 / L7 已不在文件中。

---

## 完成标准（Self-Review 清单）

| 检查项 | 判定 |
|---|---|
| `cargo fmt --all -- --check` | 无输出 |
| `cargo clippy --all-targets -- -D warnings` | 0 warning |
| `cargo test --lib` | 487 passed / 0 failed / 4 ignored |
| `cargo doc --no-deps` | 0 warning（无断链） |
| `cargo tree` 无 axum / rust-embed / mime_guess / tower-http | 是 |
| `bundle/web/` 7 个文件完好 | 是 |
| `README.md` / `docs/CLI.md` / `docs/BOOK_SOURCES.md` 无死链、无 `--web` | 是 |
| `docs/CHANGELOG.md` 有 `[Unreleased] → Removed` | 是 |
| CLI 冒烟（`--version` / `sources list --json` / `--help`） | 全通过 |
| GUI 冒烟（无参数启动窗口） | **人工确认** |
| 已提交 | **否 —— 等用户指示** |
