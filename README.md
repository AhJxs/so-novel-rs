# so-novel-rs

So Novel 的 Rust + GPUI 桌面客户端，从 Java 版本完整重写。早期版本使用 egui，**Stage 11 已完成 egui → GPUI + gpui-component 全面迁移**。

## 功能概览

| 模块 | 状态 | 说明 |
|------|------|------|
| 搜索下载 | ✅ | 多源并发聚合搜索、相似度过滤排序、quanben5 加密搜索、详情面板、封面、选章下载 |
| 下载任务 | ✅ | 并发抓取、失败重试、进度跟踪、取消、封面嵌入、持久化、指定章节范围 |
| 本地书库 | ✅ | 扫描已下载书籍、按格式/日期/大小排序、删除二次确认 |
| 书源管理 | ✅ | 从 JSON 导入、启用/禁用、连通性测速、从数据库删除 |
| 设置 | ✅ | iOS 风格卡片式设置、主题切换持久化、4 色 toast 通知 |
| 导出 | ✅ | EPUB / TXT（多编码）/ HTML（zip 打包）；PDF 暂未实现 |
| CLI | ✅ | `search` / `download` / `sources` / `version` 四个子命令 |
| 配置 | ✅ | `~/.sonovel/config.toml`（toml_edit 保留注释）+ `~/.sonovel/sonovel.db`（SQLite） |

## 技术栈

- **GUI**: GPUI 0.2 + gpui-component 0.5（Stage 11 全面替换原 egui 0.34 / eframe 0.34 / egui_extras 0.34）
- **异步**: tokio 1（rt-multi-thread，leak 成 `&'static Runtime`）
- **HTTP**: reqwest 0.13（rustls，无 OpenSSL 依赖）
- **HTML 解析**: scraper 0.27 + regex 1
- **JS 引擎**: boa_engine 0.21（书源规则 `@js:` 后处理）
- **数据库**: rusqlite 0.40（bundled，书源 + 下载任务 + 用户覆写）
- **配置**: toml_edit 0.25（保留注释 + 字段顺序）
- **导出**: epub-builder 0.8 / zip 8 / encoding_rs 0.8
- **编码检测**: chardetng 1.0
- **文件选择**: rfd 0.15（原生对话框）
- **图标**: gpui-component 内置 IconName（Lucide 系列）
- **平台适配**: Windows 暗色窗口 / 无控制台窗口

## 项目结构

```
so-novel-rs/
├── ~/.sonovel/                  # 用户数据目录（首次启动自动生成）
│   ├── config.toml              # 用户配置
│   └── sonovel.db               # SQLite 数据库（书源 + 下载任务 + 覆写）
├── bundle/
│   ├── fonts/               # Noto Sans SC 全 9 字重
│   ├── rules/               # 默认书源 JSON（首次启动 seed 到数据库）
│   └── web/                 # JS 脚本 + 封面占位图
├── docs/                    # 迁移审计文档
└── src/
    ├── main.rs              # 入口
    ├── lib.rs               # 模块声明
    ├── app/                 # AppModel 顶层容器（Stage 2 重命名自 SoNovelApp）
    │   ├── mod.rs           # struct AppModel + 业务方法（不再 impl eframe::App）
    │   ├── download_task.rs # DownloadTask 模型
    │   ├── search_state.rs  # 搜索状态（含封面、详情缓存、TOC 预取）
    │   ├── library_state.rs # 本地书库状态
    │   ├── sources_state.rs # 书源测速状态
    │   ├── update_state.rs  # GitHub release 检查状态
    │   ├── cover.rs         # 封面字节解码 + URI 生成
    │   ├── toast.rs         # ToastKind 枚举
    │   ├── now.rs           # now_unix_secs
    │   ├── runtime.rs       # build_shared_runtime
    │   ├── tasks_db.rs      # load_tasks_from_db
    │   └── ops/             # 跨多个状态结构的业务方法
    │       ├── download.rs  # spawn_download / spawn_resolve_toc / spawn_download_range
    │       ├── search.rs    # spawn_search / select_search_result
    │       ├── sources.rs   # toggle/add/delete source / spawn_health_check
    │       ├── library.rs   # refresh_library / delete_library_entry
    │       ├── update.rs    # spawn_update_check
    │       └── settings.rs  # persist_settings
    ├── cli.rs               # clap CLI 子命令
    ├── config/loader.rs     # config.toml 读写 + AppConfig + ThemePref re-export
    ├── crawler/             # 搜索 / 下载（两阶段：resolve_book + download_chapters）/ 重试 / 健康检测
    ├── db/                  # SQLite 表（sources / source_overrides / download_tasks）
    ├── design_system/       # 配色 / 字体 / 公共 UI 组件
    │   ├── color.rs         # ACCENT + semantic 色函数
    │   ├── font.rs          # install_cjk_fonts / install_visuals
    │   ├── frame.rs         # nav / title_bar / content frame 工厂
    │   ├── button.rs        # primary/danger/success/warning/ghost/text/inline/icon 按钮
    │   ├── input.rs         # icon_text_input / rounded_combo / rounded_text_input / rounded_drag_value
    │   ├── popup.rs         # Popup 通用弹窗（自定义标题栏 + icon_button 关闭 + 尺寸控制）
    │   ├── chip.rs          # stat_chip / empty_state
    │   ├── toggle.rs        # iOS 风格 toggle_switch
    │   ├── settings.rs      # settings_row 通用布局
    │   └── theme_picker.rs  # ThemePref 枚举 + 段控选择器
    ├── export/              # EPUB / TXT / HTML / PDF(stub) 导出
    ├── http/                # reqwest 封装 / 代理 / 编码 / CF 旁路 / URL 拼接
    ├── js/                  # boa_engine 包装（书源 JS 后处理 + quanben5）
    ├── material_icons/      # Material Symbols 字体 vendor
    │   ├── mod.rs           # 字体加载 + 图标 API
    │   ├── icons.rs         # 生成的所有 ICON_* 常量
    │   └── *.ttf *.codepoints
    ├── models.rs            # Rule / Book / Chapter / SearchResult 等
    ├── parser/              # DOM / 搜索 / 详情 / 目录 / 章节 / 过滤 / 格式化
    ├── rules/               # 从 DB 加载书源 + 用户覆写
    ├── ui/
    │   ├── nav.rs           # 顶部水平导航（5 个页面 Tab + toast）
    │   ├── title_bar.rs     # 无装饰窗口标题栏 + 拖拽 + 窗口控制
    │   └── pages/
    │       ├── search.rs    # 搜索下载页
    │       ├── tasks.rs     # 下载任务页
    │       ├── library.rs   # 本地书库页
    │       ├── sources.rs   # 书源管理页
    │       └── settings.rs  # 设置页
    └── util/                # 文件名清洗 / 时间格式 / 语言检测 / 系统命令
```

## 设置页结构

设置页采用 iOS 风格卡片式布局，同一类设置放在一个卡片内，项间用分割线隔开：

| 卡片 | 包含设置项 |
|------|-----------|
| 外观 | 主题选择（浅色 / 跟随系统 / 深色，段控样式） |
| 下载 | 缓存保留、进度条、下载目录、默认格式、TXT 编码 |
| 书源 | 界面语言、搜索条数上限、相似度过滤 |
| 抓取 | 并发上限、请求间隔、重试开关、重试次数、重试间隔 |
| 代理 | 启用代理、Host、Port |
| Cookie | 起点 Cookie |
| 网络 | GitHub 代理、CF bypass URL |
| 关于 | 版本号、手动检查更新、项目主页 |

## Toast 系统

顶部状态栏临时消息支持 4 种类型，按颜色区分：

- **Info**（蓝 / ACCENT）— 通用提示
- **Success**（绿）— 保存成功、导入成功、删除成功
- **Warn**（橙）— 警告、新版本可用
- **Failed**（红）— 保存失败、解析失败、打开失败

调用方使用 `app.show_toast_*` 系列方法触发。检查更新完成后结果自动 toast 化。

## 运行

```sh
cargo run
```

工作目录建议在仓库根，使应用能找到 `bundle/` 下的字体和默认书源。配置和数据文件存放在 `~/.sonovel/` 目录，首次启动自动创建：

```sh
cd <repo-root>
cargo run --manifest-path so-novel-rs/Cargo.toml
```

### CLI 用法

不带子命令启动 GUI；带子命令走 CLI 模式：

```sh
# 搜索
so-novel-rs search "斗破苍穹"
so-novel-rs search "斗破苍穹" --source 1 --limit 10

# 下载
so-novel-rs download "https://example.com/book/123" --format epub

# 列出书源
so-novel-rs sources

# 版本
so-novel-rs version
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
cargo clippy           # 零警告
cargo test --lib       # 201 passed (3 ignored 为真实联网)
```

## 测试

```sh
cargo test --manifest-path so-novel-rs/Cargo.toml
```

当前 **201 个测试全通过**（3 个 ignored 为真实联网测试，需 `--ignored` 手动执行）。
