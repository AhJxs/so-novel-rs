# Changelog

## [0.4.0] - 2026-09-04

主题：**底层 UI 栈迁移 gpui-kit 0.6.0**（7 commits，经 PR #5 从
`fix/gpui-kit-0.6-migration` 合入，详见 [`CHANGELOG_ALL.md`](./CHANGELOG_ALL.md)）。

### Changed

- **UI 栈整体迁移**：`gpui + gpui-component 0.5.1` → `gpui-kit 0.6.0`
  （底层 gpui-pre 0.3.3）。适配破坏性 API：确认对话框 `Dialog` → `AlertDialog`
  （`.confirm()` / `.button_props()` / `.on_ok()` 语义保留）；多行输入
  `Input::multi_line` → `TextareaState` + `Textarea`；`SliderEvent` 新增 `Release`
  变体；`Progress::new()` 增加必填 `id`；`Sidebar` / `SidebarToggleButton`
  `left()` → `new()`；`update_entity` 返回 `R`；`IntoElement` `Component` →
  `ViewElement`
- **注释 / 文档术语统一**：gpui-component / GPUI 0.2.2 旧称呼 → gpui-kit 0.6 词汇
- **pdf_oxide** `0.3.73` → `0.3.77`（连带 office_oxide / fax / taffy / windows 重新解析）
- **web-ui 前端**：补齐 node_modules 依赖，tsc + vite 构建恢复

### Fixed

- **SidebarToggleButton 点击失效**：gpui-kit 0.6 起 Windows 上组件库 `TitleBar`
  把 children 行标成 `window_control_area(Drag)`，NCHITTEST 返回 HTCAPTION 让 OS
  按下即接管为拖窗 → click 丢失（hover 仍正常）；改用 `.occlude()` + mousedown
  `stop_propagation` 修复
- **鸟书网书源失效**：域名 `99xs.info` → `99wx.info`
