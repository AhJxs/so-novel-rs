<div align="center">

# so-novel-rs

**多源聚合小说搜索下载器 · Rust + GPUI 桌面客户端**

原生桌面应用，支持多源搜索、并发下载、简繁转换与多格式导出。

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.85+-orange.svg)](#)
[![Version](https://img.shields.io/badge/version-0.2.3-green.svg)](#)

</div>

---

## ✨ 功能

- **🔍 搜索下载** — 多源并发聚合搜索、相似度过滤排序、quanben5 加密搜索、详情面板、封面预览、选章下载
- **📥 下载任务** — 并发抓取、失败重试、进度跟踪、取消、封面嵌入、持久化、指定章节范围、**简繁自动转换**
- **📚 本地书库** — 扫描已下载书籍，按格式/日期/大小排序，删除二次确认（无空态闪烁）
- **🔌 书源管理** — JSON 导入、启用/禁用、连通性测速、URL 一键跳转
- **📄 多格式导出** — EPUB / TXT（多编码）/ HTML（zip 打包）/ **PDF**（DocumentBuilder 直接构建，CJK 字体嵌入）
- **🎨 主题系统** — 38 个可用主题，文件 watcher 热重载，无需重启
- **🌐 多语言** — 简体中文 / 繁体中文 / English，切换即时生效
- **💻 CLI 模式** — `search` / `download` / `sources` 子命令，`--json` 机器可读输出，与 GUI 共享同一套过滤排序逻辑
- **🔄 更新检查** — 检测 GitHub Release，有新版时一键跳转下载

## 🛠 技术栈

| 领域 | 选型 |
|------|------|
| GUI | GPUI 0.2 + gpui-component 0.5（Sidebar 导航、Settings、Dialog/Sheet/Notification） |
| 异步 | tokio 1（rt-multi-thread） |
| HTTP | reqwest 0.13（rustls，无 OpenSSL） |
| HTML 解析 | scraper 0.27 + regex |
| JS 引擎 | boa_engine（书源规则 `@js:` 后处理 + 加密） |
| 数据库 | rusqlite（bundled SQLite） |
| 配置 | toml_edit（保留注释与字段顺序） |
| 国际化 | rust-i18n（编译期嵌入） |
| 简繁转换 | zhconv（OpenCC + MediaWiki 词表，纯 Rust 无 FFI） |
| 导出 | epub-builder / zip / encoding_rs / pdf_oxide |
| 文件选择 | rfd `AsyncFileDialog` |

## 📂 项目结构

```
so-novel-rs/
├── bundle/                # 打包资源：字体 / 默认书源 / 主题 / JS 脚本
│   ├── fonts/             # Noto Sans SC 全字重
│   ├── rules/             # 默认书源 JSON（首次启动 seed 到数据库）
│   ├── themes/            # 内置主题（编译期嵌入）
│   └── web/               # JS 脚本 + 封面占位图
├── locales/               # i18n 翻译表（zh-CN / zh-HK / en）
└── src/
    ├── main.rs            # 入口
    ├── cli.rs             # clap CLI 子命令
    ├── app/               # 业务容器（与 GUI 解耦）
    │   ├── ops/           # download / search / sources / library / update / settings
    │   └── *_state.rs     # 搜索 / 书库 / 书源 / 更新 状态
    ├── config/            # config.toml 读写 + AppConfig
    ├── crawler/           # 搜索 / 下载（resolve_book + download_chapters）/ 重试 / 健康检测
    ├── db/                # SQLite 表（书源 / 覆写 / 下载任务）
    ├── export/            # EPUB / TXT / HTML / PDF 导出
    ├── gpui_app/          # GUI 层
    │   ├── root.rs        # RootView：TitleBar + Sidebar + 内容 + 覆盖层
    │   ├── themes.rs      # 主题系统（内置 + 用户目录热重载）
    │   ├── i18n.rs        # ts() 翻译函数
    │   ├── components/     # 通用组件（页头 / 空态 / 状态标签 / 分页）
    │   └── pages/         # 5 个一级页面：search / tasks / library / sources / settings
    ├── http/              # reqwest 封装 / 代理 / 编码 / CF 旁路
    ├── js/                # boa_engine 包装（书源 JS 后处理）
    ├── models/            # Rule / Book / Chapter / SearchResult
    ├── parser/            # DOM / 搜索 / 详情 / 目录 / 章节 / 过滤
    ├── rules/             # 从 DB 加载书源 + 用户覆写
    └── util/              # 文件名清洗 / 时间 / 语言检测 / 简繁转换
```

## 🚀 快速开始

```sh
cargo run
```

应用数据存放在 `~/.sonovel/`，首次启动自动创建：

| 路径 | 用途 |
|------|------|
| `config.toml` | 用户配置（保留注释） |
| `sonovel.db` | SQLite（书源 + 下载任务 + 覆写） |
| `themes/` | 用户主题目录（JSON，热重载） |
| `logs/` | tracing 日志（按天滚动） |

### CLI 用法

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

### 打包

```sh
cargo build --release                              # Windows（无控制台窗口）
cargo build --release --target x86_64-unknown-linux-gnu  # Linux
```

## ⌨️ 快捷键

| 快捷键 | 功能 |
|--------|------|
| `Cmd/Ctrl + 1..5` | 直跳页面 |
| `Cmd/Ctrl + B` | 折叠 / 展开 Sidebar |
| `F6` / `Shift+F6` | 翻页（避开 Input 的 Tab） |
| `Escape` | 关闭 Dialog / Sheet / Notification |

## 📄 License

本项目基于 [AGPL-3.0](./LICENSE) 协议开源。

---

# 法律免责声明 (Legal Disclaimer)

本项目 **So Novel** 是一款**技术工具**，其核心功能是实现**网页内容的结构化解析、数据提取与格式转换**，仅供**个人非商业性的学习、研究和技术交流目的**使用。

* **著作权与合法性义务：**
  用户在使用本工具时，应**严格遵守所有适用的国家、地区及国际法律法规**，包括但不限于著作权法、数据保护法等。**用户应自行承担因其使用本工具所抓取、处理、存储、使用、分发或传播的任何内容而产生的一切法律责任和风险**，包括但不限于侵犯他人著作权、隐私权、商业秘密或其他合法权益所引发的法律后果。本项目及其开发者不对用户因违法使用本工具而造成的任何损害或损失承担任何责任。

* **内容来源与责任：**
  本项目**不提供、不预设、不推荐任何特定的内容来源**。本工具的功能仅限于根据用户**自行配置的规则**对**公开网页信息**进行技术解析与格式转换。对于用户通过本工具所获取内容的**合法性、真实性、准确性、完整性、安全性及其著作权归属**，本项目及其开发者**不承担任何明示或暗示的保证或责任**。用户应对其所使用的内容来源的合法性进行独立判断和核实。

* **用途严格限制：**
  **严禁将本工具用于任何非法目的。** 这包括但不限于：抓取、下载、存储、传播任何受著作权法保护的、未经授权的、盗版的、非法淫秽的、煽动暴力仇恨的、侵犯他人隐私或商业秘密的，以及其他任何违反法律法规或社会公序良俗的内容。本项目开发者保留在发现或被告知任何非法用途时，采取适当措施的权利，包括但不限于停止提供服务、报告相关部门等。

* **风险自负原则：**
  在任何情况下，本项目及其开发者均不对因用户使用或无法使用本工具（包括但不限于因技术故障、网络中断、数据丢失、第三方内容源的变化、法律法规的调整等原因）而造成的任何直接、间接、附带、特殊、惩罚性或后果性损害（包括但不限于利润损失、数据损失、业务中断等）承担任何责任，即使已被告知此类损害发生的可能性。用户应充分理解并自行承担使用本工具可能带来的所有风险。

**请您务必理解并同意以上条款后再使用本工具。使用本工具即表示您已阅读、理解并同意遵守本免责声明的所有内容。**
