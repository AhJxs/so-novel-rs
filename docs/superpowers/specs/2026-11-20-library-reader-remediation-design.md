# 本地书库 / 阅读器缺陷修复与性能优化设计

**日期**：2026-11-20
**基线**：`master` @ 53c39f7
**审查方式**：三路并行静态审查（书库 / 阅读器 / 存储与跨模块），关键结论逐行复核，并对
`attrib` 退出码做了本机实测。未运行 GUI、未修改代码。

## 1. 背景

v0.7.0 引入本地书库、阅读器、工具箱与 SurrealKV 运行数据后，功能面快速扩张，但缺少一轮
面向「数据一致性 + 长时阅读」的缺陷收口。本次审查确认约 50 个问题，按用户可见程度分为三档，
并拆成三个可独立交付的工作流。

## 2. 问题清单（编号与实施计划一一对应）

### 2.1 数据一致性（工作流 A）

| 编号 | 问题 | 位置 | 级别 |
|---|---|---|---|
| A-1 | 换下载目录后，所有 `Downloaded` 书被判 missing；封面与阅读缓存随即失效 | `library/store.rs:248-257` | 高 |
| A-2 | relink 到下载目录外，下次扫描又被判 missing | `library/store.rs:491-528` | 高 |
| A-3 | 下载目录内的 `Imported` 书「移出书架」后会被扫描加回 | `desktop/model/library.rs:263-280` | 中 |
| A-4 | missing 书「删除文件」因 `NotFound` 直接失败，记录永久卡死 | `desktop/model/library.rs:283-298` | 高 |
| A-5 | 重复导入已变化的文件不刷新 size/mtime/编码，也不重新提取元数据 | `library/store.rs:280-292` | 低 |
| A-6 | 下载完成 / 工具箱产物落盘后书库不刷新，必须手动重扫 | `desktop/model/events.rs:94-129` | 高 |
| A-7 | 扫描发现下载目录不存在时静默返回，按钮「没反应」 | `desktop/model/library.rs:22-24` | 低 |

### 2.2 隐藏目录与封面（工作流 A）

| 编号 | 问题 | 位置 | 级别 |
|---|---|---|---|
| A-8 | `.sonovel` 隐藏属性永不生效（`attrib` 对不存在路径也返回 0，`HIDDEN` 被提前置真）；`covers_dir()` 在渲染路径产生副作用 | `library/paths.rs:32-46`、`desktop/model/library.rs:131-135` | 中 |
| A-9 | 封面替换 / 移除 / 提取竞态留下孤儿文件；网络封面整图解码后丢弃；封面按原始分辨率落盘 | `library/cover.rs:37-56`、`desktop/model/cover.rs:36-46` | 中 |
| A-10 | `apply_meta` 替换 `cover_file` 时不删除旧文件 | `library/store.rs:330-335` | 低 |

### 2.3 解析与分章（工作流 B）

| 编号 | 问题 | 位置 | 级别 |
|---|---|---|---|
| B-1 | TXT 把「标题行 + 紧跟正文」粘成一个段落，标题不再匹配，整本退化成按 1 万字硬切 | `tools/load/txt.rs:33-61`、`tools/split.rs:48` | 高 |
| B-2 | 无正文的标题章（卷标题）被静默丢弃 | `tools/split.rs:51-53` | 低 |
| B-3 | EPUB 不走 `split_oversized`；单个巨型段落永远切不开 | `tools/load/mod.rs:42`、`tools/split.rs:89-126` | 中 |
| B-4 | 阅读缓存临时文件固定名、无 fsync、失败不清理；解析期间源文件变化会写入错配缓存 | `reader/cache.rs:144-155` | 低 |
| B-5 | `CACHE_VERSION` 与解析器修订无联动 | `reader/cache.rs:16-17` | 低 |
| B-6 | TXT 打开时整文件读两遍；显式编码时仍做无用的整文件探测；BOM 优先时记录的却是显式标签 | `reader/content.rs:132-171`、`tools/encoding.rs:54-57` | 中 |
| B-7 | 「仍要打开」把失败的编码写回书架，下次打开继续失败 | `desktop/pages/reader/reader_page.rs:473-488,576-581` | 高 |

### 2.4 阅读进度与导航（工作流 B）

| 编号 | 问题 | 位置 | 级别 |
|---|---|---|---|
| B-8 | 单页 / 一屏章节 ratio 恒为 0，末章永远到不了 100%，`finished` 不置位 | `reader/layout.rs:164-171`、`reader_page.rs:655-663` | 中 |
| B-9 | 翻页首帧用估算视口高度分页并写进度，探针回报后页号漂移 | `reader_page.rs:1472-1580` | 中 |
| B-10 | 滚动模式下开合抽屉 / 缩放窗口不保持阅读位置 | `reader_page.rs:1732-1805` | 中 |
| B-11 | 短章被惯性滚动连续跳过 | `reader_page.rs:1812-1840` | 中 |
| B-12 | 搜索 / 书签跳转按段落序号比例定位，长短段不均时落点错误 | `search_panel.rs:245-258`、`layout.rs:175-181` | 中 |
| B-13 | Loading / Error 状态下仍会写进度、切模式、加空书签 | `reader_page.rs:655-699,898-958` | 低 |
| B-14 | 重新解析后章节数变少，越界书签不清理，跳转静默落到末章 | `reader_page.rs:555-557`、`bookmarks_panel.rs:80-95` | 低 |
| B-15 | 搜索抽屉关闭再打开显示「0 条结果」但并未重搜 | `search_panel.rs:175-186` | 低 |
| B-16 | `page_turn_to_last` 跨跳转残留 | `reader_page.rs:118-119,1563-1568` | 低 |
| B-17 | 搜索高亮 / 目录过滤 / snippet 的每帧重复分配 | `search_panel.rs:265-319`、`toc.rs:40-48`、`reader/search.rs:75-93` | 低 |

### 2.5 性能与生命周期（工作流 C）

| 编号 | 问题 | 位置 | 级别 |
|---|---|---|---|
| C-1 | `merge_scan` 在 UI 线程做 `canonicalize` + O(N²) 查找 | `library/store.rs:200-259` | 中 |
| C-2 | `books_version` 无人读取；书库页对任何后台事件全量重排 + 每卡片磁盘 stat | `library_page.rs:538-559`、`library_state.rs:32` | 中 |
| C-3 | 元数据 worker 完成后不唤醒 UI，每本固定白等 ≤100 ms | `desktop/model/library.rs:63-84` | 中 |
| C-4 | 每 5 秒 UI 线程深拷贝整库 + pretty 序列化 + 同步 KV 提交；两套脏标记重复写 | `library.rs:91-113`、`store.rs:141-145` | 中 |
| C-5 | 设置页无 debounce，逐字符 / 逐像素写 `config.toml` | `settings_page.rs:111-123,170-187` | 中 |
| C-6 | 正文每帧整章段落复制两遍 | `reader_page.rs:1672-1676,1754-1758` | 中 |
| C-7 | 全文搜索在 UI 线程、预算只按章检查，单章可阻塞数秒 | `search_panel.rs:141-170` | 中 |
| C-8 | 搜索结果最多 1000 条每帧全量重建，未虚拟化 | `search_panel.rs:354-466` | 中 |
| C-9 | 翻页模式渲染整章 DOM；宽高变化整章重排 | `reader_page.rs:1700-1728` | 中 |
| C-10 | EPUB / HTML 每章重新编译正则 | `load/epub.rs:105-107,185-192`、`load/html.rs:229-236` | 中 |
| C-11 | 阅读器仍解压 EPUB 封面；更多 / 书签抽屉每帧 clone 整本 | `load/epub.rs:127,177-183`、`more_panel.rs:24-29` | 低 |
| C-12 | 退出链路重复落盘；`StateStore::close()` 生产路径从不调用 | `root.rs:365-369,146-171` | 中 |
| C-13 | 阅读器内导航快捷键仍生效；关闭后不恢复焦点 | `root.rs:329-404` | 中 |
| C-14 | 工具箱运行中仍可选源 / 输出；旧探测结果覆盖新选择 | `tools_page.rs:147-191,464-498` | 中 |
| C-15 | `config::global` 死代码；`validate()` 启动期不调用；`font-size` 不夹取 | `config/global.rs`、`config/toml_io.rs:128-130` | 低 |
| C-16 | 运行时任务表无上限；tasks/sources 损坏无备份 | `db/tasks.rs:27-63`、`state_store.rs:228-234` | 低 |
| C-17 | 空闲时 100 ms 固定轮询常驻唤醒 | `drain_loop.rs:23-51` | 低 |

## 3. 关键设计决策

### D1：`Book` 记录扫描根，missing 按根判定

`Book` 增加 `#[serde(default)] source_root: Option<String>`（规范化路径键）。

- 旧数据为 `None`：用 `path_key.starts_with(root_key)` 做一次启发式判定，命中时自愈写入。
- `merge_scan(files, scan_root)`：只有 `source_root == 当前根` 的书才因「不在扫描结果」判 missing；
  其他书回落到 `Path::exists()`（Imported 与「别的下载目录里的 Downloaded」）。
- `relink` 接收 `scan_root: Option<&Path>`：新路径在根内 → 保持 `Downloaded` + 写入根；
  否则改为 `Imported` + `None`。

**为什么不直接把旧书全标 Imported**：会破坏「扫描来源只隐藏、不删记录」的语义（A-3 修复后
隐藏按路径判定，origin 只影响展示与删除策略）。`source_root` 是显式、可迁移的。

### D2：「移出书架」按**是否位于当前扫描根**判定，而不是 origin

- 位于根内 → `hidden = true`（无论 Downloaded / Imported）。
- 不在根内 → 删记录 + 删封面 + 删阅读缓存。
- 判定抽成纯函数 `path_key_is_child_of(path_key, root_key)`，Windows 前缀比较按路径段，
  不用字符串 `starts_with`（避免 `D:\books2` 命中 `D:\books`）。
- 扫描归属按**直接子项**判定（`scan_download_dir` 不递归，只列出根目录下的文件）；
  「移出书架」的判定沿用同一规则。

### D3：完成度语义

- 章内比例的唯一语义：**用户当前看到的位置占整章的比例**；整章已完全可见时为 1.0。
- 新纯函数 `page_ratio(page_index, page_count) -> f32`：`page_count <= 1` 时返回 1.0，
  否则沿用 `ratio_for_page`。`ensure_pagination` / `goto_page` 改用它。
- `scroll_ratio` 在 `max_offset <= 1` 时返回 `Some(1.0)`（内容不足一屏 = 整章可见）。
- `record_progress` 的 `at_end` 判据不变（末章 + ratio ≥ 0.99），由上述两处保证可达。

### D4：首帧分页必须先测量再写进度

- `viewport_height` 为 `None` 时，翻页模式**不估高**：`ensure_pagination` 直接返回，
  `record_progress` 在「Page 模式且 `page_cache.is_none()`」时跳过。
- 占位视图必须包含 `viewport_probe`，否则永远量不到高度（当前早退分支没有探针）。
- 滚动模式下视口测量到变化时，若正文宽度未变，不重排（滚动模式不需要分页）；仅记录。

### D5：滚动模式位置保持

新增 `last_content_width: Option<f32>`。渲染入口比较当前 `content_width`：

- 宽度变化且 `mode == Scroll` 且非 `restore_pending` → 先 `capture_scroll_ratio()`，
  再置 `restore_pending = true`。下一帧现有的恢复逻辑按 ratio 重设偏移。
- 抽屉开合、窗口缩放都自然走这一条路径，不新增分支。
- 翻页模式仍由指纹重排 + `page_for_ratio` 保持比例。

### D6：惯性续章冷却

新增 `continue_armed_at: Option<Instant>`。跳章后 600 ms 内忽略续章判定；
期间收到向上滚动（`delta.y > 0`）立即重新武装。理由：触控板惯性在 100–400 ms 内到达，
600 ms 覆盖主要惯性；`restore_pending` 只能挡一帧，挡不住整段惯性。

### D7：跳转定位改用字符偏移

- 新增纯函数 `char_ratio(paragraphs: &[String], index: usize, byte_offset: usize) -> f32`：
  「前面所有段落字符数 + 段内字符偏移」/ 全章字符数。
- 搜索结果已有 `(paragraph_index, byte_offset)`，直接使用。
- `Bookmark` 增加 `#[serde(default)] byte_offset: Option<usize>`；旧书签为 `None` 时退回
  原来的段落比例，不影响阅读。

### D8：隐藏目录只做一次、只对存在的目录

- `hide_dir(dir)` 首行 `if !dir.exists() { return; }`。
- 进程内记录已隐藏路径集合（`Mutex<HashSet<PathBuf>>`），不是单一 bool。
- Windows 起 `attrib` 时加 `CREATE_NO_WINDOW`，避免黑窗。
- **getter 无副作用**：`covers_dir()` / `reader_cache_dir()` 只拼路径。
- 创建目录的两处（`library::write_cover`、`reader::cache::write_json`）在 `create_dir_all`
  之后调用 `hide_dir`；扫描开始时对已存在的缓存根补一次隐藏（覆盖历史目录）。

### D9：「仍要打开」不落盘失败编码

- `finish_load` 在 `force_decode == true` 时丢弃 `pending_encoding`，并在成功加载后把
  `force_decode` 复位。
- 同时不把 force 分支记录的实际编码写回书架（保持 `book.encoding` 原值）。

## 4. 非目标

- 不做书库数据库迁移（SurrealKV schema 不变；`Book` 新字段靠 serde default 兼容）。
- 不引入新依赖（封面缩放使用已有 `image`；路径比较用 `std::path`）。
- 不改动下载 / 书源 / 导出链路。
- 不做跨平台 UI 一致性验收（Windows 以外仅保证编译）。

## 5. 验收标准

每个工作流独立成立：

1. `cargo fmt --all -- --check`、`cargo clippy --all-targets -- -D warnings`、
   `cargo test --all-targets`、`cargo test --doc` 全绿。
2. 新增回归测试覆盖每个「高」级问题的触发场景（计划中逐条给出）。
3. 涉及 UI 的修复由用户在实机上按清单手点确认（`tasks/lessons.md` 的验收原则）。
4. 不改动任何既有 i18n key 的语义；新增 key 必须三语齐全并 `touch src/i18n.rs`。

## 6. 交付拆分

| 工作流 | 计划文档 | 独立可交付 |
|---|---|---|
| A：书库数据一致性 + 隐藏目录 / 封面 | `plans/2026-11-20-library-data-consistency.md` | 是（PR 1） |
| B：解析分章 + 阅读进度 / 导航 | `plans/2026-11-20-reader-parsing-progress.md` | 是（PR 2） |
| C：性能优化 + 生命周期 / 工具健壮性 | `plans/2026-11-20-performance-lifecycle.md` | 是（PR 3） |
