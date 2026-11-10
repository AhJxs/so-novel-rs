# Changelog

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
