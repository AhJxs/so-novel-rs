# so-novel-rs

So Novel 的 Rust + egui 桌面客户端重写。**仍在分阶段迁移中**，详见仓库根的
[`docs/rust-egui-migration-audit.md`](../docs/rust-egui-migration-audit.md)。

旧 Java 实现保留在仓库根 `src/main/java/`，未迁移完成前不要删除。

## 当前进度

**阶段 1 — 骨架 + 数据/配置/规则（已完成）**

- eframe 应用骨架，左侧导航 + 6 页占位（**每页明确显示该功能尚未实现**，不是伪 UI）。
- 数据模型：`AppConfig` / `Rule` / `Book` / `Chapter` / `SearchResult` / `SourceInfo`。
- `config.ini` 兼容读写（与 Java 版字段对齐）。
- 书源规则加载（`*.json` + `*.json5`，支持单文件或目录），含 baseUri / timeout / meta 默认填充。

**阶段 2a — HTTP / 编码 / 选择器 / JS（已完成）**

- `http/`：reqwest blocking 客户端（rustls，无 OpenSSL）、代理、UA 池、Cloudflare title 检测、编码兜底（Content-Type → meta → chardetng → UTF-8）、`buildData`（hutool 风格宽松 JSON 解析）、`format_url_query`、随机抓取间隔、不可见字符清洗。
- `js/`：boa_engine 包装；`post_process(body, input)` 等价 Java `JsCaller#call`；`eval_function_returning_string` 用于 quanben5 加密；自动注入 `console` no-op shim。
- `parser/dom.rs`：scraper CSS 选择器封装 + `@js:` 后处理 + `ContentType` 派发；`clear_all_attributes`、`remove_tags`；XPath 命中返回类型化错误（待阶段 2c 决定改写）。

**阶段 2b — SearchParser + BookParser（已完成）**

- `http/url_join.rs`：`abs_url(base, href)` — 处理相对/绝对/协议相对/dot-paths。
- `http/fetch.rs`：单次 GET/POST 同步抓取，含 UA / Referer / Cookie 注入与编码兜底。
- `parser/search.rs`：构造 GET/POST、CF 检测、result 列表抽取（bookName 必填）、href absUrl、limit 截断。
- `parser/book.rs`：详情解析；以 `meta[` 开头的字段走 ATTR_CONTENT，否则走 TEXT；coverUrl 相对路径 absUrl。
- 真实联网测试 `#[ignore]` 标注，本机执行通过（笔趣阁22）。

**阶段 2c — TocParser + ChapterParser + CF 旁路（已完成）**

- `http/cf.rs::fetch_via_cf_bypass`：调用外部 `${cf-bypass}/html?url=<target>` 服务获取去 CF 后 HTML；search/book/toc/chapter 全部接入。
- `parser/dom.rs::xpath_to_css`：极小 XPath → CSS 改写（覆盖现有规则唯一一处 XPath：96读书 nextPageInJs）。
- `parser/toc.rs`：单页 / option 下拉分页（22biqu）/ 下一页递归 / `isDesc` 倒序（69shuba）/ `Book.url` 正则提取书 ID 注入 toc.url 模板。
- `parser/chapter.rs`：单页 / 分页递归 / `nextPageInJs`（含 XPath 路径）/ 终止判定（`nextChapterLink` 正则 + 通用兜底）；**不**做正文清洗 / 模板渲染（归阶段 3）。

**阶段 3a — 内容处理纯函数层（已完成）**

- `parser/filter.rs`（ChapterFilter）：不可见字符 / HTML 实体 / filterTxt 正则替换（backreference 不支持时降级 warn，不崩）/ filterTag 节点删除 / 重复标题去除 / `1.章节名`→`第1章 章节名` / 空 tag 清理。
- `parser/formatter.rs`（ChapterFormatter）：clear_all_attributes 后段落整形 — `paragraphTagClosed=true` 替换非 p 闭合标签为 `<p>`，否则按 `paragraphTag` 切分包 `<p>`。
- `export/render.rs`（ChapterRenderer）：拼装 filter → formatter → 模板渲染；txt 用全角缩进抽段落，html/epub 用内嵌模板（`assets/chapter_{html,epub}.tmpl`，仅 `${title}`/`${content}` 字符串替换），pdf 降级 html + warn 不崩。

**阶段 3b — 导出层（已完成）**

- `export/exporter.rs`：`Exporter` trait + `write_chapter_files`（前导零文件名）+ `sort_chapter_files` + `exporter_for` 派发（PDF 降级 Html）。
- `export/txt.rs`：首页书籍信息 + 全章节合并；UTF-8 / GBK / Big5 等编码切换（`encoding_rs`）；intro 去 HTML 标签 + 实体；空 intro 填"暂无"。
- `export/html.rs`：每章一文件 + `0_目录.txt` + zip 打包（`zip` crate，仅 deflate，无 C 依赖）。
- `export/epub.rs`：`epub-builder` 0.8 V3.0；metadata + 章节 XHTML；`merge_with_cover_bytes` 接收外部封面字节（PNG/JPEG magic 自动识别 mime）；export 层**不**联网。

**阶段 3c — 调度层（已完成，整套数据管线端到端可用）**

- `crawler/retry.rs`：泛型 `retry_with_backoff(op, max_attempts, sleep_fn)`；`linear_backoff(base, attempt)` 递增退避。
- `crawler/mod.rs::download_book`：核心入口；流程 = parse_book → parse_toc → tokio Semaphore 并发抓 chapter（spawn_blocking + 重试 + 随机间隔）→ render → write → 封面下载 soft-skip → `Exporter::merge_with_cover` → 按 preserve_chapter_cache 清理 → 推 Finished。
- `Progress` 5 种事件 + `CancelToken`（Arc<AtomicBool>，无额外依赖）+ `CrawlerError`。
- 失败章节**不**中断整本下载（与 Java 一致），通过 `Progress::ChapterFailed` 上报。

**阶段 4a — UI 接入真实后台（已完成）**

- `crawler/search.rs::search_aggregated`：多源并发聚合搜索，每源 spawn_blocking。
- `app.rs`：Arc<tokio Runtime> + SearchState + DownloadTask 列表；update 循环 try_recv 排空进度通道，绝不阻塞 GUI；任意活动任务 200ms 重绘。
- `ui/pages/search.rs`：真实搜索框（关键字 + 书源选择）+ 源状态徽章（⏳/✓/∅/✗）+ 结果表格 + 下载按钮 → `spawn_download(...)`。
- `ui/pages/tasks.rs`：进度条 + 失败章节折叠列表 + 取消按钮。
- 启动 egui 窗口成功，CJK 字体自动加载，主循环正常。

**阶段 4b — 本地书库页 + 任务页打磨（已完成）**

- `util/system.rs`：跨平台 `open_path` / `reveal_in_folder`（cmd/open/xdg-open/explorer/select），无 opener 依赖。
- `app.rs`：`LibraryEntry` / `LibraryState` + `refresh_library()` + `delete_library_entry()`，扫描 `download_path` 不递归。
- `ui/pages/library.rs`：搜索过滤 + 格式下拉 + 刷新 + 表格（文件名/格式/大小/修改时间/操作）+ 打开/位置/删除二次确认。
- `ui/pages/tasks.rs` 升级：完成任务的"打开"+"位置"按钮、失败任务的"重试"、顶部"清除已完成"。

**阶段 4c — 书源管理升级 + 搜索详情面板（已完成）**

- `rules/overrides.rs::SourceOverrides`：用户启用/禁用 sidecar JSON 持久化（不污染上游 rules），只覆盖"禁用"方向。
- `crawler/health.rs::check_sources_health`：HEAD 5s 超时，spawn_blocking 并发，mpsc 实时推送。
- `app.rs`：`SourcesState` + `spawn_health_check`；`SearchState` 加 `selected` + `detail_cache` + `DetailState`，`select_search_result` 缓存命中跳过 + spawn_blocking 调 `parse_book_detail`。
- `ui/pages/sources.rs`：延迟彩色标记（≤400ms 绿/≤1500ms 黄/其它橙/不通红）+ HTTP 状态色标 + 启用/禁用 toggle 立即持久化。
- `ui/pages/search.rs`：左右分栏（结果列表 + 详情面板）；书名 `SelectableLabel` 高亮选中行；详情面板 4 状态（无选择 / Pending / Failed / Loaded）。封面预览推阶段 5b。

**阶段 5a — quanben5 加密搜索 + 相似度过滤排序（已完成）**

- 加 `strsim` 依赖（纯 Rust，等价 Java hutool `StrUtil.similar`）。
- `parser/search_filter.rs::filter_sort`：等价 Java `SearchResultsHandler#filterSort`，按 `cfg.search_filter` 在 SearchState drain 完成时一次性触发。
- `parser/search_quanben5.rs`：`quanben5.js` 通过 `include_str!` 嵌入；`is_quanben5_pattern` 检测双 `%s` 自动派发；JSONP 解析含 unicode / HTML 实体 / 反斜杠还原 + 花括号配平。

测试：单元 180 + 集成 3 = **183 全过**（2 个 ignored 真实联网）。

## 未实现（按阶段）

- **阶段 5b（视觉/可用性）**：OpenCC 简繁；封面预览（`egui_extras` + `image`）。
- **阶段 5c（增强）**：CLI（clap）；CoverUpdater (qidian cookie)；批量下载；搜索建议；HTTP 层级取消；PDF（如评估再做）。

## 运行

```sh
cd so-novel-rs
cargo run
```

工作目录建议在仓库根，使应用能找到 `bundle/config.ini` 和 `bundle/rules/`：

```sh
cd <repo-root>
cargo run --manifest-path so-novel-rs/Cargo.toml
```

## 测试

```sh
cargo test --manifest-path so-novel-rs/Cargo.toml
```

测试默认从 `CARGO_MANIFEST_DIR/../bundle` 读取真实规则与配置文件。
