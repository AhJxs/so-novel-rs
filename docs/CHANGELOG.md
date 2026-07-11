# Changelog

## [0.3.5] - 2026-07-11

主题：**模块重组 + 业务层抽离 + Web 多语言适配**（49 commits，详见
[`CHANGELOG_ALL.md`](./CHANGELOG_ALL.md)）。

### Added

- **Web API 多语言**：错误响应按 per-request locale 翻译
  （Accept-Language → AppConfig.language → `en` fallback），
  全局无 mutation；新增稳定数字错误码表（38 个，1xxx-5xxx），
  前端 dispatch 由 substring 匹配改为按 `codeId` 数字码
- **AppError / DaoError 根类型 + AppResult 别名**：业务层错误统一形态；
  `AppConfig` 拆 6 个 sub-struct + validate + singleton
- **`/api/health` JSON 端点** + Docker Compose 编排
- **Crawler 性能**：章节边下边写（消除批量 syscall）+ `tokio::sync::Notify`
  立即唤醒 drain_loop（TOC / 搜索 / 详情 / 更新检查）
- **Observability**：关键 public fn 加 `tracing::instrument` + rust-doc
- **Pedantic lint 套件**（`unwrap_used` / `expect_used` / `panic` 强制 justify）

### Changed

- **重大模块重组**：
  - `gpui_app` → `desktop`，`app` → `desktop::model`，
    `download_task` → `core::download_task`，`util` → `utils`，`persistent` → `db`
- **`core/` 业务层抽离**（Phase 3.0–3.9，10 个子阶段）：search / sources /
  bootstrap / download_task / library / update / async_progress / web helpers /
  cli runtime 等与 GUI 解耦，三端共享
- **大文件按职责拆分**：所有 400+ LOC 的 monolithic 文件拆成 focus 子文件
  （crawler / export / parser / web / desktop 等 14 处）
- **Web SSE 本地化**：所有 error / reason 字段从硬编码中文 / `format!("{e:#}")`
  改为按 locale 翻译的稳定文案；3 个新错误码变体
- **`zh-HK` → `zh-TW` 全量重命名**：消除 locale tag 漂移
- **Logger 默认切回 Text**（JSON 通过 `RUST_LOG_FORMAT=json` 启用）
- **`README.md` 项目结构精简**

### Fixed

- **Web 错误响应泄漏内部 cause**（anyhow / thiserror 完整链曾被拼进 500 body）
- **起点站 cookie 通过 `GET /api/settings` 泄漏**（改返脱敏 `PublicSettings`）
- **SSE 错误事件泄漏内部错误细节**（统一走 ErrorCode 翻译）
- **前端把后端 envelope 当 opaque 文本**（改为解析 `code / codeId / message`）
- **346 个 build warning + 0 个 clippy warning under `-D warnings`**
