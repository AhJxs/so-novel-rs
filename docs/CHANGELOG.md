# Changelog

## [0.3.3] - 2026-06-25

### Added
- **CLI 模式正式可用**：`so-novel-rs search` / `download` / `sources` 三个子命令
  共享 GUI 的 parser / crawler / export。`-v, --verbose` 打开 tracing，
  `-q, --quiet` 抑制逐章 / 失败源 dump，脚本管道友好
- `sources list / enable <ID> / disable <ID>` 子命令；写回
  `~/.sonovel/sources_config.json`，跟 GUI / Web 共享同一文件
- `download --from <N> / --to <N>` 章节范围下载（1-based，闭区间；
  `to` 越界静默截断）
- TTY 原地进度行（搜索 / 下载统一走 `crate::util::tty::print_in_place_line`）
- Ctrl-C 接到 `CancelToken`，crawler 走 `Cancelled` 事件干净退出（不再硬杀进程）
- 搜索走 `search_streaming + tokio::spawn` 流式进度（参考
  `[[streaming-search-lesson]]` memory）

### Changed
- **`src/cli.rs` 549 行 → `src/cli/` 7 文件模块拆分**（参照
  `src/config/` 的子模块拆分模式）：`args` / `mod` / `search` /
  `download` / `sources` / `util` / `tests`
- `--help` / `--version` 全量中文化（`disable_help_flag` +
  `SetTrue` 手动分发，避开 clap 默认英文 help 文本；详见 `src/cli/args.rs`
  注释）
- 抽出 `crate::util::tty::print_in_place_line`（TTY 进度行 helper，
  从 `src/cli/helpers.rs` 提到 `src/util/tty.rs`，与 `formatting::truncate`
  等通用工具并列）
- `src/cli/helpers.rs` 改名 `src/cli/util.rs`（`util` 名跟 `src/util/`
  顶级工具目录呼应，区分"CLI 内部粘合层" vs "cross-cutting 工具"）
- TTY 原地进度行退出时补 `\n`，Windows 控制台 prompt 不再卡住要按 Enter
  （见 `src/cli/search.rs:154-160` 的 Windows console 坑说明）
- 搜索失败源 dump：≤3 条全打、>3 条压成 `top 3 + N more`；`--quiet` 完全跳过
- Web 模式默认 host 改回 `127.0.0.1`（loopback only）以匹配
  `so-novel-rs --web` 默认行为

### Fixed
- `fix(search)`: clear search state when rule set changes
- `chore(rules)`: update 梦书中文 domain to `mcxs.la`
- `feat(sources)`: make URL column a clickable Link
- Windows console prompt 在 in-place 进度模式退出后不再卡住（详见上方"Changed"）
- `fix(cli/download)`: 未传 `--source` 时按 URL 自动匹配书源（origin 比较），
  而非硬取第一个源；匹配不到再回退到首源
- `fix(cli/search)`: `--json` 模式不再在 stderr 打印搜索进度条，
  保持 stdout 仅含 JSON
- `fix(main)`: `attach_parent_console` 失败时回退 `AllocConsole`
  自行分配控制台，从 Explorer 启动 CLI/Web 模式不再无控制台可用
- `fix(Dockerfile)`: 运行阶段补装 GPUI 运行时共享库（`libxcb.so.1` 等），
  修复 `error while loading shared libraries` 启动失败

### Documentation
- 新增 [`docs/CLI.md`](./CLI.md)：CLI 完整用法（子命令参数、进度行为、
  JSON 输出、范围校验、常见工作流、注意事项、故障排查）
- 新增 [`docs/WEB.md`](./WEB.md)：Web 模式部署、API 端点、Docker
  多架构镜像、反向代理（SSE `proxy_buffering off` 等坑）、打包脚本
- 新增 [`docs/BOOK_SOURCES.md`](./BOOK_SOURCES.md)：6 套书源说明 +
  Cloudflare 绕过（`CloudflareBypassForScraping` 容器）+ 排查指引
- 新增 [`docs/CHANGELOG.md`](./CHANGELOG.md) + [`docs/CHANGELOG_ALL.md`](./CHANGELOG_ALL.md)
  两份变更日志；release workflow 用 `body_path: docs/CHANGELOG.md` 自动上传
  到 GitHub Release
- 新增 [`Dockerfile`](../../Dockerfile)（多阶段 + tini init + 非 root
  用户 `uid 1000` + data dir `/home/so-novel/.sonovel`）+ [`.dockerignore`](../../.dockerignore)
- 新增 [`.github/workflows/docker-release.yml`](../../.github/workflows/docker-release.yml)
  （BuildKit 多架构镜像推 `ghcr.io/ahjxs/so-novel-rs:<tag>` + `:latest`）