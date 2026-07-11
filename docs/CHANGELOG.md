# Changelog

## [0.3.6] - 2026-07-12

主题：**Markdown 导出 + URL 直接下载 + Tasks 删除 toast 反馈**（32 commits，
详见 [`CHANGELOG_ALL.md`](./CHANGELOG_ALL.md)）。

### Added

- **Markdown 导出格式**：`ExportFormat::Markdown` + `MdExporter`
  （YAML front matter + H1 书名 + TOC + 章节锚点）；desktop settings、
  web `FORMAT_OPTIONS`、CLI `--format markdown`、library `.md` 扩展名过滤按钮
  全链路打通
- **URL 直接下载**（SearchPage header「下载链接」按钮）：弹 Dialog 自动粘贴
  剪贴板 + 「粘贴」兜底按钮；按 URL origin / hash / port 匹配书源，复用
  `open_range_dialog` 走选章下载流程
- **Tasks 删除 toast 反馈**：`DeleteTaskResult` 枚举（`Ok` /
  `StillRunning` / `Missing`），`prompt_delete.on_ok` 按结果推
  success / warning toast
- **`match_source_by_url` 测试矩阵**：锁定 query / hash / port 三个易踩点的
  匹配敏感性，防止后续回归

### Changed

- **Library 移除 `notify` watcher**：改用手动「刷新」按钮 + 加载态，避免
  watcher 在打包后的 Windows 资源占用 + 资源刷新双触发问题
- **ExportFormat 三端对齐**：`Markdown` 在 desktop settings / web
  `FORMAT_OPTIONS` / CLI `--format` 一致暴露 `'md'`

### Fixed

- **Dialog-stack pop race**：URL 匹配成功后 `open_range_dialog` 通过 flag
  延迟到下一帧弹出，避免与正在关闭的 URL Dialog 栈冲突
- **`download_path` 默认值**：fallback `'./downloads'` 改带 `./` 前缀，
  与显式配置统一路径解析语义
- **Export 新代码 clippy nits** + md i18n key 补全 + 2 个缺失测试（review 反馈）
- **`mod tasks` 可见性**：提升到 `pub(crate)`，让 `DeleteTaskResult` 跨模块
  可见（之前需在 model 内重组才能导出）