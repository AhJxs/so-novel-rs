<div align="center">

<img src="assets/logo.png" alt="So Novel" width="128" height="128" />

# So Novel

**多源聚合小说搜索下载器 · Rust + GPUI 桌面客户端**

原生桌面应用，支持多源搜索、并发下载、简繁转换与多格式导出。

[![Release](https://img.shields.io/github/v/release/Ahjxs/so-novel-rs?style=flat&label=version&color=green)](https://github.com/Ahjxs/so-novel-rs/releases/latest)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey)](#-快速开始)
[![GitHub stars](https://img.shields.io/github/stars/Ahjxs/so-novel-rs?style=flat)](https://github.com/Ahjxs/so-novel-rs/stargazers)

[功能](#-功能) · [技术栈](#-技术栈) · [快速开始](#-快速开始) · [CLI](#-cli-用法) · [快捷键](#-快捷键) · [免责声明](./DISCLAIMER.md)

</div>

---

## 📸 截图

| 搜索 | 任务 |
|:---:|:---:|
| ![搜索](screenshots/search.png) | ![任务](screenshots/task.png) |

| 书库 | 设置 |
|:---:|:---:|
| ![书库](screenshots/library.png) | ![设置](screenshots/settings.png) |

## ✨ 功能

| | |
|---|---|
| 🔍 **多源搜索** | 聚合多书源并发搜索、相似度过滤排序、quanben5 加密搜索、详情面板、封面预览、选章下载 |
| 📥 **下载任务** | 并发抓取、失败重试、进度跟踪、取消、封面嵌入、持久化、章节范围、**简繁自动转换** |
| 📚 **本地书库** | 扫描已下载书籍，按格式/日期/大小排序，删除二次确认（无空态闪烁） |
| 🔌 **书源管理** | 多规则文件切换、JSON 导入、启用/禁用、连通性测速 |
| 📄 **多格式导出** | EPUB / TXT（多编码）/ HTML（zip 打包）/ **PDF**（DocumentBuilder 直接构建，CJK 字体嵌入） |
| 🎨 **主题系统** | 38 个可用主题，文件 watcher 热重载，无需重启 |
| 🌐 **多语言** | 简体中文 / 繁体中文 / English，UI 即时切换 |
| 💻 **CLI 模式** | `search` / `download` / `sources` 子命令，`--json` 机器可读输出 |
| 🔄 **更新检查** | 自动检测 GitHub Release，有新版时一键跳转下载 |

## 🛠 技术栈

| 领域 | 选型 |
|------|------|
| 🎨 GUI | [GPUI 0.2](https://gpui.rs) + [gpui-component 0.5](https://github.com/longbridge/gpui-component) |
| ⚡ 异步 | [tokio 1](https://tokio.rs) (rt-multi-thread) |
| 🌐 HTTP | [reqwest 0.13](https://docs.rs/reqwest) (rustls，无 OpenSSL) |
| 🔍 HTML 解析 | scraper 0.27 + regex |
| 📜 JS 引擎 | [boa_engine](https://github.com/boa-dev/boa)（书源规则 `@js:` 后处理 + 加密） |
| 💾 持久化 | JSON 文件（原子写入，零依赖） |
| ⚙️ 配置 | [toml_edit](https://docs.rs/toml_edit)（保留注释与字段顺序） |
| 🌏 国际化 | [rust-i18n](https://docs.rs/rust-i18n)（编译期嵌入） |
| 🈶 简繁转换 | [zhconv](https://docs.rs/zhconv)（OpenCC + MediaWiki 词表，纯 Rust） |
| 📦 导出 | epub-builder / zip / encoding_rs / pdf_oxide |
| 📂 文件选择 | [rfd](https://docs.rs/rfd) `AsyncFileDialog` |

## 📂 项目结构

```
so-novel-rs/
├── assets/                # logo
├── bundle/
│   ├── rules/             # 默认书源 JSON（首次启动复制到 ~/.sonovel/rules/）
│   └── web/               # Web 前端静态资源
├── locales/app.yml        # i18n 翻译（zh-CN / zh-HK / en）
└── src/
    ├── main.rs            # 进程入口（18 行：crane attribute + 委托给 startup）
    ├── startup/           # 启动层：mode 判定 / console attach / dispatch
    │   ├── mod.rs         # LaunchMode / detect / dispatch / attach_parent_console
    │   └── web.rs         # web bootstrap（paths → config → rules → http → tasks → axum）
    ├── cli/               # CLI 子命令（args / search / download / sources / util / tests）
    ├── app/               # 业务层（与 GUI 解耦）
    │   └── ops/           # download / search / sources / library / settings / update
    ├── config/            # config.toml 读写
    ├── i18n/              # rust_i18n 包装 + locale_for
    ├── persistent/        # JSON 持久化（tasks.json / sources_config.json / rules/）
    ├── models/            # Rule / Book / Chapter / SearchResult / TaskRecord
    ├── crawler/           # 搜索 / 下载 / 重试 / 健康检测
    ├── parser/            # HTML 解析
    ├── http/              # HTTP 客户端 / 代理 / CF 旁路
    ├── export/            # EPUB / TXT / HTML / PDF
    ├── js/                # boa_engine（@js: 后处理）
    ├── gpui_app/          # GPUI 桌面 GUI（5 个页面）
    ├── web/               # Web 服务（axum + SSE）
    └── util/              # 工具函数
```

## 🚀 快速开始

```sh
# 克隆 & 编译
git clone https://github.com/Ahjxs/so-novel-rs.git
cd so-novel-rs
cargo run
```

> **前置依赖**：Rust 1.85+，Windows / macOS / Linux 均可。Windows 下首次 GPUI 构建需设 `GPUI_FXC_PATH`（详见 [build.rs](./build.rs)）。

应用数据存放在 `~/.sonovel/`，首次启动自动创建：

| 路径 | 用途 |
|------|------|
| `config.toml` | 用户配置（保留注释） |
| `rules/` | 书源规则文件（JSON，首次启动从 bundle 复制） |
| `sources_config.json` | 书源配置（当前选中的规则文件 + 禁用列表） |
| `tasks.json` | 下载任务记录（自动清理超额的已完成任务） |
| `themes/` | 用户主题目录（JSON，热重载） |

### 💻 CLI 用法

不带子命令启动 GUI，带子命令走 CLI：

```sh
# 搜索（与 GUI 一致：自动按相似度过滤 + 排序）
so-novel-rs search "斗破苍穹"
so-novel-rs search "斗破苍穹" --source 1 --limit 10 --json | jq length

# 下载
so-novel-rs download "https://example.com/book/123" --format epub
so-novel-rs download "https://example.com/book/123" --output D:\novels --format txt

# 列出书源
so-novel-rs sources --json
```

`--help` / `-h` / 子命令 help 跟随 `~/.sonovel/config.toml [global].language`
显示对应语言（zh-CN / zh-TW → zh-HK / en），与 GUI 同步。

📖 完整 CLI 用法、子命令参数、注意事项、故障排查见 [docs/CLI.md](./docs/CLI.md)。

### 📚 书源

仓库自带 6 套书源规则（位于 `bundle/rules/`，首次运行复制到 `~/.sonovel/rules/`）：

- `main.json` — 默认书源（12 个，均支持搜索、大陆 IP）
- `proxy-required.json` — 需要代理的书源（4 个，非大陆 IP）
- `rate-limit.json` — 下载限流的源（4 个）
- `no-search.json` — 不支持搜索的源（2 个）
- `cloudflare.json` — 有 Cloudflare 保护的源（3 个）
- `rule-template.json5` — 自定义书源模板

切换书源集：在 `config.toml` 改 `active-rules` 字段；Cloudflare 保护的书源需要
部署 [CloudflareBypassForScraping](https://github.com/sarperavci/CloudflareBypassForScraping)
反代并设置 `cf-bypass`。

📖 完整书源表（IP 要求 / 注意事项）、CF 绕过部署步骤、排查指引见
[docs/BOOK_SOURCES.md](./docs/BOOK_SOURCES.md)。

### 🌐 Web 模式

启动 Web 服务器，通过浏览器访问：

```sh
# 命令行启动
so-novel-rs --web
so-novel-rs --web --host 0.0.0.0 --port 9000

# 环境变量（Docker 友好）
SO_NOVEL_WEB=1 so-novel-rs
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

### 📦 打包

```sh
cargo build --release                                       # 当前平台（Windows 无控制台窗口）
cargo build --release --target x86_64-unknown-linux-gnu     # Linux
cargo build --release --target aarch64-unknown-linux-gnu    # Linux ARM64
```

产物在 `target/<triple>/release/so-novel-rs[.exe]`，可单独分发。

## ⌨️ 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Cmd/Ctrl + 1..5` | 直跳页面（搜索 / 任务 / 书库 / 书源 / 设置） |
| `Cmd/Ctrl + B` | 折叠 / 展开 Sidebar |
| `F6` / `Shift+F6` | 翻页（避开 Input 的 Tab 绑定） |
| `Escape` | 关闭 Dialog / Sheet / Notification |

## 🛠 开发与质量门禁

本项目对企业级工程标准有完整配置，提交前**必须**通过以下门禁：

```sh
# 1. 格式
cargo fmt --all -- --check

# 2. 严格 clippy (pedantic + nursery)
cargo clippy --all-features --all-targets -- -D warnings

# 3. 单元 + 集成测试
cargo test --all-features --lib

# 4. 文档检查
cargo doc --no-deps --all-features
```

**Lint 规则** 在 `Cargo.toml [lints.rust]` + `[lints.clippy]` 集中声明：

| 类别 | 等级 | 说明 |
|------|------|------|
| `unsafe_code` | deny | 全仓禁 `unsafe`（除非有 `// SAFETY:` 注释） |
| `missing_docs` | warn | `pub` 项必须有 `///` 文档 |
| `unwrap_used` / `expect_used` / `panic` | warn | 业务层禁裸 unwrap（测试模块除外） |
| clippy `pedantic` + `nursery` | warn | 渐进式收紧，PR #1 起批量清 |

完整 lint 阈值在 `.clippy.toml`（圈复杂度 25 / 函数参数 8 / 类型复杂度 300）。

## ⚡ 性能要点

| 场景 | 关键优化 | 文件 |
|------|----------|------|
| HTTP 连接复用 | 共享 `reqwest::Client`（连接池 + TLS session） | `src/http/clients.rs` |
| 章节并发抓取 | `tokio::Semaphore` 限并发 + `JoinSet` 拉取 | `src/crawler/mod.rs` |
| 指数退避重试 | `rand` 抖动 [min, max] 区间，避免雷鸣群 | `src/crawler/retry.rs` |
| 增量文件监听 | `gpui_component` 0.5.1 自带 `notify` 7.0 + smol timer | `src/gpui_app/pages/library/watcher.rs` |
| 简繁转换 | zhconv 词表 Aho-Corasick 编译期嵌入，零运行时 IO | `src/utils/zhconv.rs` |
| 主题加载 | 21 个 JSON `include_str!` 嵌入二进制 | `src/gpui_app/themes.rs` |
| PDF 生成 | `pdf_oxide` 直接拼文本，不走 HTML/CSS 渲染流水线 | `src/export/pdf.rs` |
| 配置文件写 | tmp + fsync + rename 原子写，断电最坏"老文件还在" | `src/db/mod.rs::write_atomically` |

## 🔧 排障指引

| 症状 | 原因 | 处置 |
|------|------|------|
| 首次 `cargo run` 卡在 web UI build | `tsc --noEmit && vite build` 需要 Node.js | 装 Node 18+；或设 `SO_NOVEL_SKIP_WEB_BUILD=1` 跳过前端（仅 Rust 静态分析用） |
| `unsafe_code` deny 触发 | 业务层新增 `unsafe` 块 | 写明 `// SAFETY: <理由>` 注释 + 在 `Cargo.toml` 申请豁免或 `#[allow(unsafe_code)]` 标注 |
| GUI 启动 panic: `tokio::time::sleep` 在 smol executor | `gpui 0.2.2 cx.spawn` 跑在 smol, 不接 tokio reactor | 用 `cx.background_executor().timer(...)` 替代；channel 用 `smol::channel` |
| 配置文件改完启动报错 "字段超出合法范围" | PR #6 加了 `AppConfig::validate()` 启动期校验 | 按报错改 `font_size ∈ [12,24]` / `min_interval <= max_interval` / `download_path` 非空 |
| 书源搜不到结果但能 ping 通 | 命 Cloudflare 验证，搜索结果未登录态被截断 | 配 `cf-bypass = "http://your-bypass-host:port"`，或部署 CloudflareBypassForScraping 反代 |
| `cargo clippy` 一堆 `unwrap_used` warning | PR #5a 起的 pedantic lint | 业务代码 `?` 透传；测试模块 `#[cfg(test)] mod tests` 已自动豁免；初始化常量 `expect("static")` 加 `// SAFETY:` |
| 章节正文为空但书源能加载详情 | 规则 `chapter.filter` 把所有段落都过滤掉了，或源站就是空 | 用 `chapter-filter-rule = ""` 临时关掉过滤看原始内容；调整规则 |

## 🏗 企业级架构改造记录 (2026-07)

最近一次大规模重构，16 个 PR 分批落地：

| PR | 主题 | 关键产出 |
|----|------|----------|
| #1 | 目录重命名 + lint 收紧 | `util→utils`、`persistent→db`、`.clippy.toml`、pedantic lint |
| #2 | 错误体系根 | `AppError` 14 变体 + `AppResult<T>` + `From<ExportError/anyhow/io/Json/Toml>` |
| #3 | 错误码表 | 41 变体 `ErrorCode` 数字码 (1xxx-5xxx)，`web::WebError::message` 委托 |
| #4 | utils 文档化 | 模块总览 + 5 lock 测试 + 3 doctest |
| #5 | 安全性 lint | `unwrap_used` / `expect_used` / `panic` warn，PR #5b/5c 待续 |
| #6 | config 拆分 | `AppConfig` 拆 6 sub-struct + `ConfigError` + `LazyLock` 单例 |
| #7-9 | AppError 迁移 | `app/ops/` 5 ops + 2 message fields → `AppResult<T>` |
| #10 | model 文档化 | 34 字段加 /// 业务含义；PO+DTO 同体说明 |
| #11 | db 职责收敛 | `DaoError` 根 + `tracing::instrument` + 6 测试 |
| #12 | crawler 架构图 | ASCII 流程图 + 拒绝"service/ 抽出"过抽象 |

**核心决策**：不颠覆业务、不改对外接口，仅做架构规整、错误体系统一、文档化、测试补齐。完整计划见 `docs/superpowers/plans/2026-07-08-so-novel-rs-refactor.md`。

## 🤝 贡献

欢迎 PR！本项目采用 AGPL-3.0 协议，贡献即同意按该协议授权。

* 提交前跑 `cargo fmt --all -- --check` + `cargo clippy --all-features --all-targets -- -D warnings` + `cargo test --lib`
* 新增 / 改动 UI 文案 → 同步 `locales/app.yml` 三语
* 新增书源 → 走 `bundle/rules/` JSON，规则语法见 `rule-template.json5`（如存在）
* 业务函数返回错误 → 用 `AppResult<T>` + `?` 透传；边界（CLI / Web）才转 `anyhow`

## 🙏 致谢

本项目基于 [freeok/so-novel](https://github.com/freeok/so-novel)（Java 版）重写为 Rust + GPUI 原生桌面客户端。感谢原作者的书源规则设计与核心架构思路。

## ⚠️ 免责声明

本项目是**技术工具**，仅供个人学习与研究使用。**严禁用于侵犯著作权、传播非法内容等任何违法用途**。详细条款见 [DISCLAIMER.md](./DISCLAIMER.md)。

## 📄 License

本项目基于 [AGPL-3.0](./LICENSE) 协议开源。
