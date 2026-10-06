# Changelog

## [Unreleased]

### Changed

- **GPUI 栈升级**：`gpui-kit` 0.7.0 → 0.7.1（连带 `gpui-base` / `gpui-component` /
  `gpui-component-macros` / `gpui-kit-assets` 升至 0.7.1，`gpui-pre*` 快照升至 0.3.8）。
  补丁级更新，无 API 变更，业务代码零改动。

### Removed

- **孤儿代码**：删除 `core::download_task` 的 `DownloadTask::apply_to_task` 及其单元测试
  （原唯一调用方已随 Web 端移除）。

### Fixed

- **文档一致性**：修正 `README.md` 的书源切换教程（删除不存在的 `config.toml`
  `active-rules` 字段说明）、`docs/BOOK_SOURCES.md` 的源码路径与书源文件数量、
  `AGENTS.md` 的悬空引用；清理 `src/` 内残留的 CLI / Web / feature 字样注释，
  并把 `ExportFormat::Pdf` 的"暂不实现"注释改为与实际一致。

## [0.5.0] - 2026-10-04

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
  `find_rule_by_id_cloned` / `find_rule_by_url`；`core::config_helpers` 删
  `validate_download_path`；`i18n` 删 `WEB_ERROR_KEYS` 常量与 2 个测试；连带删除
  共 23 个单元测试
- **CLI 端整体移除**：删 `src/cli/`（6 文件）、`src/startup/`、`src/utils/tty.rs`、
  `docs/CLI.md`；`src/main.rs` 内联原 `startup/` 启动逻辑；`locales/app.yml` 删 `Cli:` 段
- **依赖清理**：删 `clap` / `windows-sys`，tokio 摘除 `signal` feature
- **顶层目录重排**：`bundle/rules/` → `assets/rules/`、`bundle/web/` →
  `tests/fixtures/web/`、`screenshots/` → `docs/screenshots/`，`bundle/` 目录消失

仓库由此收敛为 **GUI** 单一形态。`cargo test --lib` 由 510 passed 降至 439 passed。
