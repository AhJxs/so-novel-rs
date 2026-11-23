# 工作流 C：性能优化 + 生命周期 / 工具健壮性 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除书库/阅读器在长时使用下的卡顿与白等（UI 线程 I/O、每帧重复分配、无谓轮询），
并收敛退出落盘、焦点恢复、工具箱重入、配置校验等生命周期问题。

**Architecture:** 原则是「重活离开 UI 线程、稳定结果做缓存、状态变更只留一个真源」。
每个任务都用可测量的指标（调用次数 / 分配次数 / 唤醒间隔）或纯函数测试来固定行为。

**Tech Stack:** Rust 2024、gpui-kit、smol/tokio、serde_json、SurrealKV

**Spec:** `docs/superpowers/specs/2026-11-20-library-reader-remediation-design.md` §2.5

## Global Constraints

- 不引入新依赖；不使用 `unsafe`。
- 后台线程不碰 `Entity`；结果一律经 channel + `wakeup.notify()` 回 UI。
- 每个任务独立提交、可单独回滚；性能任务必须附带「改前 / 改后」的量化依据（计数或实测）。
- `cargo fmt` / `clippy -D warnings` / `test --all-targets` 每个任务结束都要绿。

---

### Task C1: 扫描合并移出 UI 线程，路径键只算一次

**Files:**
- Modify: `src/library/scan.rs:7-51`
- Modify: `src/library/store.rs:195-259`
- Modify: `src/desktop/model/library.rs:27-41`、`src/desktop/model/library_state.rs:96-99`
- Test: `src/library/store.rs`、`src/library/scan.rs`

**Interfaces:**
- Produces: `ScannedFile.key: String`（`normalize_path_key` 的结果）
- Produces: `LibraryStore::merge_scan(&mut self, files: &[ScannedFile], scan_root: &Path) -> Vec<String>`（内部用 HashMap 索引）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn scanned_files_carry_normalized_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, b"x").unwrap();
    let files = scan_download_dir(dir.path()).unwrap();
    assert_eq!(files[0].key, normalize_path_key(&path));
}
```

- [ ] **Step 2: 实现**

`scan_download_dir` 在 push 前算一次 `key: normalize_path_key(&path)`。
`merge_scan`：

```rust
        let mut by_key: HashMap<&str, usize> = self
            .books
            .iter()
            .enumerate()
            .map(|(index, book)| (book.path_key.as_str(), index))
            .collect();
```

匹配循环改为 `if let Some(&index) = by_key.get(key.as_str())`，新书 push 后
`by_key.insert(...)`（注意借用冲突：先收集新书到 `Vec<Book>`，循环结束后统一 push 并重建索引）。

`refresh_library_async` 的 `spawn_blocking` 已经负责扫描，`merge_scan` 仍在 UI；
把 `merge_scan` 的输入改成后台返回的 `Vec<ScannedFile>`（已带 key），UI 侧只剩 O(N) 合并。

- [ ] **Step 3: 验证**

```bash
cargo test --lib library
```

标准库没有 `canonicalize` 计数钩子，验证方式：`merge_scan` 内不再调用
`normalize_path_key`（grep 确认），2000 文件路径下 UI 帧耗时可用 `tracing` 临时打点对比。

- [ ] **Step 4: 提交**

```bash
git add src/library/scan.rs src/library/store.rs src/desktop/model/library.rs
git commit -m "perf(library): 扫描路径键在后台算一次，合并改用哈希索引"
```

---

### Task C2: 书库页按 `books_version` 缓存卡片

**Files:**
- Modify: `src/library/query.rs:73-81`（`ShelfQuery` derive `PartialEq`）
- Modify: `src/desktop/pages/library/library_page.rs:528-559`
- Test: `src/library/query.rs`（查询等价性）

**Interfaces:**
- Produces: `LibraryPage` 私有字段 `render_cache: Option<(u64, ShelfQuery, PathBuf, Rc<Vec<CardData>>, usize)>`

- [ ] **Step 1: 写测试（缓存键语义）**

```rust
#[test]
fn query_equality_covers_all_filters() {
    let a = ShelfQuery::default();
    let mut b = a.clone();
    assert_eq!(a, b);
    b.favorite_only = true;
    assert_ne!(a, b);
}
```

- [ ] **Step 2: 实现**

`ShelfQuery` 派生 `PartialEq`。

`LibraryPage` 加字段：

```rust
    /// (books_version, query, covers_dir, cards, total)：数据没变就复用上一帧结果。
    render_cache: Option<(u64, ShelfQuery, PathBuf, Rc<Vec<CardData>>, usize)>,
```

渲染时：

```rust
        let (version, query, covers) = self.model.read_with(cx, |model, _cx| {
            (
                model.library.books_version,
                model.library.query.clone(),
                model.covers_dir().unwrap_or_default(),
            )
        });
        let cached = self
            .render_cache
            .as_ref()
            .filter(|(v, q, c, _, _)| *v == version && *q == query && *c == covers);
        let (cards, total) = if let Some((_, _, _, cards, total)) = cached {
            (Rc::clone(cards), *total)
        } else {
            let (cards, total) = self.model.read_with(cx, |model, _cx| {
                let visible = visible_books(model.library.store.books(), &model.library.query);
                let total = visible.len();
                let cards: Vec<CardData> = visible
                    .iter()
                    .map(|book| CardData::from_book(book, covers.as_deref()))
                    .collect();
                (Rc::new(cards), total)
            });
            self.render_cache = Some((version, query, covers, Rc::clone(&cards), total));
            (cards, total)
        };
```

注意：缓存里存的是**全部**可见书的卡片，渲染时再 `.iter().take(self.visible_limit)`，
`visible_limit`（「加载更多」）变化不需要重建缓存。

`CardData` 不再每帧 clone；`visible_books` 的全量过滤排序与 `is_file()` 磁盘 stat 只在
`books_version` / 查询 / 封面目录变化时发生。

- [ ] **Step 3: 验证**

```bash
cargo test --lib library::query
cargo clippy --all-targets -- -D warnings
```

实机：下载进行中停留在书库页，任务栏进度更新时书库区不应持续闪烁重排。

- [ ] **Step 4: 提交**

```bash
git add src/library/query.rs src/desktop/pages/library/library_page.rs
git commit -m "perf(library): 书库列表按版本号缓存，避免每帧全量重排与磁盘 stat"
```

---

### Task C3: 元数据 worker 完成后立即唤醒 UI

**Files:**
- Modify: `src/desktop/model/library.rs:44-85`
- （`AppModel.wakeup: WakeupHandle` 已存在且可 `Clone`，无需改 `app_model.rs`）

- [ ] **Step 1: 实现**

`spawn_meta_worker` 取 `let wakeup = self.wakeup.clone();`（`WakeupHandle: Clone`），
在异步任务里紧挨 `tx.send(...).await` 之后 `wakeup.notify();`。

`refresh_library_async` 同样在 `tx.send(event).await` 后 `wakeup.notify()`。

参考 `desktop/model/ops/download.rs:94-99` 的既有写法，保持一致。

- [ ] **Step 2: 验证**

```bash
cargo check --all-targets
```

说明：drain_loop 的 wakeup 是容量 1 的 channel，`notify` 不会堆积。

- [ ] **Step 3: 提交**

```bash
git add src/desktop/model/library.rs src/desktop/model/app_model.rs
git commit -m "perf(library): 元数据与扫描完成后立即唤醒 UI，去掉 100ms 白等"
```

---

### Task C4: `persist_library` 只在脏时写，且不深拷贝整库

**Files:**
- Modify: `src/library/store.rs:130-146`
- Modify: `src/desktop/model/library.rs:87-119`
- Test: `src/library/store.rs`

**Interfaces:**
- Produces: `LibraryStore::mark_dirty(&mut self)`
- Produces: `LibraryStore::is_dirty(&self) -> bool`（已存在，确认可见性）

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn to_json_does_not_pretty_print() {
    let mut store = LibraryStore::default();
    // 空库也要能序列化；格式为紧凑 JSON（无换行）。
    store.dirty = true; // 测试模块可访问私有字段
    let bytes = store.to_json().unwrap();
    assert!(!bytes.contains(&b'\n'));
}
```

- [ ] **Step 2: 实现**

- `LibraryFile` 改为借用切片：

```rust
#[derive(Serialize)]
struct LibraryFileRef<'a> {
    version: u32,
    books: &'a [Book],
}
// 反序列化仍用 LibraryFile（拥有 Vec<Book>）。
```

`to_json` 用 `LibraryFileRef { version: LIBRARY_VERSION, books: &self.books }` +
`serde_json::to_vec`。

- `persist_library` 开头：

```rust
        if !self.library.store.is_dirty() {
            return;
        }
```

新增 `force_persist_library()`（绕过 dirty 检查）供退出 / 首次迁移使用。

- `flush_library` 拆开：

```rust
    /// 退出时强制落盘书架（不看脏标记）。
    pub fn flush_library(&mut self) {
        self.library.store.mark_dirty();
        self.persist_library();
    }
```

`settings` 由调用方显式 `persist_settings()`。

- `ReaderPage::autosave` 的 `persist_library` 保留（dirty 已是真）。

- [ ] **Step 3: 验证**

```bash
cargo test --lib library::store
cargo test --all-targets
```

- [ ] **Step 4: 提交**

```bash
git add src/library/store.rs src/desktop/model/library.rs
git commit -m "perf(library): 书架只在脏时写入，序列化不再深拷贝整库"
```

---

### Task C5: 设置页输入与滑块合并写盘

**Files:**
- Modify: `src/desktop/pages/settings/settings_page.rs:100-190`
- Modify: `src/desktop/model/persistence.rs:23-27`（更正注释）
- Test: 逻辑改动 + 实机

**Interfaces:**
- Produces: `SettingsPage::schedule_persist(&mut self, cx: &Context<Self>)`（400ms 合并）
- Produces: `SettingsPage::flush_persist(&mut self, cx: &Context<Self>)`

- [ ] **Step 1: 实现**

`SettingsPage` 加字段：

```rust
    /// 合并写盘代次：每次输入变化 +1，计时器只写最新一代。
    persist_generation: u64,
```

```rust
    fn schedule_persist(&mut self, cx: &Context<Self>) {
        self.persist_generation = self.persist_generation.wrapping_add(1);
        let generation = self.persist_generation;
        let model = self.model.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(400))
                .await;
            let _ = this.update(cx, |page, cx| {
                if page.persist_generation != generation {
                    return;
                }
                model.update(cx, |model, _cx| model.persist_settings());
                cx.notify();
            });
        })
        .detach();
    }

    fn flush_persist(&mut self, cx: &Context<Self>) {
        self.persist_generation = self.persist_generation.wrapping_add(1);
        self.model.update(cx, |model, _cx| model.persist_settings());
        cx.notify();
    }
```

- 下载目录 / Cookie 输入框：`InputEvent::Change` → 只改 `config` + `schedule_persist`；
  `InputEvent::PressEnter { .. }` → `flush_persist`。不依赖失焦事件（组件不保证上报）：
  停止输入 400ms 后计时器一定落盘，退出时 `flush_before_quit` 再兜底一次。
- 字号滑块：`Change` → 改 config + `apply_font_size` + `schedule_persist`；
  `Release` → `flush_persist`。
- 其余开关类（theme、encoding、switch）保持立即 `persist_settings`（低频、语义明确）。

同时把 `persistence.rs` 顶部「暂不做 debounce」的注释改为「设置页输入走 400ms 合并写，
其余 setter 立即写」。

- [ ] **Step 2: 验证**

```bash
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

实机：在下载目录框输入长路径 → 输入过程中 `config.toml` 不被每个字符重写（可观察文件
修改时间）；停止输入 400ms（或按回车）后一次性写入最终路径。

- [ ] **Step 3: 提交**

```bash
git add src/desktop/pages/settings/settings_page.rs src/desktop/model/persistence.rs
git commit -m "perf(settings): 输入与滑块改 400ms 合并写盘"
```

---

### Task C6: 正文段落文本按章节 + 排版指纹缓存

**Files:**
- Modify: `src/desktop/pages/reader/reader_page.rs`（新增缓存字段 + 两个渲染入口）
- Modify: `src/desktop/pages/reader/content_view.rs:24-79`（`chapter_column` 接收 `&[String]`）
- Test: 纯函数 + 实机

**Interfaces:**
- Produces: `ReaderPage::display_paragraphs(&mut self, reader: &ReaderConfig, width: f32) -> Rc<Vec<String>>`

- [ ] **Step 1: 实现**

```rust
    /// 当前章 display_text 的结果 + 生成它的指纹；排版/宽度不变就复用。
    display_cache: Option<(u64, String, Rc<Vec<String>>)>,
```

指纹用 `LayoutFingerprint` 的字段哈希，或直接 `format!("{reader:?}{width}")` 生成字符串
（`ReaderConfig` 已派生 `Debug`；宽度按 `to_bits()` 格式化避免浮点噪声）。

```rust
    fn display_paragraphs(&mut self, reader: &ReaderConfig, width: f32) -> Rc<Vec<String>> {
        let key = format!(
            "{}:{:08x}:{}:{}:{}",
            reader.font_size,
            width.to_bits(),
            reader.line_height,
            reader.font_weight,
            reader.font_family
        );
        if let Some((chapter, cached_key, texts)) = &self.display_cache
            && *chapter == self.chapter as u64
            && *cached_key == key
        {
            return texts.clone();
        }
        let layout = ReaderLayout::new(/* 同渲染参数 */);
        let texts = Rc::new(
            self.content
                .as_ref()
                .and_then(|content| content.chapters.get(self.chapter))
                .map(|chapter| {
                    chapter
                        .paragraphs
                        .iter()
                        .map(|paragraph| layout.display_text(paragraph).into_owned())
                        .collect()
                })
                .unwrap_or_default(),
        );
        self.display_cache = Some((self.chapter as u64, key, texts.clone()));
        texts
    }
```

`ensure_pagination` 与两个 `render_*_body` 共用这一份；`chapter_column` 改为接收
`&[String]` 并用 `.iter().map(|text| text.as_str())` 渲染，删除 `.cloned()`。

`goto_chapter` / `reload_from_disk` 清空 `display_cache`（或靠章节号自然失效）。

- [ ] **Step 2: 验证**

```bash
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

实机：滚动大章节时滚动条拖动流畅度改善；改字号 / 页宽后正文立即按新排版重绘。

- [ ] **Step 3: 提交**

```bash
git add src/desktop/pages/reader/reader_page.rs src/desktop/pages/reader/content_view.rs
git commit -m "perf(reader): 缓存整章 display_text，去掉每帧两遍字符串拷贝"
```

---

### Task C7: 全文搜索移到后台并支持取消

**Files:**
- Modify: `src/desktop/pages/reader/search_panel.rs:141-228`
- Modify: `src/reader/search.rs:97-132`
- Test: `src/reader/search.rs`（可中断扫描）

**Interfaces:**
- Produces: `search::search_chapter_interruptible(chapter, needle, limit, deadline: &AtomicBool) -> Vec<Hit>`
- Produces: `SearchPanel` 使用 `smol::channel` 回传 `SearchEvent::{Batch(Vec<Hit>), Done}`

- [ ] **Step 1: 实现**

- `search_chapter` 的段落循环每 64 段检查一次 `cancel.load(Relaxed)`，命中即提前返回。
- `SearchPanel::start_search` 改为 `cx.spawn` + `background_executor`，每 32 章
  `tx.send(Batch)`；UI 侧 `spawn` 一个接收循环追加结果并 `cx.notify()`。
- 取消沿用现有 `search_cancel: Option<Arc<AtomicBool>>`。

- [ ] **Step 2: 验证**

```bash
cargo test --lib reader::search
```

实机：搜索 10 MB TXT 时拖动窗口 / 滚动目录不卡；关闭抽屉立即停止。

- [ ] **Step 3: 提交**

```bash
git add src/desktop/pages/reader/search_panel.rs src/reader/search.rs
git commit -m "perf(reader): 全文搜索移出 UI 线程并支持段级取消"
```

---

### Task C8: 搜索结果虚拟化

**Files:**
- Modify: `src/desktop/pages/reader/search_panel.rs:354-466`

- [ ] **Step 1: 实现**

把平铺的 `for` 循环改成 `uniform_list`（与目录同一个组件），分组标题作为列表项：
先构造 `Vec<SearchRow>`（`GroupHeader(String)` / `Hit { .. }`），再按固定行高渲染。
首屏之外的命中不再每帧建 div。

`uniform_list` 在 flex 子项里必须 `.h_full()` + `.track_scroll(&handle)`（见 lessons
2026-11-19「目录整块空白」）。

- [ ] **Step 2: 验证**

```bash
cargo clippy --all-targets -- -D warnings
```

实机：命中 500+ 条时滚动搜索列表流畅；点击命中仍能跳转。

- [ ] **Step 3: 提交**

```bash
git add src/desktop/pages/reader/search_panel.rs
git commit -m "perf(reader): 搜索结果改用虚拟列表"
```

---

### Task C9（可选，最后做）: 翻页模式只渲染当前页相交的段落

**Files:**
- Modify: `src/reader/layout.rs`（`paginate` 同时返回段落起始偏移）
- Modify: `src/desktop/pages/reader/reader_page.rs:1640-1730`
- Test: `src/reader/layout.rs`

**Interfaces:**
- Produces: `PageCache.paragraph_offsets: Vec<f32>`
- Produces: `visible_paragraphs(page_start: f32, viewport: f32, offsets: &[f32]) -> Range<usize>`

- [ ] **Step 1: 写测试**

```rust
#[test]
fn visible_range_covers_boundary_paragraphs() {
    let offsets = vec![0.0, 100.0, 200.0, 300.0];
    assert_eq!(visible_paragraphs(150.0, 100.0, &offsets), 1..3);
}
```

- [ ] **Step 2: 实现**

`paginate` 已有累计偏移，顺手返回 `Vec<f32>`。`PageCache` 保存它。
渲染时取 `range`，列顶部加 `.pt(px(offsets[range.start] - start))`（与现有
`-start` 位移合并），只渲染 `paragraphs[range]`。

若像素比对出现页边界错位，回滚本任务，不做进一步拆分（该优化不是必需项）。

- [ ] **Step 3: 提交**

```bash
git add src/reader/layout.rs src/desktop/pages/reader/reader_page.rs
git commit -m "perf(reader): 翻页模式只渲染当前页相交段落"
```

---

### Task C10: 正则缓存（EPUB / HTML 载入）

**Files:**
- Modify: `src/tools/load/html.rs:229-236`
- Modify: `src/tools/load/epub.rs:105-107,185-192`
- Test: `src/tools/load/html.rs`

- [ ] **Step 1: 实现**

```rust
static H1_RE: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"(?is)<h1[^>]*>(.*?)</h1>").unwrap());
// 同理 H2_RE / TITLE_RE / DC_TITLE_RE / DC_CREATOR_RE / DC_LANGUAGE_RE
```

`extract_tag_text` 改成按 tag 从静态表取 `&Regex`；`dc_text` 同理。
禁止在函数内 `format!` 生成 pattern。

- [ ] **Step 2: 验证**

```bash
cargo test --lib tools::load
```

- [ ] **Step 3: 提交**

```bash
git add src/tools/load/html.rs src/tools/load/epub.rs
git commit -m "perf(tools): 缓存 EPUB/HTML 元数据正则"
```

---

### Task C11: 阅读器不解压封面；抽屉快照按需

**Files:**
- Modify: `src/tools/load/epub.rs:127,177-183`
- Modify: `src/desktop/pages/reader/reader_page.rs`（ReaderBook 投影处）
- Modify: `src/desktop/pages/reader/more_panel.rs:24-29,46-47`
- Modify: `src/desktop/pages/reader/bookmarks_panel.rs:98-99`

- [ ] **Step 1: 实现**

- `LoadOptions` 增加 `include_cover: bool`，手写 `Default` 让默认值为 `true`（工具箱 /
  转换流程的现有 `..Default::default()` 行为不变）；`reader/content.rs` 构造
  `LoadOptions { source_encoding: explicit, chapter_regex: None, include_cover: false }`。
- `epub::load` 接受 `include_cover` 并把它透传；`include_cover == false` 时跳过
  `read_cover`，`SourceBook.cover` 为 `None`。
- `more_panel` / `bookmarks_panel` 改为只在 `open` 变 true 或 `books_version` 变化时
  从 model 取快照存到页面字段，`render` 读快照（不再每帧 `book_snapshot().cloned()`）。

- [ ] **Step 2: 验证**

```bash
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

- [ ] **Step 3: 提交**

```bash
git add src/tools/load/epub.rs src/desktop/pages/reader/
git commit -m "perf(reader): 阅读路径不解压封面，抽屉按需取快照"
```

---

### Task C12: 退出落盘收敛为单一路径

**Files:**
- Modify: `src/desktop/root.rs:145-171,365-369`
- Modify: `src/desktop/model/library.rs:115-119`
- Modify: `src/db/state_store.rs:191-198`
- Test: 逻辑改动 + 实机

- [ ] **Step 1: 实现**

- `Quit` action 只做 `close_reader(window, cx); cx.quit();`，删掉 `flush_library()`。
- `flush_before_quit` 明确为唯一退出落盘点：

```rust
    fn flush_before_quit(&mut self, cx: &mut Context<Self>) {
        if let Some(reader) = self.reader.take() {
            reader.update(cx, ReaderPage::close_and_flush);
        }
        self.model.update(cx, |model, _| {
            model.persist_settings();
            model.flush_library();
            model.persist_tasks();
            model.state.close();
        });
    }
```

- `close_and_flush` 里删掉 `persist_settings()`（设置由 `flush_before_quit` 统一写；
  阅读器只负责把排版设置写回内存 config）。
- `StateStore::close(&self)` 保持幂等，`Drop` 之外允许显式调用。

- [ ] **Step 2: 验证**

```bash
cargo test --all-targets
```

实机：阅读中 Cmd+Q 退出，重启后进度与设置都在；用 Process Monitor 粗看退出时
`config.toml` 写入次数为 1。

- [ ] **Step 3: 提交**

```bash
git add src/desktop/root.rs src/desktop/model/library.rs src/db/state_store.rs
git commit -m "fix(app): 退出落盘收敛为单一路径并显式关闭状态库"
```

---

### Task C13: 阅读器内禁用导航快捷键并恢复焦点

**Files:**
- Modify: `src/desktop/root.rs:328-370,390-404,145-156`
- Test: 逻辑改动 + 实机

- [ ] **Step 1: 实现**

把 `bind_nav_actions` 拆成 `bind_quit_action` 与 `bind_nav_actions`（后者内部先调前者）。
阅读器分支：

```rust
            return Self::bind_quit_action(
                div().track_focus(&self.focus).key_context(KEY_CONTEXT).flex().flex_col().size_full(),
                cx,
            )
            // …
```

即阅读器内 Cmd+1..6 / F6 不再关闭阅读器跳页；只保留 Cmd+Q。

`close_reader` 末尾：

```rust
        window.focus(&self.focus, cx);
```

- [ ] **Step 2: 验证**

```bash
cargo clippy --all-targets -- -D warnings
```

实机：阅读中按 Cmd+2 无反应；Esc / 返回关闭阅读器后，直接按 Cmd+3 能切页（无需先点窗口）。

- [ ] **Step 3: 提交**

```bash
git add src/desktop/root.rs
git commit -m "fix(reader): 阅读器内禁用导航快捷键，关闭后恢复主窗口焦点"
```

---

### Task C14: 工具箱运行中锁定选择入口，探测结果按路径校验

**Files:**
- Modify: `src/desktop/pages/tools/tools_page.rs:147-191,464-498,707-716`
- Test: 逻辑改动 + 实机

- [ ] **Step 1: 实现**

- `pick_source` 的编码探测闭包带上 `detect_path: PathBuf`，回写前：

```rust
                    let Ok(current) = this.update(async_cx, |this, cx| {
                        let form = &mut this.forms[kind.index()];
                        if form.source.as_deref() != Some(detect_path.as_path()) {
                            return false;
                        }
                        form.detected = Some(name.clone());
                        cx.notify();
                        true
                    }) else { return; };
                    let _ = current;
```

- 源文件 / 输出目录按钮 `.disabled(Box::new(form.running))`（`Disableable` 已在作用域）。
- `run_tool` 结束回写时已有 `form.running = false`；若运行期间用户切换了 `self.kind`，
  回写仍写 `forms[kind.index()]`（正确），但 `form.result` 的展示会随卡片切换，符合预期。

- [ ] **Step 2: 验证**

实机：选一个 50 MB TXT 后立刻改选另一个 → 编码标签最终与当前文件一致；运行中两个
选择按钮置灰。

- [ ] **Step 3: 提交**

```bash
git add src/desktop/pages/tools/tools_page.rs
git commit -m "fix(tools): 运行中锁定选择入口，编码探测结果按路径校验"
```

---

### Task C15: 配置校验与 `font-size` 夹取

**Files:**
- Modify: `src/core/bootstrap.rs:38-50`
- Modify: `src/config/toml_io.rs:128-130`
- Delete: `src/config/global.rs`（若确认无业务调用）
- Test: `src/config/`

- [ ] **Step 1: 实现**

`load_config` 末尾：

```rust
    config.global.font_size = config
        .global
        .font_size
        .clamp(crate::desktop::themes::FONT_SIZE_MIN, crate::desktop::themes::FONT_SIZE_MAX);
```

（若 `config` 模块不想依赖 desktop，把两个常量移到 `config::types`，desktop 从那里取。）

`bootstrap::load_context`：

```rust
    let config_err = config_err.or_else(|| config.validate().err().map(|e| e.to_string()));
```

`config/global.rs`：grep 确认只有 `mod.rs` re-export 与自身测试后删除，并删掉 re-export。

- [ ] **Step 2: 验证**

```bash
cargo test --all-targets
```

手改 `config.toml` 的 `font-size = 0` → 启动后 UI 仍可读（夹到 12）。

- [ ] **Step 3: 提交**

```bash
git add src/core/bootstrap.rs src/config/
git commit -m "fix(config): 启动时校验配置并夹取全局字号，删除死代码模块"
```

---

### Task C16: 任务表运行时裁剪 + 损坏数据备份

**Files:**
- Modify: `src/db/tasks.rs:27-63`
- Modify: `src/db/state_store.rs:155-188,228-234`
- Modify: `src/desktop/model/app_model.rs`（加载时的损坏回调）
- Test: `src/db/tasks.rs`、`src/db/state_store.rs`

- [ ] **Step 1: 实现**

- `LibraryState`/`AppModel` 在每次任务结束调用 `model.trim_completed_tasks()`：
  与 `tasks_json_with_trim` 同一阈值（最近 1000 条已完成），直接修改 `self.tasks`。
- `decode_schema` 改为必须 4 字节：`value.try_into().ok()`；长度不为 4 视为损坏。
- `AppModel::new_with_wakeup` 解析 tasks / sources 失败时先
  `state.backup_corrupt("tasks", bytes)` / `("sources", bytes)` 再回落默认，且本次不再覆盖写
  （与书库 `readonly` 语义一致）。

- [ ] **Step 2: 验证**

```bash
cargo test --lib db
```

- [ ] **Step 3: 提交**

```bash
git add src/db/ src/desktop/model/app_model.rs
git commit -m "fix(db): 运行时任务表也裁剪，损坏数据先备份再回落"
```

---

### Task C17: 排空循环空闲退避

**Files:**
- Modify: `src/desktop/drain_loop.rs:23-51`
- Test: 逻辑改动

- [ ] **Step 1: 实现**

```rust
        let mut idle = 0_u32;
        loop {
            let woken = wakeup.try_recv().is_some();
            if !woken {
                // 连续空闲时把兜底间隔从 100ms 退到 500ms；任一事件后立刻恢复。
                let backoff = if idle >= 5 { 500 } else { 100 };
                async_cx.background_executor().timer(Duration::from_millis(backoff)).await;
                let _ = wakeup.try_recv();
                idle = idle.saturating_add(1);
            } else {
                idle = 0;
            }
            // …drain
            if any {
                idle = 0;
            }
        }
```

注意：任务运行中必须保持 100ms 节奏，否则进度条变卡。进入循环前判断
`model.tasks.iter().any(Task::is_running)` 来重置 `idle`。

- [ ] **Step 2: 验证**

```bash
cargo clippy --all-targets -- -D warnings
```

实机：空闲时 CPU 占用下降；开始下载后进度更新仍是 100ms 级。

- [ ] **Step 3: 提交**

```bash
git add src/desktop/drain_loop.rs
git commit -m "perf(app): 排空循环空闲时退避到 500ms"
```

---

### Task C18: 目录过滤 / 搜索高亮 / snippet 去掉每帧重复分配

**Files:**
- Modify: `src/desktop/pages/reader/toc.rs:40-48`
- Modify: `src/desktop/pages/reader/search_panel.rs:265-319`
- Modify: `src/desktop/pages/reader/content_view.rs:24-79`
- Modify: `src/reader/search.rs:75-93`
- Test: `src/reader/search.rs`

- [ ] **Step 1: 目录小写标题缓存**

`ReaderPage` 加 `toc_lower: Option<Rc<Vec<String>>>`，在 `finish_load` 构造 `toc_titles`
的同一处构造（`title.to_lowercase()`）。`toc.rs` 的过滤直接遍历这份缓存，不再每帧
`to_lowercase()`。

- [ ] **Step 2: 高亮改稀疏表**

`chapter_highlights` 返回 `Option<HashMap<usize, Vec<(Range<usize>, HighlightStyle)>>>`，
只插入有命中的段落；`chapter_column` 的 `highlights` 参数改成同一个 map，按段落下标
`get(&index)` 查。删除 `vec![Vec::new(); count]`。

- [ ] **Step 3: snippet 复用字符边界表**

`search_chapter` 对每个段落先建一次 `Vec<usize>`（`char_indices` 的字节位置），
把 `&[usize]` 传给 `snippet`；`snippet` 不再自己 `collect`。段落无命中时跳过建表。

- [ ] **Step 4: 验证**

```bash
cargo test --lib reader::search
cargo clippy --all-targets -- -D warnings
```

实机：目录过滤框快速输入不卡；搜索面板滚动时高亮正常。

- [ ] **Step 5: 提交**

```bash
git add src/desktop/pages/reader/ src/reader/search.rs
git commit -m "perf(reader): 目录过滤、高亮与搜索摘要去重复分配"
```

---

## 验收（工作流 C）

自动化：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo test --doc
```

实机手点（交给用户）：

- [ ] 下载进行中停留在书库页 → 列表不闪烁、拖动流畅。
- [ ] 导入 20 本新书 → 标题/封面快速依次出现（不是每本隔 100ms）。
- [ ] 在设置页输入长下载路径 → 输入完成后才写盘；失焦后配置保存。
- [ ] 阅读 10 MB 大章节滚动流畅；改字号后立即重排。
- [ ] 全书搜索时窗口仍可交互，关闭抽屉立即停止。
- [ ] 阅读中按 Cmd+2 不切页；关闭阅读器后快捷键直接可用。
- [ ] 阅读中 Cmd+Q 退出 → 重启后进度、排版、书架都在。
- [ ] 手动把 `config.toml` 的 `font-size` 改成 0 → 应用启动后字号正常。
