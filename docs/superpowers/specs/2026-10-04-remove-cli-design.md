# 移除 CLI 端实现 + 顶层目录重排 — 设计文档

- 日期：2026-10-04
- 状态：已确认（删除范围 / 顶层布局方案 B 两项决策获用户批准）

## 背景与目标

上一轮移除 Web 端后，仓库收敛为 **GUI + CLI** 两形态。本轮再砍掉 CLI：
`src/cli/**` 维护成本高于价值（独立 clap 帮助树 + 本地化 help 手搓、
三套子命令、TTY 进度/取消逻辑），而桌面端已能覆盖全部功能。

目标：

1. 仓库收敛为**纯桌面形态**，不留下孤儿代码、失效文档、断链注释或已无消费者的依赖。
2. **顶层目录重排**：消灭名实不符、混装两类内容的 `bundle/`，把截图收进 `docs/`。

> **附带影响**：紧邻本轮之前的「CLI 后台任务增量反馈优化」改动全部落在
> `src/cli/{search,download}.rs` 与 `src/utils/tty.rs`，正是本轮删除对象 ——
> 该任务整体作废。用户已知悉并确认。

## 已确认决策

| 决策点 | 结论 |
|---|---|
| 删除范围 | **CLI 全套移除**：`src/cli/**` + `src/startup/**` + `utils::tty` + `clap` / `windows-sys` 依赖 + `docs/CLI.md` + README/i18n 相关段 |
| 顶层布局 | **方案 B**：`bundle/rules/` → `assets/rules/`；`bundle/web/` → `tests/fixtures/web/`；`screenshots/` → `docs/screenshots/`；`bundle/` 目录消失 |
| `src/startup/` | **内联进 `main.rs`** —— GUI-only 后 `detect`/`dispatch`/`LaunchMode` 全无意义 |
| 可恢复性 | **不打 tag / 分支** —— git history 永久保留，无需额外噪音 |
| `tokio` 依赖 | **保留**；仅 `signal` feature 候选摘除（需 build 验证），`time` 必留（crawler `sleep`） |

## 一、直接删除的代码

| 目标 | 规模 |
|---|---|
| `src/cli/**` | 6 文件（`mod` / `args` / `search` / `download` / `sources` / `tests`） |
| `src/startup/mod.rs` | 1 文件 |
| `src/utils/tty.rs` | 1 文件 |
| `docs/CLI.md` | 1 文件（312 行） |

合计 9 个被 git 跟踪的文件。

## 二、Cargo 依赖收敛

| 目标 | 处理 |
|---|---|
| `clap = { version = "4", features = ["derive"] }` | 删依赖（含上方 `# CLI 子命令。` 注释） |
| `[target.'cfg(target_os = "windows")'.dependencies]` 整块（`windows-sys` + `Win32_System_Console`） | 删依赖（唯一消费者是 `attach_parent_console`，随 `startup/` 一起走） |
| `tokio` 的 `signal` feature | 候选摘除 —— 全仓 `tokio::signal` 仅 `src/cli/download.rs` 的 Ctrl-C 注册在用；实现时若 `cargo build` 通过即摘，否则保留 |
| `tokio` 的 `time` feature | **保留** —— `crawler/{download,resolve}.rs` 的 `tokio::time::sleep` 在用 |
| `winres` build-dep + `build.rs` 图标段 | **保留** —— 与 CLI 无关 |

## 三、启动层（`startup/` 内联）

`src/main.rs` 由「argv 收集 → 委托 `startup::dispatch`」收敛为直接启动 GUI：

```rust
// Windows release 下走 GUI subsystem，避免启动时弹出控制台黑窗。
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::Result;

fn main() -> Result<()> {
    so_novel_rs::logger::init();
    so_novel_rs::desktop::run()
}
```

随之内联/删除的内容：

- `LaunchMode` enum、`detect()`、`dispatch()` —— 全删（GUI-only 无分支可言）
- `attach_parent_console()` —— 删（含 `allow(unsafe_code)` 标记）。它是 `#![deny(unsafe_code)]`
  的唯一例外，删后 `windows-sys` 依赖与 unsafe 例外一起消失
- `src/lib.rs` 删 `pub mod cli;` 与 `pub mod startup;`

> `logger::init()` 从 `dispatch` 的 Gui 分支搬到 `main` 首行，调用时机不变。

## 四、连带孤儿清理

删除 CLI 后失去唯一消费者的代码：

| 位置 | 处理 | 说明 |
|---|---|---|
| `core::bootstrap::cli_load_paths_and_config` | 删 | 仅 `cli::run` 用 |
| `core::bootstrap::effective_cfg` | 删 | 仅 `cli::{download,tests}` 用 |
| `core::bootstrap::load_active_sources` | 删 | 仅 `cli::{search,download}` 用 |
| `core::bootstrap::validate_range` | 删 | 仅 `cli::{download,tests}` 用 |
| `core::bootstrap::load_context` | **保留** | `desktop::model::mod` 在用 |
| `core::bootstrap` 的 `mod tests`（10 个测试） | 删 | 全部针对上述 4 个被删函数 |
| `logger::init_compat_legacy` | 删 | 仅 `cli::run` 的 `--verbose` 分支用 |
| `logger.rs` 的 `init_compat_legacy_does_not_panic` | 删 | 1 个测试 |
| `utils::tty` 整个模块 | 删 | 仅 CLI 进度行用；`utils/mod.rs` 去掉 `pub mod tty;` 与模块头提及 |
| `utils::system::open_path` | **保留** | 桌面书库页 / 任务页在用 |

> **注**：本项目是 lib crate，`pub` 孤儿**不会**触发 `dead_code` 警告。上述清理是
> 必要的可读性维护，不是编译前置条件 —— 但留着就是给后来者的错误地图。

### 注释与文档链接修正

`clippy --all-targets -- -D warnings` 会挂断链 intra-doc link，故下列注释措辞必须同步：

| 位置 | 改动 |
|---|---|
| `src/main.rs:1-8` | 模块头删「CLI 模式通过 `startup::attach_parent_console`…」「职责委托给 `startup`」 |
| `src/logger.rs:7` | 删「`cli::run`（`--verbose`）与 `startup::dispatch`（Gui 路径）已分流」措辞 |
| `src/config/mod.rs:34` | 「仅在启动期（`main` / `startup` 模块）调一次」→ 「启动期（`main`）调一次」 |
| `src/i18n.rs:18,32` | 删「CLI 不依赖 `desktop`…」「CLI 路径走 `locale_for`」措辞 |
| `src/utils/mod.rs:1-9` | 模块头删 `cli` 复用声明与 `tty` 子模块描述 |

## 五、顶层目录重排（方案 B）

### 目标布局

```
assets/                    # 唯一「编译期嵌入的静态资源」目录
  logo.{ico,png,svg}
  chapter_{html,epub}.tmpl
  rules/                   # 原 bundle/rules（include_str! 嵌入 + release 随包分发）
    {main,cloudflare,no-search,rate-limit,proxy-required}.json
    rule-template.json5
docs/
  BOOK_SOURCES.md  CHANGELOG.md
  screenshots/             # 原根 screenshots/（4 png）
  superpowers/{plans,specs}/
locales/app.yml            # 保持根级（rust-i18n 惯例）
scripts/                   # {install-hooks.sh, package-linux.sh}
src/
tests/
  fixtures/web/            # 原 bundle/web（书源解析样例，2 个测试在读）
    chapter.html  cover.jpg  js/*.js
README.md  AGENTS.md  DISCLAIMER.md  LICENSE
Cargo.toml  Cargo.lock  build.rs  rustfmt.toml  .clippy.toml  .editorconfig
```

顶层受控目录：7 → 6（`assets docs locales scripts src tests`），且 `bundle/` 消失。
`assets/` 语义收敛为「所有 `include_str!` / `include_bytes!` 进来的东西」——规则本就编译期
嵌入，归入其中自洽。

### 为什么 `tests/fixtures/web/`

`bundle/web/` 是调规则时的真实 HTML / JS 样例，有 2 个单元测试在读：

- `src/parser/dom/selector.rs::parses_real_chapter_html_resource` → `chapter.html`
- `src/js/runtime.rs` 的 `repo_web()` → `js/96dushu-chapter.js`

放进 `tests/fixtures/` 是 Rust 惯例。**风险**：根 `tests/` 会被 Cargo 当作 integration test
目标目录，但 `tests/fixtures/web/` 下无 `.rs` 文件，Cargo 不会生成任何测试二进制，安全。

## 六、路径引用点（必须同步改）

| 类别 | 位置 | 改动 |
|---|---|---|
| 编译期嵌入 | `src/db/rules/constants.rs` 5 处 `include_str!("../../../bundle/rules/*.json")` | → `../../../assets/rules/*.json` |
| 测试 helper | `src/db/rules/loader.rs:167` `.join("bundle").join("rules")` | → `.join("assets").join("rules")` |
| 测试 helper | `src/db/rules/init.rs:82` 同上 | 同上 |
| 测试 helper | `src/js/runtime.rs:117-120` `repo_web()` `.join("bundle").join("web")` | → `.join("tests").join("fixtures").join("web")` |
| 测试 helper | `src/parser/dom/selector.rs:421` `.join("bundle").join("web").join("chapter.html")` | → `tests/fixtures/web/chapter.html` |
| CI 打包 | `.github/workflows/release.yml:95,107` `cp -r bundle/rules` | → `cp -r assets/rules`（注释 L79 同步） |
| 打包脚本 | `scripts/package-linux.sh:74` `cp -r bundle/rules` | → `assets/rules`（注释 L6 同步） |
| gitignore | `.gitignore:13-16` `/bundle/*.db*` | 删这 4 行（`bundle/` 不复存在） |

> `assets/NotoSansCJK-Regular.ttc` 只是 `src/export/pdf/fonts.rs` 的候选字体路径之一
> （文件不存在，走 `p.exists()` 跳过），不受本次移动影响。

## 七、文档

| 文件 | 改动 |
|---|---|
| `docs/CLI.md` | 整文件删除 |
| `README.md` | 删 L17 导航的 `[CLI](#-cli-用法)` 段；删 L46「💻 CLI 模式」功能行；删 L133-153「💻 CLI 用法」整节；结构树删 `cli/`（L77）行、`core/` 注释改「桌面端」（L78）、`docs/` 注释去掉「CLI」（L72）；L89「分层」段去掉 `cli/` 是命令行入口的表述；L199「边界(CLI)才转 anyhow」改写 |
| `docs/BOOK_SOURCES.md` | L3-4 / L119 / L165-166 的 `bundle/rules/` → `assets/rules/`（含 2 处相对链接）；L178 删指向已删除 `./CLI.md` 的链接 |
| `docs/CHANGELOG.md` | `[Unreleased] → Removed` 追加本轮条目；修正 L21「收敛为 GUI + CLI 两形态」的过时表述 |
| `locales/app.yml` | 删 `Cli:` 段（L1247 → 文件尾） |
| `tasks/todo.md` / `tasks/lessons.md` | 按项目规约在实现后追加 review / lesson（`tasks/` 已被 gitignore） |

## 八、明确保留

- `assets/` 的 logo（`build.rs` 图标 + `desktop/logo.rs`）与导出模板（`export/render.rs`）
- `locales/app.yml` 除 `Cli:` 外的全部段
- `docs/superpowers/`（历史规划 / 设计记录）、`docs/CHANGELOG.md`、`docs/BOOK_SOURCES.md`
- `scripts/`、`.githooks/`、`.github/workflows/release.yml`
- `src/desktop/**`、`src/core/**`、`src/crawler/**`、`src/parser/**`、`src/export/**`、
  `src/db/**`、`src/js/**`、`src/http/**`、`src/models/**`、`src/config/**`
- `tests/fixtures/web/` 全部 7 个样例文件（只挪位置，不删）

## 九、验证方式

实现完成后按顺序跑：

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --lib
grep -n 'clap\|windows-sys' Cargo.toml                  # 应无输出（直接依赖）
grep -rn 'crate::cli\|startup::\|utils::tty\|bundle/' src/   # 应无输出
ls bundle 2>/dev/null && echo "FAIL: bundle still exists"
```

预期：

- `cargo test --lib` 由 **485 passed / 4 ignored** 降至约 **439 passed**
  （−46：`cli` 33（`tests.rs` 29 + `sources.rs` 4）+ `bootstrap` 10 +
  `utils::tty` 2 + `logger` 1）。实现时以改动前实测基线为准，plan 里锁定精确数字。
- fmt / clippy 保持 0 警告。
- 未被删除的测试（`db::rules`、`js::runtime`、`parser::dom::selector`）在**新路径**下继续通过
  —— 这是路径重排正确性的直接证据。

## 风险与对策

| 风险 | 对策 |
|---|---|
| 漏改 `include_str!` 路径 → 编译失败 | 编译期硬失败会立刻暴露，5 处一次改全 |
| 漏删孤儿 → clippy 不报但留下错误地图 | 按第四节清单逐条 grep 确认无调用方后再删 |
| intra-doc link 断裂挂 clippy gate | 全仓 grep `crate::cli` / `startup::` / `utils::tty`，逐一改写 |
| `bundle/web` 样例被误删 | 只 `git mv` 到 `tests/fixtures/web/`，`git ls-files tests/fixtures/web \| wc -l` 应 = 7 |
| CI / 打包脚本仍指向 `bundle/rules` | 第六节逐条改 `release.yml` 与 `package-linux.sh`，并 grep `bundle` 确认 0 命中 |
| `tokio` 摘 `signal` 后某处隐藏用法编译失败 | 摘 feature 后立即 `cargo build`；失败则保留该 feature，不做进一步裁剪 |
| 根 `tests/` 被 Cargo 误当测试目标 | 目录下无 `.rs`，不生成测试二进制；实现后 `cargo test` 输出目标列表核对 |
