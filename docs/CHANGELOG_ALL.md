# 完整变更日志

so-novel-rs 的**所有 git 提交**，按版本分组。最近版本与对外摘要见
[`CHANGELOG.md`](./CHANGELOG.md)。

> **说明**：
> - 早期版本（v0.2.6 之前）的发布以 `release: vX.Y.Z` 单独 commit + 之前若干
>   `chore: bundled changes` 收口，本表按"release commit → 上一 release commit"
>   的范围列出所有中间提交。
> - 同一 commit hash 在多个范围里都出现过是正常的（release commit 自身属于
>   上一个版本的尾）。
> - 短 hash（`%h`）便于在 `git show <hash>` 查详情；日期格式 `YYYY-MM-DD`。

---

## [Unreleased] (master)

> 等待下一个 release 的占位段。

## [0.3.3] - 2026-06-25

> 5 个 commit + 大量未单独提交的 CLI / docs / Docker 工作

- `94ceecd` 2026-06-25 — **feat(cli)**: add `--verbose` flag; defer tracing init; default web host to loopback
- `38b32ad` 2026-06-24 — **refactor(config)**: split loader into `defaults` / `paths` / `toml_io` / `types` / `tests`; add `ListCache`; cleanup
- `6fc0daa` 2026-06-24 — **feat(sources)**: make URL column a clickable Link
- `5d297be` 2026-06-24 — **chore(rules)**: update 梦书中文 domain to `mcxs.la`
- `3acc1da` 2026-06-24 — **fix(search)**: clear search state when rule set changes

> v0.3.3 期间还包含本仓库一系列 **CLI 重构 + UX 升级 + docs / Docker** 改动（未作为独立 commit 提交）：
> - `src/cli.rs` 549 行 → `src/cli/` 7 文件模块拆分
> - 新增 `search` / `download` / `sources` 子命令（详情见 [docs/CLI.md](./CLI.md)）
> - `--quiet` / `-q` 全局 flag + `SetTrue` 手动分发（中文 `--help` / `--version`）
> - TTY 原地进度行 + Ctrl-C 接 `CancelToken`（Windows console 退出 prompt 修复）
> - `download --from` / `--to` 章节范围下载
> - `sources enable <ID>` / `disable <ID>` 书源管理
> - 搜索走 `search_streaming` + `tokio::spawn` 流式进度（参考 `streaming-search-lesson`）
> - `docs/CLI.md` 完整 CLI 用法 + 故障排查
> - `docs/WEB.md` Web 模式 + Docker 部署（多架构镜像 + `proxy_buffering off` 等坑）
> - `docs/BOOK_SOURCES.md` 书源说明 + CF 绕过（`CloudflareBypassForScraping` 容器）
> - `docs/CHANGELOG.md` + `CHANGELOG_ALL.md` 两份变更日志
> - `Dockerfile`（多阶段 + tini + 非 root uid 1000）+ `.dockerignore`
> - `.github/workflows/docker-release.yml`（BuildKit 多架构推 `ghcr.io/...:latest`）
> - `src/util/tty.rs` 抽出 TTY 进度 helper；`src/cli/helpers.rs` 改名 `util.rs`

---

## [0.3.2] - 2026-06-24

> 4 个 commit（含 release 自身）

- `04fe52c` 2026-06-24 — **release**: v0.3.2
- `722f8e9` 2026-06-23 — **refactor**: simplify code across entire codebase (-151 lines)
- `0b3550c` 2026-06-22 — **refactor**: simplify code across parser, ops, and CLI modules
- `dd12cda` 2026-06-22 — **refactor(tracing)**: add TraceId chain tracing, clean up duplicate logs, remove file logging
- `f581f8a` 2026-06-22 — **refactor(crawler)**: box `Book` in `Progress::BookResolved` to shrink enum size

---

## [0.3.1] - 2026-06-23

> 1 个 commit

- `722f8e9` 2026-06-23 — **refactor**: simplify code across entire codebase (-151 lines)

> 注：v0.3.1 的 release commit 在 v0.3.2 历史里；上面 v0.3.2 的列表里 `722f8e9`
> 是该版本的 release 主体。

---

## [0.3.0] - 2026-06-22

> 0 个独立 commit（v0.3.0 的工作以 v0.2.6 → v0.3.0 范围内的 commit 为准，见下）

---

## [v0.2.6 → v0.3.0] 期间 (2026-06-19 ~ 2026-06-22)

- `a5d9d0f` 2026-06-21 — **feat(rule+parser)**: sync upstream Java changes & inlined quanben5 `@js:` handling
- `8f0341d` 2026-06-21 — **chore**: include bundle/rules in release artifacts and fix shuhaige backreference regex
- `206bc5b` 2026-06-21 — **chore**: update dependencies, docs, and add Dockerfile
- `906e25e` 2026-06-21 — **refactor(web)**: optimize code structure and config management
- `5ca0983` 2026-06-20 — **docs**: add acknowledgment to freeok/so-novel original project
- `4478f73` 2026-06-20 — **refactor**: P0+P1+P2 safety/code-quality optimizations
- `95d5690` 2026-06-20 — **fix(export)**: dedup final output filenames on collision
- `f53da17` 2026-06-20 — **refactor(logging)**: extract tracing init to `src/logging.rs`
- `5bc6d66` 2026-06-20 — **fix(app)**: Phase 2 quality fixes bundled
- `2e7036d` 2026-06-20 — **docs**: Phase 2+3+4 final summary + audit status (Phase 5)
- `92a7133` 2026-06-20 — **privacy**: truncate search keyword in logs (Phase 4.3)
- `71c93fb` 2026-06-20 — **tracing**: add chapter-level tracing to export + pagination (Phase 4.2)
- `24b9962` 2026-06-20 — **test(parser,export)**: add missing unit tests (Phase 4.1)
- `3094d6c` 2026-06-20 — **perf(crawler)**: cancel token uses Notify for <1ms response (Phase 3.6)
- `d54f576` 2026-06-20 — **perf(export)**: BufWriter for epub/pdf writes + filename dedup (Phase 3.3)
- `8c6db6d` 2026-06-20 — **perf(parser)**: cache Regex / Selector compilation per string (Phase 3.2)
- `be76b9e` 2026-06-20 — **perf(http)**: 跨任务复用 reqwest client 连接池 + TLS session cache (Phase 3.1)
- `35cb0f7` 2026-06-20 — **docs(tasks)**: mark Phase 2 checklist complete + write `review.md`
- `bc410c1` 2026-06-20 — **fix(app)**: surface tokio runtime build failures via Result
- `ee78f4b` 2026-06-20 — **feat(log)**: purge old log files (>30d) at startup
- `9c10c4c` 2026-06-20 — **perf(db)**: single-statement `delete_finished` via `json_extract`
- `418f79c` 2026-06-20 — **perf(db)**: wrap `seed_from_default` inserts in a single transaction
- `196d413` 2026-06-19 — **chore**: bundled changes for v0.2.6
- `242fd6e` 2026-06-19 — **refactor(pages)**: 抽 `SearchDelegate` + `LibraryDelegate` 到独立 `delegate.rs` (PR6)
- `b0cd230` 2026-06-19 — **refactor(sources)**: split `sources.rs` 688 → 5 files (PR4)
- `50ac993` 2026-06-19 — **refactor(tasks)**: split `tasks.rs` 824 → 6 files (PR3)
- `65d9ba0` 2026-06-19 — **refactor(library)**: split `library.rs` 832 → 5 files (PR2)
- `314250b` 2026-06-19 — **refactor(search)**: split `search.rs` 1517 → 7 files (PR1)
- `24e0803` 2026-06-19 — **refactor**: 提取 `format_local_unix_secs` + `SourceHealth::classify`（PR0 helpers）

---

## [0.2.5] - 2026-06-19

> 6 个 commit

- `da28758` 2026-06-19 — **release**: v0.2.5
- `477320d` 2026-06-19 — **chore**: bundled changes (assets + library/search/i18n/config + deps)
- `809a547` 2026-06-19 — **docs**: rewrite README + split DISCLAIMER + auto-version badge
- `ee00235` 2026-06-19 — **feat(gpui_app)**: swap sidebar and titlebar background colors
- `5245010` 2026-06-19 — **feat(sources)**: show latency in ms instead of raw HTTP status
- `536266e` 2026-06-19 — **refactor(gpui_app)**: split settings page monolith into `settings/` module

---

## [0.2.4] - 2026-06-18

> 1 个 commit

- `5535773` 2026-06-18 — **release**: v0.2.4 — PDF 导出重写 + AGPL-3.0 协议 + README 现代化
- `3f4866a` 2026-06-18 — **feat**: 简繁中文自动转换 + URL 链接 + 更新按钮 + 书库删除无闪

---

## [0.2.3] - 2026-06-18

> 0 个独立 commit（变更随 v0.2.4 release 一起发布）

---

## [0.2.2] - 2026-06-18

- `4ff3e02` 2026-06-18 — **release**: v0.2.3
- `b937724` 2026-06-18 — **ci(release)**: Linux aarch64 改用 `ubuntu-24.04-arm` 原生 runner
- `3fc5cd1` 2026-06-18 — **release**: v0.2.2
- `53858ae` 2026-06-18 — **chore(deps)**: Cargo.lock 同步 rfd async-std feature 变化
- `0b8b44c` 2026-06-18 — **fix(deps)**: rfd 0.15 切到 async-std feature 修 Linux build 7 个 ashpd 编译错误
- `a0b422e` 2026-06-18 — **ci(release)**: Linux aarch64 改用 `ubuntu-24.04-arm` 原生 runner

---

## [0.2.1] - 2026-06-18

- `be8af8a` 2026-06-18 — **release**: 0.2.1
- `336581f` 2026-06-18 — **ci(release)**: 删 Cross.toml + 修 Linux vulkan 包 + 去掉 macOS x86

---

## [0.2.0] - 2026-06-18

> 跨多个工作日的大版本：多平台打包 + UI 改版 + i18n 全面化 + 日志系统

- `e18e6b2` 2026-06-18 — **release**: 0.2.0 多平台打包 + UI 改版 + i18n 全面化 + 日志系统
- `2b26ee6` 2026-06-17 — **feat(ui)**: 书源/搜索页 Tag 化改造 + 搜索结果行重排 + Enter 搜索
- `17268d8` 2026-06-17 — **feat(ui)**: 持久化 Sidebar 折叠状态
- `43ecc83` 2026-06-17 — **refactor(notification)**: toast 字段 + 4 个 `show_toast_*` 替换为 gpui-component Notification

---

## 早期版本

> 以下 commit 在 v0.2.0 之前，跨多个内部里程碑（"下载弹窗 + 选章"、
> "设计系统统一"、"书源数据库迁移"、"依赖升级"等），保留作为完整历史。

- `3af6aed` 2026-06-16 — **feat(download)**: 选章下载 + 下载弹窗 + crawler 两阶段拆分
- `a2a26a9` 2026-06-15 — **refactor(design_system)**: 统一按钮/输入框/弹窗组件 + 迁移配置到 `~/.sonovel/`
- `3da0a23` 2026-06-15 — **feat(search)**: 流式搜索进度 + **ui(toggle)**: 增强 iOS 风格开关
- `c4e3ac6` 2026-06-15 — **refactor**: 拆分 crates 与 `app.rs`
- `1e9e121` 2026-06-14 — **ui(sources)**: 卡片化 + 测速/添加按钮 + 删除（含二次确认）
- `3176ea4` 2026-06-14 — **ui(library)**: 卡片化列表 + 时间对齐到书名下方
- `84e5bb2` 2026-06-14 — **feat**: `config.toml` + 书源入 `sonovel.db` + Linux 打包工具链
- `8b21c67` 2026-06-14 — **deps**: 升级一批依赖大版本并修复 API 变更
- `dba9299` 2026-06-14 — **ui**: 集成 `material_icons` 并替换 emoji/手绘图标
- `6bdf1f4` 2026-06-14 — **ui**: 优化搜索/导航交互与样式
- `015edc0` 2026-06-18 — **fix(fs)**: Linux `sanitize_filename` 保留扩展名点并清理反斜杠

---

## 元数据

- **总提交数**：72（含 release commit）
- **首个 commit**：早于 2026-06-14（项目从 `freeok/so-novel` Java 移植启动）
- **当前版本**：`v0.3.3`（Cargo.toml + git tag）
- **CHANGELOG 规范**：[Keep a Changelog 1.1.0](https://keepachangelog.com/zh-CN/1.1.0/)
- **版本号规范**：[Semantic Versioning 2.0](https://semver.org/lang/zh-CN/)
- **commit 类型**：`feat` / `fix` / `refactor` / `perf` / `test` / `docs` /
  `chore` / `ci` / `ui` / `release`（前 6 个走 Conventional Commits）

## 进一步阅读

- [CHANGELOG.md](./CHANGELOG.md) — 对外摘要
- [CLI.md](./CLI.md) — CLI 用法
- [README.md](../README.md) — 项目总览
