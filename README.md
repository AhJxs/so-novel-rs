# so-novel-rs

So Novel 的 Rust + GPUI 桌面客户端，从 Java 版本完整重写。GUI 栈从早期版本的 egui 0.34 迁移到 **GPUI 0.2 + gpui-component 0.5** —— sidebar 导航、gpui-component `Settings` / `Select` / `Input` 等组件、热重载主题系统、rust-i18n 多语言、SidebarCollapsible、Dialog / Sheet / Notification 覆盖层一应俱全。

## 功能概览

| 模块 | 状态 | 说明 |
|------|------|------|
| 搜索下载 | ✅ | 多源并发聚合搜索、相似度过滤排序、quanben5 加密搜索、详情面板、封面、选章下载 |
| 下载任务 | ✅ | 并发抓取、失败重试（retry-min/max 真正生效）、进度跟踪、取消、封面嵌入、持久化、指定章节范围、下载内容**简繁自动转换** |
| 本地书库 | ✅ | 扫描已下载书籍、按格式/日期/大小排序、删除二次确认（外科式移除 + watcher 抑制，无空态闪烁） |
| 书源管理 | ✅ | 从 JSON 导入（原生对话框）、启用/禁用、连通性测速、从数据库删除、URL 可点击跳浏览器 |
| 设置 | ✅ | gpui-component `Settings` 组件 4-page 布局；改任一字段立即落盘；检查更新按钮在有新版时切换成"下载新版"跳浏览器 |
| 导出 | ✅ | EPUB / TXT（多编码）/ HTML（zip 打包）；PDF 暂未实现；含**简繁中文自动转换** |
| 主题系统 | ✅ | 38 个可用主题；embed 21 个 + 用户 `~/.sonovel/themes/`；文件 watcher 热重载 |
| 多语言 | ✅ | zh-CN / zh-HK / en 三语；切换即时生效（locale 全局 + Settings id 重建） |
| CLI | ✅ | `search` / `download` / `sources` 三个子命令；`--json` 机器可读输出；结果与 GUI 一致（共享 `filter_sort`） |
| 更新检查 | ✅ | 检测 GitHub release，有新版时按钮变"下载新版 vX.Y.Z"（ExternalLink 图标） |
| 配置 | ✅ | `~/.sonovel/config.toml`（toml_edit 保留注释）+ `~/.sonovel/sonovel.db`（SQLite） |

## 技术栈

- **GUI**: GPUI 0.2.2 + gpui-component 0.5.1（替代早期 egui 0.34 / eframe）
- **异步**: tokio 1（rt-multi-thread，leak 成 `&'static Runtime`）
- **HTTP**: reqwest 0.13（rustls，无 OpenSSL 依赖）
- **HTML 解析**: scraper 0.27 + regex 1
- **JS 引擎**: boa_engine 0.21（书源规则 `@js:` 后处理 + quanben5 加密）
- **数据库**: rusqlite 0.40（bundled，书源 + 下载任务 + 用户覆写）
- **配置**: toml_edit 0.25（保留注释 + 字段顺序）
- **国际化**: rust-i18n 3（locales/app.yml 编译期嵌入，与 gpui-component 共享全局 locale）
- **导出**: epub-builder 0.8 / zip 8 / encoding_rs 0.8
- **编码检测**: chardetng 1.0
- **简繁转换**: zhconv 0.4（OpenCC + MediaWiki 词表，Aho-Corasick 匹配；纯 Rust 嵌入词表，无 FFI 依赖）
- **文件选择**: rfd 0.15 + `AsyncFileDialog`（**必须用 async API**，见下文踩坑说明）
- **图标**: gpui-component 内置 IconName（Lucide 系列）
- **平台适配**: Windows 暗色窗口 / 无控制台窗口

## 项目结构

```
so-novel-rs/
├── ~/.sonovel/                  # 用户数据目录（首次启动自动生成）
│   ├── config.toml              # 用户配置（toml_edit 保留注释）
│   ├── sonovel.db               # SQLite（书源 + 下载任务 + 覆写）
│   ├── themes/                  # 用户主题目录（JSON 文件，可热重载）
│   └── logs/                    # tracing 日志（按天滚动 `<日期>.log`，见「日志」一节）
├── bundle/
│   ├── fonts/                   # Noto Sans SC 全 9 字重
│   ├── rules/                   # 默认书源 JSON（首次启动 seed 到数据库）
│   ├── themes/                  # embed 主题（21 个 .json，编译期嵌入）
│   └── web/                     # JS 脚本 + 封面占位图
├── locales/
│   └── app.yml                  # 自有 i18n 翻译表（zh-CN / zh-HK / en）
└── src/
    ├── main.rs                  # 入口（console attach + tracing init）
    ├── lib.rs                   # 模块声明 + `rust_i18n::i18n!("locales")` 初始化
    ├── cli.rs                   # clap CLI 子命令
    ├── app/                     # 业务容器（GUI 解耦）
    │   ├── mod.rs               # struct AppModel + 业务方法
    │   ├── download_task.rs     # DownloadTask 模型
    │   ├── search_state.rs      # 搜索状态（封面 / 详情缓存 / TOC 预取）
    │   ├── library_state.rs     # 本地书库状态
    │   ├── sources_state.rs     # 书源测速状态
    │   ├── update_state.rs      # GitHub release 检查状态
    │   ├── cover.rs             # 封面字节解码 + URI 生成
    │   ├── now.rs               # now_unix_secs
    │   ├── runtime.rs           # build_shared_runtime
    │   ├── tasks_db.rs          # load_tasks_from_db
    │   ├── events.rs            # 跨页面事件
    │   └── ops/                 # 跨多个状态结构的业务方法
    │       ├── download.rs      # spawn_download / spawn_resolve_toc / spawn_download_range
    │       ├── search.rs        # spawn_search / select_search_result
    │       ├── sources.rs       # toggle/add/delete source / spawn_health_check
    │       ├── library.rs       # refresh_library / delete_library_entry
    │       ├── update.rs        # spawn_update_check
    │       └── settings.rs      # persist_settings
    ├── config/                  # config.toml 读写 + AppConfig / ThemePref
    ├── crawler/                 # 搜索 / 下载（两阶段：resolve_book + download_chapters）/ 重试 / 健康检测
    ├── db/                      # SQLite 表（sources / source_overrides / download_tasks）
    ├── export/                  # EPUB / TXT / HTML / PDF(stub) 导出
    ├── gpui_app/                # 新 GUI（替代旧 src/ui + src/design_system）
    │   ├── mod.rs               # 启动 + 全局初始化（themes / i18n / asset 注册）
    │   ├── root.rs              # RootView：TitleBar + Sidebar + 内容 + dialog/sheet/notification 层
    │   ├── themes.rs            # 主题系统：embed 21 + 用户目录热重载 + ThemeRegistry observer
    │   ├── i18n.rs              # `ts(key)` 函数（rust-i18n 后端 + SharedString 转换）
    │   ├── components/          # 跨页通用组件
    │   │   ├── page_header.rs   # 标题 + 副标题 + 右侧 actions
    │   │   ├── empty_state.rs   # 空态占位
    │   │   ├── status_badge.rs  # 状态标签（成功/失败/警告）
    │   │   ├── pagination.rs    # 分页页脚 + `compute_page_window` 公共 helper
    │   │   └── formatting.rs    # 文件大小 / 时间 / 数量格式化
    │   └── pages/               # 5 个一级页面（NavPage）
    │       ├── search.rs        # 搜索下载
    │       ├── tasks.rs         # 下载任务
    │       ├── library.rs       # 本地书库
    │       ├── sources.rs       # 书源管理
    │       └── settings.rs      # 设置（gpui-component Settings）
    ├── http/                    # reqwest 封装 / 代理 / 编码 / CF 旁路 / URL 拼接
    ├── js/                      # boa_engine 包装（书源 JS 后处理 + quanben5）
    ├── models/                  # Rule / Book / Chapter / SearchResult / SourceInfo / ContentType
    ├── parser/                  # DOM / 搜索 / 详情 / 目录 / 章节 / 过滤 / 格式化
    ├── rules/                   # 从 DB 加载书源 + 用户覆写
    └── util/                    # 文件名清洗 / 时间格式 / 语言检测 / 系统命令 / 简繁转换
```

## Sidebar 导航（Stage 4+）

`RootView` 用 gpui-component `Sidebar::left()` 搭左侧 5 项菜单：

- **Search** / **Tasks** / **Library** / **Sources** / **Settings**
- 可折叠：`SidebarCollapsible::Icon` —— `Cmd+B` 折叠到 48px 图标宽度，200ms `ease_in_out_cubic` 缓动
- `SidebarToggleButton` 放在 TitleBar 最左侧
- 5 个 page entity 一次性创建，跨切换保留内部状态（输入框 / 滚动位置）

快捷键：`Cmd+1..5` 直跳页面、`F6` / `Shift+F6` 翻页（避开 Input 的 Tab 绑定）、`Cmd+B` 折叠 sidebar、`Escape` 关 dialog / sheet / notification。

## 设置页结构（4 page）

`settings.rs` 用 gpui-component `Settings` 组件搭 4 个 `SettingPage` + 内部 `SettingGroup` + `SettingItem`：

| Page | Group | Item |
|------|-------|------|
| **常规** | 外观 | 主题（下拉，38 个候选项）/ 应用语言 |
|  | 网络 | GitHub 代理 / Cloudflare bypass URL |
|  | 下载 | 下载目录（Input + suffix 图标 → rfd `AsyncFileDialog`）/ 默认格式 / TXT 编码 / 保留章节缓存 / 启用下载进度条 |
|  | 书源 | 书源语言 / 搜索条数上限 / 相似度过滤 |
| **抓取** | 并发与间隔 | 并发上限 / 请求间隔 min/max |
|  | 重试 | 启用重试 / 最大重试次数 / 重试间隔 min/max（`retry-min/max` 真正生效） |
| **代理** | HTTP 代理 | 启用 / Host / Port |
|  | Cookie | 起点 Cookie |
| **关于** | 信息 | 版本号 / 检查更新（按钮在有新版时变"下载新版"）/ 项目主页 |

任一字段改动 → `model.update → persist_settings()` 立即落盘，**无「保存」按钮**。

## 主题系统

主题列表由 `gpui_app/themes.rs` 维护：

- **embed 主题**（21 个）：编译期嵌入二进制，跨平台必有
- **用户主题**（动态）：扫描 `~/.sonovel/themes/*.json`，文件 watcher 监听变更 → 自动 reload → `Theme` observer → `cx.refresh_windows()` → 整 app 重新 render

`SettingsPage::sync_theme_items` 在每次 render 时重新拍快照 `themes::list_theme_names(cx)`，跟上次缓存对比：

- 没变 → 0 开销直接返回
- 变了 → `theme_state.update` 调 `set_items` + `set_selected_index` 把列表推到 `SelectState`

所以用户装了新主题 / 改了主题文件，**下拉框自动出现新选项，无需重启**。

## 国际化（i18n）

双套 i18n 实例，**全局 locale 共享**：

- **本 crate**：`rust_i18n::i18n!("locales")` 加载 `locales/app.yml`，自有 key（Nav.* / App.* / Settings.*）
- **gpui-component**：自带 `locales/ui.yml`，管框架内置文案

`gpui_component::set_locale(lang)` 写到全局 `CURRENT_LOCALE`，**一次设置两边同时生效**。

3 种语言：`zh-CN` / `zh-HK` / `en`，切语言后 Settings 页 id 重建 → `SettingsState` 重建 → 内部 `InputState.placeholder` 重新求值（这是为了修复「只刷全局 locale 不刷 InputState 占位符」的坑）。

UI 中调 `ts!("Settings.item.theme")` 拿当前 locale 翻译；缺翻译时 fallback 返回 key 字符串本身（开发期可见漏翻译）。

## 日志

`tracing` 双层输出：

- **stdout**：开发期直接看 trace；用 `EnvFilter`（`RUST_LOG` 环境变量可覆盖，默认 `info,so_novel_rs=debug`）。
- **文件**：按天滚动到 `~/.sonovel/logs/<YYYY-MM-DD>.log`，文件名带 `tracing_appender::rolling::daily` 的日期后缀。

文件 layer 启动失败（如权限不足）静默退化为只有 stdout，不阻塞主程序。

Retention 由用户 / 部署环境自管（`tracing_appender::rolling::daily` 不自带 retention，手写清理容易在日期边界 / 文件锁上踩坑；推荐 `cron` / `logrotate` 或自己定期删）。

## 文件选择器（rfd / Windows 踩坑）

GUI 里两处用 `rfd` 弹原生对话框：

- `SettingsPage::pick_folder` —— 下载目录「浏览」按钮
- `SourcesPage::pick_and_add` —— 书源管理「添加」按钮（导入 JSON）

**必须用 `rfd::AsyncFileDialog`，不能用同步 `rfd::FileDialog`**。

原因：Windows 上 `IFileOpenDialog::Show()` 是 COM STA UI API，需要当前线程 `CoInitializeEx(COINIT_APARTMENTTHREADED)` + 有 message pump。GPUI 的 `cx.background_executor().spawn(...)` 把任务丢到 tokio worker thread pool —— 这些 thread 既没初始化 COM apartment，也没 message pump，`Show()` 静默失败立即返回 `None`（**实测 69µs 内返回**，正常用户取消至少秒级），dialog 不显示。

`AsyncFileDialog` 内部走 `tokio::task::spawn_blocking`，在 tokio 专门的 blocking thread pool 上跑，COM 初始化走对路。`Cargo.toml` 已 enable `tokio` feature。

## 运行

```sh
cargo run
```

工作目录建议在仓库根，使应用能找到 `bundle/` 下的字体和默认书源。配置和数据文件存放在 `~/.sonovel/` 目录，首次启动自动创建。

### CLI 用法

不带子命令启动 GUI；带子命令走 CLI 模式：

```sh
# 搜索（结果与 GUI 一致：自动按 config.search_filter 过滤 + 排序）
so-novel-rs search "斗破苍穹"
so-novel-rs search "斗破苍穹" --source 1 --limit 10
so-novel-rs search "斗破苍穹" --json | jq length   # 机器可读输出

# 下载
so-novel-rs download "https://example.com/book/123" --format epub
so-novel-rs download "https://example.com/book/123" --output D:\novels --format txt

# 列出书源（--json 输出 rules 数组）
so-novel-rs sources
so-novel-rs sources --json

# 版本（clap 自动注入的 -V/--version flag，version 子命令已移除）
so-novel-rs --version
```

### 打包

```sh
# Windows（无控制台窗口）
cargo build --release

# Linux
cargo build --release --target x86_64-unknown-linux-gnu
```

## 代码质量

```sh
cargo clippy --all-targets -- -D warnings   # 零警告（CI 严格模式）
cargo test --lib                          # 234 passed (3 ignored 为真实联网)
```

## 测试

```sh
cargo test
```

当前 **234 个测试全通过**（3 个 ignored 为真实联网测试，需 `--ignored` 手动执行）。

## 简繁中文转换

下载章节时若 `Rule.language`（书源自带语言标记）与 `config.language`（用户目标语言）不同，自动把章节正文做简繁转换：

- TXT：zhconv 整串转换（含台湾用词差异，如"软件"→"軟體"）
- HTML / EPUB：跳过 `<script>` / `<style>` 块，其它内容（标签外文本、属性值）走 zhconv（ASCII 字符不会被改 → 标签结构稳定）
- PDF：阶段 1 降级为 HTML 模板，照走转换

`Settings → 书源语言` 即决定目标语言。

底层用 [zhconv 0.4](https://crates.io/crates/zhconv)（OpenCC + MediaWiki 词表合并，Aho-Corasick 匹配，编译期嵌入数据），纯 Rust 无 FFI 依赖。注意：`t2s` 是字面繁→简（如"軟體"→"软体"），不会反向做台湾→大陆用词映射。

## 更新检查

`Settings → 关于 → 检查更新` 按钮：
- 未检查 / 正在检查：spinner + "检查 GitHub 最新版本"
- 检查完成且有新版本：ExternalLink 图标 + `下载新版 vX.Y.Z`（带实际版本号），点击跳 `https://github.com/AhJxs/so-novel-rs/releases/latest`

底层 `update_state.rs` 用 `serde_json` 解析 GitHub API 响应（旧版按行匹配 pretty-print 会在压缩 JSON 下误报"(empty result)"）。

## 本地书库删除

`delete_library_entry` 外科式从 `entries` 中 `retain` 掉被删条目（不做全量 rescan），同时设 `watcher_skip_until_unix_ms = now + 1000ms` 抑制 watcher 在 1s 内触发 rescan。效果：删除时 UI 立即少一行，无空态闪烁。1s 后窗口过期，新文件添加等正常 fs 事件仍会触发 rescan。