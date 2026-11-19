# Changelog

## [0.7.0] - 2026-11-19

### Added

- **工具箱**：本地书稿格式转换（TXT / Markdown / HTML / EPUB 读入，可导出 TXT / Markdown /
  HTML / EPUB / ZIP）、TXT 编码转换与简繁转换；卡片式 tab 切换，进度条按真实阶段推进。
- **本地书库**：扫描下载目录或手动导入书籍，书架卡片网格与筛选 / 排序；封面自动提取，
  取不到时按书名生成配色封面；书籍信息、编辑、重新定位、隐藏与移出书架走右键菜单。
- **阅读器**：TXT / EPUB 阅读，滚动与整章连续翻页两种模式；目录抽屉、排版面板
  （字号 / 行距 / 字体 / 主题，默认跟随应用主题）、沉浸模式、书籍信息抽屉；
  书内搜索与命中高亮、书签、阅读进度落盘；滚动到底自动续接下一章。
- **编码**：自动检测解码不匹配并提示，可换编码重试或以「仍要打开」有损阅读，结果可写回。
- 新增 `[reader]` 阅读配置节。

### Changed

- **运行数据收进嵌入式 SurrealKV**（`~/.sonovel/state`）：书架、下载任务与书源选择
  从三份 JSON 合并进一个库，带 schema 版本闸门；写入即 fsync，改动立刻落盘。
- **单实例**：启动时独占运行数据目录，第二个实例提示「应用已在运行」并退出。
- **封面与阅读解析缓存统一到 `<下载目录>/.sonovel/`**（`covers/` 与 `books/`，Windows 下
  隐藏）；首次打开一本书生成解析缓存，再开秒开。
- 旧的 `library.json` / `tasks.json` / `sources_config.json` 与 `~/.sonovel/covers/`
  不做迁移，原样保留。
- 升级依赖：`gpui-kit` 0.9.1 → 0.10.0，`zip` / `scraper` / `base64` 等大版本迁移。

### Fixed

- 阅读器：正文列宽收成单一来源、滚动到底自动续章、页边界回到行边界；翻页模式的视口
  定位与行高取整；修正滚动偏移符号、`Esc` 关闭与恢复重绘。
- 阅读器：工具栏与页脚改图标按钮，`F11` 只认首次按下，作废残留的收起任务。
- 书架：书籍信息弹窗长值换行不再被裁掉；扫描分批进行，关窗落盘，默认阅读模式生效。
- 配置：过滤 TOML 中非法的 `NaN` / `inf` 浮点值。

## [0.6.0] - 2026-11-10

### Changed

- **全仓按 [Google Rust 风格指南](https://google.github.io/styleguide/rust.html) 重构**：
  rustfmt 切换到 Google 选项（`max_width=100`、`imports_granularity=Module`、
  `group_imports=StdExternalCrate`）；首方自由函数改为 `module::function()` 调用、类型名
  改为非限定导入、trait 导入下沉到使用点；叶子模块按主条目重命名
  （`http/cf.rs → cloudflare.rs`、`http/clients.rs → http_clients.rs`、
  `models/task_record.rs → download_task_record.rs`），实现搬出 `db/dao.rs`、
  `desktop/{actions,run}.rs`、`desktop/model/app_model.rs` 与 5 个 `*_page.rs`。
- **命名与公开 API**：`*Cfg → *Config`、`OpsCtx → OpsContext`、`random_ua → random_user_agent`
  等；移除 19 个 `impl Into<T>` 参数，元组返回改具名结构体（`LoadedTasks` / `ResolvedBook` /
  `GhProxy`），配置枚举补 `FromStr`（`parse` 保留为薄包装），`bool` 参数改 `ThemeVariant`。
- **性能与内存**：用 `Arc` 共享替换每帧 / 每任务的深拷贝——`Arc<Rule>`、`Arc<Book>`、
  `Arc<SearchResult>` 贯穿下载、任务、搜索与书源列表；章节重试复用同一个 `Arc<Chapter>`；
  任务列表持久化不再克隆整张 `Vec<DownloadTask>`，序列化留在调用线程、只把写盘丢进
  `spawn_blocking`。全仓 `.clone()` 调用点 329 → 293。
- **文档**：补齐全仓 424 个公开项中的 118 个文档缺口，修正 Markdown、写在 `use` 上的文档
  与分裂的 `//!`；文档示例统一改用 `expect("原因")`。
- **工程**：新增 CI（`fmt` 固定 `nightly-2026-11-08`，`clippy` + `test` 固定 1.99.0）；
  lint 收紧为 `unsafe_code` / `unwrap_used` deny，`pedantic` + `nursery` warn，CI 上
  `-D warnings`；补充 105 处 `#[must_use]` 与 37 个 `# Errors` 小节。

### Fixed

- **解析缓存不再吞掉 panic**：删除 `parser/cache.rs` 两处 `panic::catch_unwind`，
  poison 场景改由 scoped thread `join()` 断言覆盖。

### 说明

- 本次发布没有用户可见的功能变更：界面、书源规则与下载行为保持不变，改动集中在代码结构、
  所有权与工具链。
