# Changelog

## [0.5.1] - 2026-11-03

### Changed

- **GPUI 栈升级到 0.9.0**：`gpui-kit` 0.7.1 → 0.9.0（连带 `gpui-base` /
  `gpui-component` / `gpui-component-macros` / `gpui-kit-assets` 升至 0.9.0，
  `gpui-pre*` 快照升至 0.3.11）。上游 0.8/0.9 的破坏性改动包括 Theme 系列结构体
  改为 `#[non_exhaustive]`、`SelectionFormat` / `TabVariant` 新增变体、
  `gpui-wry` 改名 `gpui-webview`；本项目均未直接依赖这些写法，业务代码无需改动。
- **书源同步上游 v1.12.0**：以 [freeok/so-novel](https://github.com/freeok/so-novel)
  最新书源为基线更新全部 5 个书源集：
  - 域名更新：香书 `ibiqugu.net`、新天禧 `xtxxs9.com`、速读谷 `suduguu.com`；
  - 新增：笔趣屋（proxy-required）、笔趣阁中文网（rate-limit，原 `xbiquge.cc`）；
  - 归类调整：顶点小说移入 cloudflare；新笔趣阁info 更名为「新笔趣阁小说网」；
  - 删除上游已清理的失效书源：笔趣阁22、燃文小说网、老幺小说网；
  - 未导入「和图书」——上游版本把地址改写为本地 `127.0.0.1:8000`，与内置
    cf-bypass 机制重复。
  - 保留本项目引擎适配：3 处 `:contains(下一章)` 换成 scraper 可解析的等价
    选择器，书海阁过滤正则去掉 Rust regex 不支持的反向引用。
- **书源引擎支持 `@java:` 后处理**：实现上游 DSL 的 `@java:base64.decode()`
  （按行 trim → 过滤空行 → 逐行解码 → 拼接）。此前该后缀会被整段当作 JS 执行而
  报语法错误，少年小说网、新笔趣阁等正文无法解码；现在可直接使用上游原始规则。
- **代理设置改为三态「代理模式」**：`[proxy].enabled`（bool）→ `[proxy].mode`
  （`"none"` / `"manual"` / `"system"`）。新增「使用系统代理」——Windows 读 WinINET
  注册表（Clash / v2ray 的「系统代理」开关写的就是它），其它平台读 `HTTPS_PROXY` /
  `HTTP_PROXY`。探测不到时静默退化为直连，并在设置页显示原因（开关关闭 / 只有 PAC /
  只有 SOCKS / 环境变量未设 / 读取失败）。旧键在首次保存配置时自动迁移并删除，
  手动改过 `config.toml` 的用户无需任何操作。
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
