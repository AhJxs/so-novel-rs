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
| 🔌 **书源管理** | JSON 导入、启用/禁用、连通性测速、URL 一键跳转 |
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
| 💾 数据库 | rusqlite（bundled SQLite） |
| ⚙️ 配置 | [toml_edit](https://docs.rs/toml_edit)（保留注释与字段顺序） |
| 🌏 国际化 | [rust-i18n](https://docs.rs/rust-i18n)（编译期嵌入） |
| 🈶 简繁转换 | [zhconv](https://docs.rs/zhconv)（OpenCC + MediaWiki 词表，纯 Rust） |
| 📦 导出 | epub-builder / zip / encoding_rs / pdf_oxide |
| 📂 文件选择 | [rfd](https://docs.rs/rfd) `AsyncFileDialog` |

## 📂 项目结构

```
so-novel-rs/
├── assets/                # logo（png/svg/ico）
├── bundle/                # 打包资源：字体 / 默认书源 / 主题 / JS 脚本
│   ├── fonts/             # Noto Sans SC 全字重
│   ├── rules/             # 默认书源 JSON（首次启动 seed 到数据库）
│   ├── themes/            # 内置主题（编译期嵌入）
│   └── web/               # JS 脚本 + 封面占位图
├── locales/               # i18n 翻译表（zh-CN / zh-HK / en，编译期嵌入）
├── docs/                  # 用户文档（截图 / FAQ）
└── src/
    ├── main.rs            # 入口
    ├── cli.rs             # clap CLI 子命令
    ├── app/               # 业务容器（与 GUI 解耦）
    │   ├── ops/           # download / search / sources / library / update / settings
    │   └── *_state.rs     # 搜索 / 书库 / 书源 / 更新 状态
    ├── config/            # config.toml 读写 + AppConfig
    ├── crawler/           # 搜索 / 下载 / 重试 / 健康检测
    ├── db/                # SQLite 表（书源 / 覆写 / 下载任务）
    ├── export/            # EPUB / TXT / HTML / PDF 导出
    ├── gpui_app/          # GUI 层
    │   ├── root.rs        # RootView：TitleBar + Sidebar + 内容 + 覆盖层
    │   ├── themes.rs      # 主题系统（内置 + 用户目录热重载）
    │   ├── i18n.rs        # ts() 翻译函数
    │   ├── components/    # 通用组件（页头 / 空态 / 状态标签 / 分页）
    │   └── pages/         # 5 个一级页面：search / tasks / library / sources / settings
    ├── http/              # reqwest 封装 / 代理 / 编码 / CF 旁路
    ├── js/                # boa_engine 包装
    ├── models/            # Rule / Book / Chapter / SearchResult
    ├── parser/            # DOM / 搜索 / 详情 / 目录 / 章节 / 过滤
    ├── rules/             # 从 DB 加载书源 + 用户覆写
    └── util/              # 文件名清洗 / 时间 / 语言检测 / 简繁转换
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
| `sonovel.db` | SQLite（书源 + 下载任务 + 覆写） |
| `themes/` | 用户主题目录（JSON，热重载） |
| `logs/` | tracing 日志（按天滚动） |

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

## 🤝 贡献

欢迎 PR！本项目采用 AGPL-3.0 协议，贡献即同意按该协议授权。

* 提交前跑 `cargo fmt` + `cargo clippy --all-targets -- -D warnings` + `cargo test --lib`
* 新增 / 改动 UI 文案 → 同步 `locales/app.yml` 三语
* 新增书源 → 走 `bundle/rules/` JSON，规则语法见 `docs/rules.md`（如存在）

## 🙏 致谢

本项目基于 [freeok/so-novel](https://github.com/freeok/so-novel)（Java 版）重写为 Rust + GPUI 原生桌面客户端。感谢原作者的书源规则设计与核心架构思路。

## ⚠️ 免责声明

本项目是**技术工具**，仅供个人学习与研究使用。**严禁用于侵犯著作权、传播非法内容等任何违法用途**。详细条款见 [DISCLAIMER.md](./DISCLAIMER.md)。

## 📄 License

本项目基于 [AGPL-3.0](./LICENSE) 协议开源。
