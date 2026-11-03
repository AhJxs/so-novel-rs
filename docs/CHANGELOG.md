# Changelog

## [0.5.2] - 2026-11-03

### Fixed

- **macOS 退出行为**：单窗口应用没有后台 / 托盘模式，但 GPUI 默认在最后一个窗口
  关闭后仍让进程驻留。现改为 `QuitMode::LastWindowClosed`，关闭主窗口即退出。
- **应用级退出动作**：新增 `Quit` action 与 `cmd-q` 绑定，无论当前焦点在哪个视图
  都能退出；macOS 菜单栏新增本地化的「退出 So Novel」项（en / zh-CN / zh-TW）。
- **启动失败时真正退出**：`open_window` 失败或启动期致命错误弹窗后，补上
  `cx.quit()`，不再留下无窗口的空进程。
