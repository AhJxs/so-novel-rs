# 本地阅读器与书架改造 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把「本地书库」的文件列表升级为带封面 / 元数据 / 进度 / 收藏的书架，并新增应用内 TXT / EPUB 阅读器（滚动 + 翻页、目录、搜索、书签、沉浸模式）。

**Architecture:** 新增两个 UI 中立子系统：`src/library`（Book / LibraryStore / 扫描 / 元数据 / 封面 / 查询）与 `src/reader`（内容加载 / 分页纯函数 / 书内搜索）。`desktop` 层只做状态调度与渲染：`LibraryState` 持有 store 与后台 channel，`ReaderPage` 作为覆盖式视图挂在 `RootView` 上，不新增第 7 个导航页。

**Tech Stack:** Rust 2024（rust-version 1.95）、gpui-kit 0.10.0（GPUI + gpui-component）、serde / serde_json、toml_edit、encoding_rs、zip、epub-builder（已是普通依赖）、tempfile（dev）。

**Spec:** `docs/superpowers/specs/2026-11-18-local-reader-library-design.md`（本计划的每个行为都以它为准，实施时两份一起读）

## Global Constraints

- 仓库禁止 `unsafe`；`#![deny(clippy::unwrap_used)]`，生产代码禁止 `unwrap` / `expect` / `panic`；测试模块顶部写 `#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]`。
- clippy 走 `pedantic` + `nursery`，最终必须 `cargo clippy --all-targets -- -D warnings` 全绿。
- `src/library` 与 `src/reader` **不得** `use gpui_kit` / 任何 GUI 类型；它们只依赖 `config`、`db`、`tools`、`error`。
- 首版只有 TXT / EPUB 可读；其他格式的 `match` 分支返回错误 / 展示「暂不支持在应用内阅读」，不得删除扩展点。
- 所有用户可见文案走 `t!("...")`，`locales/app.yml` 三语齐备（en / zh-CN / zh-TW）；技术名词（TXT / EPUB）不译。
- 所有 JSON 落盘走 `crate::db::write_atomically`；`Book` 及其子结构每个字段带 `#[serde(default)]`（含容器级）。
- 注释用中文、只写「为什么」，不复述代码；public fn 带 `#[tracing::instrument]`（含 IO / 可能耗时的）与 `# Errors` / `# Examples` 文档，遵循 `src/lib.rs` 的工程规约。
- 每个任务结束都要跑：`cargo fmt --all`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。
- 不新增第三方依赖；不引入字体文件。

---

## 文件结构

新增：

```
src/library/mod.rs          # 模块门面与 re-export
src/library/book.rs         # Book / BookFormat / BookOrigin / ShelfStatus / ReadingProgress / Bookmark / 路径键与 ID
src/library/query.rs        # ShelfStatusFilter / ShelfSort / ShelfQuery / visible_books
src/library/cover.rs        # 魔数嗅探、封面文件名、写盘 / 删除、生成封面配色
src/library/meta.rs         # extract：EPUB OPF 元数据、TXT 编码探测
src/library/scan.rs         # scan_download_dir -> Vec<ScannedFile>
src/library/store.rs        # LibraryStore：加载 / 保存 / 合并扫描 / 导入 / 编辑 / 进度 / 书签
src/reader/mod.rs           # 模块门面与 re-export
src/reader/content.rs       # ReaderBook / ReaderChapter / load_book / ReaderError
src/reader/layout.rs        # ReaderLayout / ParagraphMetrics / paginate / 比例换算
src/reader/search.rs        # SearchGroup / SearchHit / 搜索执行
src/desktop/pages/reader/mod.rs          # ReaderPage 门面
src/desktop/pages/reader/reader_page.rs  # ReaderPage 状态机 + 布局骨架
src/desktop/pages/reader/content_view.rs # 正文渲染、测量、滚动 / 翻页
src/desktop/pages/reader/theme.rs        # ReaderTheme -> 颜色
src/desktop/pages/reader/toc.rs          # 目录抽屉
src/desktop/pages/reader/search_panel.rs # 搜索抽屉
src/desktop/pages/reader/bookmarks_panel.rs # 书签抽屉
src/desktop/pages/reader/typography.rs   # 排版面板
src/desktop/pages/reader/more_panel.rs   # 书籍信息 + 文件操作抽屉
src/desktop/pages/library/card.rs        # 书架卡片
src/desktop/pages/library/dialogs.rs     # 操作 / 编辑 / 信息 / 重新定位 / 确认弹窗
```

修改：

```
src/lib.rs                         # 注册 library / reader
src/config/types.rs                # ReadingMode / ReaderTheme / ReaderConfig
src/config/defaults.rs             # [reader] 模板
src/config/toml_io.rs              # [reader] 读写
src/config/paths.rs                # library_file / covers_dir
src/config/mod.rs                  # re-export
src/config/tests.rs                # 新配置测试
src/tools/load/epub.rs             # 抽出 read_metadata + package 解析
src/core/library.rs                # 删除 LibraryEntry，保留 SUPPORTED_LIBRARY_EXTS
src/desktop/model/library_state.rs # 重写为 LibraryStore + 扫描 / 元数据通道
src/desktop/model/library.rs       # AppModel 书架 / 阅读进度方法
src/desktop/model/events.rs        # drain 书架扫描 / 元数据 / 进度节流
src/desktop/model/app_model.rs     # 启动加载 library.json
src/desktop/model/mod.rs           # 调整 re-export
src/desktop/model/ops/library.rs   # 删除旧扫描 / 删除逻辑（改由 store 承担）
src/desktop/pages/library/*        # 整页重写
src/desktop/pages/mod.rs           # 导出 ReaderPage
src/desktop/root.rs                # 阅读器覆盖层 + 导航旁路
src/desktop/nav.rs                 # Nav.library 文案（书架）
src/desktop/actions.rs             # 无需新增 action（阅读器快捷键走 key_context）
src/i18n.rs                        # key 完整性清单
locales/app.yml                    # 新增 Library.* / Reader.*
README.md                          # 功能表与结构
```

---

### Task 1: `[reader]` 配置与路径

**Files:**
- Modify: `src/config/types.rs`, `src/config/defaults.rs`, `src/config/toml_io.rs`, `src/config/paths.rs`, `src/config/mod.rs`, `src/config/tests.rs`

**Interfaces:**
- Consumes: 现有 `toml_float` / `toml_bool` / `toml_str` helper。
- Produces:
  - `config::ReadingMode { Scroll, Page }`，`as_str()` / `parse(&str)`，`Default = Scroll`。
  - `config::ReaderTheme { Paper, Sepia, Night, Green, App }`，`as_str()` / `parse(&str)`，`Default = Paper`。
  - `config::ReaderConfig { font_size: f32, line_height: f32, page_width: f32, paragraph_spacing: f32, indent: bool, theme: ReaderTheme, default_mode: ReadingMode, font_family: String }` + `clamped(&self) -> Self`。
  - `AppConfig.reader: ReaderConfig`。
  - `ConfigPaths.library_file: PathBuf`（`~/.sonovel/library.json`）、`ConfigPaths.covers_dir: PathBuf`（`~/.sonovel/covers`）。

- [ ] **Step 1: 写失败测试**

在 `src/config/tests.rs` 末尾追加：

```rust
#[test]
fn reader_defaults_when_section_missing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[global]\nfont-size = 16\n").unwrap();
    let cfg = load_config(&path).unwrap();
    assert!((cfg.reader.font_size - 19.0).abs() < f32::EPSILON);
    assert!((cfg.reader.line_height - 1.8).abs() < f32::EPSILON);
    assert!((cfg.reader.page_width - 720.0).abs() < f32::EPSILON);
    assert!(cfg.reader.indent);
    assert_eq!(cfg.reader.theme, ReaderTheme::Paper);
    assert_eq!(cfg.reader.default_mode, ReadingMode::Scroll);
    assert!(cfg.reader.font_family.is_empty());
}

#[test]
fn reader_values_are_clamped_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[reader]\nfont-size = 99\nline-height = 0.2\npage-width = 10\nparagraph-spacing = 999\n",
    )
    .unwrap();
    let cfg = load_config(&path).unwrap();
    assert!((cfg.reader.font_size - 32.0).abs() < f32::EPSILON);
    assert!((cfg.reader.line_height - 1.3).abs() < f32::EPSILON);
    assert!((cfg.reader.page_width - 480.0).abs() < f32::EPSILON);
    assert!((cfg.reader.paragraph_spacing - 32.0).abs() < f32::EPSILON);
}

#[test]
fn reader_round_trips_through_save() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let mut cfg = AppConfig::default();
    cfg.reader.font_size = 22.0;
    cfg.reader.theme = ReaderTheme::Night;
    cfg.reader.default_mode = ReadingMode::Page;
    cfg.reader.indent = false;
    save_config(&path, &cfg).unwrap();
    let loaded = load_config(&path).unwrap();
    assert!((loaded.reader.font_size - 22.0).abs() < f32::EPSILON);
    assert_eq!(loaded.reader.theme, ReaderTheme::Night);
    assert_eq!(loaded.reader.default_mode, ReadingMode::Page);
    assert!(!loaded.reader.indent);
}

#[test]
fn reader_out_of_range_rejected_by_validate() {
    let mut cfg = AppConfig::default();
    cfg.reader.font_size = 40.0;
    assert!(cfg.validate().is_err());

    let mut cfg = AppConfig::default();
    cfg.reader.page_width = 100.0;
    assert!(cfg.validate().is_err());

    let cfg = AppConfig::default();
    assert!(cfg.validate().is_ok());
}
```

同步更新 `use crate::config::{...}` 列表，加入 `ReaderConfig, ReaderTheme, ReadingMode`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib config::tests::reader_ -- --nocapture`
Expected: 编译失败，`ReaderTheme` / `reader` 字段不存在。

- [ ] **Step 3: 实现类型**

`src/config/types.rs` 在 `ProxyConfig` 之后追加：

```rust
/// 阅读方式。定义在 `config` 里而不是 `library`：配置读写与业务共用，
/// 放业务层会让 `config` 反向依赖 `library`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReadingMode {
    /// 纵向连续滚动。
    #[default]
    Scroll,
    /// 左右翻页。
    Page,
}

impl ReadingMode {
    /// 存进 TOML / `library.json` 的取值。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Scroll => "scroll",
            Self::Page => "page",
        }
    }

    /// 解析配置值；无法识别时回落到 [`Self::Scroll`]。
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "page" => Self::Page,
            _ => Self::Scroll,
        }
    }
}

impl FromStr for ReadingMode {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

/// 阅读器配色主题。`App` = 跟随应用主题（背景 / 前景取自当前应用主题）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ReaderTheme {
    /// 纸质白。
    #[default]
    Paper,
    /// 羊皮纸（暖黄）。
    Sepia,
    /// 夜间（深灰）。
    Night,
    /// 护眼绿。
    Green,
    /// 跟随应用主题。
    App,
}

impl ReaderTheme {
    /// 存进 TOML 的取值。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Sepia => "sepia",
            Self::Night => "night",
            Self::Green => "green",
            Self::App => "app",
        }
    }

    /// 解析配置值；无法识别时回落到 [`Self::Paper`]。
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "sepia" => Self::Sepia,
            "night" => Self::Night,
            "green" => Self::Green,
            "app" => Self::App,
            _ => Self::Paper,
        }
    }

    /// 三语 key 后缀（`Reader.typography.theme.*`）。
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Paper => "Reader.typography.theme.paper",
            Self::Sepia => "Reader.typography.theme.sepia",
            Self::Night => "Reader.typography.theme.night",
            Self::Green => "Reader.typography.theme.green",
            Self::App => "Reader.typography.theme.app",
        }
    }
}

impl FromStr for ReaderTheme {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(s))
    }
}

/// `[reader]` 章节：排版与阅读方式偏好。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReaderConfig {
    /// 正文字号 px，16–32。
    pub font_size: f32,
    /// 行高倍数（相对字号），1.3–2.4。
    pub line_height: f32,
    /// 正文列最大宽度 px，480–1200。
    pub page_width: f32,
    /// 段间距 px，0–32。
    pub paragraph_spacing: f32,
    /// 首行缩进两个全角空格。
    pub indent: bool,
    /// 阅读主题。
    pub theme: ReaderTheme,
    /// 新打开的书默认用哪种阅读方式。
    pub default_mode: ReadingMode,
    /// 字体族；空 = 系统默认。
    pub font_family: String,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            font_size: 19.0,
            line_height: 1.8,
            page_width: 720.0,
            paragraph_spacing: 12.0,
            indent: true,
            theme: ReaderTheme::Paper,
            default_mode: ReadingMode::Scroll,
            font_family: String::new(),
        }
    }
}

impl ReaderConfig {
    /// 字号允许区间。
    pub const FONT_SIZE_RANGE: (f32, f32) = (16.0, 32.0);
    /// 行高倍数允许区间。
    pub const LINE_HEIGHT_RANGE: (f32, f32) = (1.3, 2.4);
    /// 页宽允许区间。
    pub const PAGE_WIDTH_RANGE: (f32, f32) = (480.0, 1200.0);
    /// 段距允许区间。
    pub const PARAGRAPH_SPACING_RANGE: (f32, f32) = (0.0, 32.0);

    /// 把越界值夹到合法区间。读取配置时调用：手改出错也不至于排版崩掉。
    #[must_use]
    pub fn clamped(&self) -> Self {
        Self {
            font_size: self
                .font_size
                .clamp(Self::FONT_SIZE_RANGE.0, Self::FONT_SIZE_RANGE.1),
            line_height: self
                .line_height
                .clamp(Self::LINE_HEIGHT_RANGE.0, Self::LINE_HEIGHT_RANGE.1),
            page_width: self
                .page_width
                .clamp(Self::PAGE_WIDTH_RANGE.0, Self::PAGE_WIDTH_RANGE.1),
            paragraph_spacing: self
                .paragraph_spacing
                .clamp(Self::PARAGRAPH_SPACING_RANGE.0, Self::PARAGRAPH_SPACING_RANGE.1),
            ..self.clone()
        }
    }
}
```

在 `AppConfig` 里加字段并补 `new()`：

```rust
    /// 阅读器排版与阅读方式（`[reader]`）。
    #[serde(default)]
    pub reader: ReaderConfig,
```

```rust
            proxy: ProxyConfig { /* 保持不变 */ },
            reader: ReaderConfig::default(),
```

在 `validate()` 末尾（`download_path` 检查之后）追加：

```rust
        let reader = &self.reader;
        for (field, value, range) in [
            ("reader.font_size", reader.font_size, ReaderConfig::FONT_SIZE_RANGE),
            ("reader.line_height", reader.line_height, ReaderConfig::LINE_HEIGHT_RANGE),
            ("reader.page_width", reader.page_width, ReaderConfig::PAGE_WIDTH_RANGE),
            (
                "reader.paragraph_spacing",
                reader.paragraph_spacing,
                ReaderConfig::PARAGRAPH_SPACING_RANGE,
            ),
        ] {
            if !(range.0..=range.1).contains(&value) {
                return Err(ConfigError::OutOfRange {
                    field,
                    value: f64::from(value),
                    min: f64::from(range.0),
                    max: f64::from(range.1),
                });
            }
        }
```

- [ ] **Step 4: 实现 TOML 读写**

`src/config/toml_io.rs` 顶部 use 加入 `ReaderConfig, ReaderTheme, ReadingMode`（`ReaderConfig` 仅用于区间常量时可直接用 `super::types::ReaderConfig`）。

`load_config` 在 `proxy` 段之后、旧主题迁移之前追加：

```rust
    // [reader]：缺失整节时用默认值；读到的越界值统一 clamp，手改配置不会让排版崩掉。
    let mut reader = cfg.reader.clone();
    if let Some(v) = toml_float(&doc, "reader", "font-size") {
        reader.font_size = v;
    }
    if let Some(v) = toml_float(&doc, "reader", "line-height") {
        reader.line_height = v;
    }
    if let Some(v) = toml_float(&doc, "reader", "page-width") {
        reader.page_width = v;
    }
    if let Some(v) = toml_float(&doc, "reader", "paragraph-spacing") {
        reader.paragraph_spacing = v;
    }
    if let Some(v) = toml_bool(&doc, "reader", "indent") {
        reader.indent = v;
    }
    if let Some(v) = toml_str(&doc, "reader", "theme") {
        reader.theme = ReaderTheme::parse(&v);
    }
    if let Some(v) = toml_str(&doc, "reader", "default-mode") {
        reader.default_mode = ReadingMode::parse(&v);
    }
    if let Some(v) = toml_str(&doc, "reader", "font-family") {
        reader.font_family = v;
    }
    cfg.reader = reader.clamped();
```

`save_config` 在 `proxy` 写入之后追加：

```rust
    set_float(&mut doc, "reader", "font-size", f64::from(cfg.reader.font_size));
    set_float(
        &mut doc,
        "reader",
        "line-height",
        f64::from(cfg.reader.line_height),
    );
    set_float(
        &mut doc,
        "reader",
        "page-width",
        f64::from(cfg.reader.page_width),
    );
    set_float(
        &mut doc,
        "reader",
        "paragraph-spacing",
        f64::from(cfg.reader.paragraph_spacing),
    );
    set_bool(&mut doc, "reader", "indent", cfg.reader.indent);
    set_str(&mut doc, "reader", "theme", cfg.reader.theme.as_str());
    set_str(
        &mut doc,
        "reader",
        "default-mode",
        cfg.reader.default_mode.as_str(),
    );
    set_str(&mut doc, "reader", "font-family", &cfg.reader.font_family);
```

- [ ] **Step 5: 模板与路径**

`src/config/defaults.rs` 在 `[cookie]` 之前插入：

```toml
[reader]
# 字号 px（16–32）；行高倍数（1.3–2.4）；正文列最大宽度 px（480–1200）；段距 px（0–32）
font-size = 19
line-height = 1.8
page-width = 720
paragraph-spacing = 12
indent = true
# paper | sepia | night | green | app
theme = "paper"
# scroll | page
default-mode = "scroll"
# 留空 = 系统默认字体
font-family = ""
```

`src/config/paths.rs`：

```rust
    /// 书架数据 `~/.sonovel/library.json`。
    pub library_file: PathBuf,
    /// 封面缓存目录 `~/.sonovel/covers/`。
    pub covers_dir: PathBuf,
```

```rust
            library_file: base.join("library.json"),
            covers_dir: base.join("covers"),
```

`src/config/mod.rs` 的 `pub use types::{...}` 加入 `ReaderConfig, ReaderTheme, ReadingMode`。

- [ ] **Step 6: 跑测试确认通过**

Run: `cargo test --lib config:: -- --nocapture`
Expected: 全部 PASS。

- [ ] **Step 7: 提交**

```bash
git add src/config
git commit -m "feat(config): 新增 [reader] 阅读配置与书架路径"
```

---

### Task 2: `library::book` 书籍模型

**Files:**
- Create: `src/library/mod.rs`, `src/library/book.rs`
- Modify: `src/lib.rs`（`pub mod library;`）

**Interfaces:**
- Consumes: `config::ReadingMode`。
- Produces:
  - `library::Book`、`BookFormat`、`BookOrigin`、`ShelfStatus`、`ReadingProgress`、`Bookmark`。
  - `Book::from_file(&Path, BookOrigin) -> Result<Book, String>`、`Book::status() -> ShelfStatus`、`Book::status_key() -> &'static str`。
  - `library::book::{hash64, id_for_key, normalize_path_key, now_unix, mtime_secs, absolute_dir}`。
  - `BookFormat::{from_extension, from_path, is_readable, label}`。

- [ ] **Step 1: 写失败测试**

`src/library/book.rs` 末尾（此时只有测试）：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn format_maps_extensions() {
        assert_eq!(BookFormat::from_extension("EPUB"), Some(BookFormat::Epub));
        assert_eq!(BookFormat::from_extension("markdown"), Some(BookFormat::Markdown));
        assert_eq!(BookFormat::from_extension("htm"), Some(BookFormat::Html));
        assert_eq!(BookFormat::from_extension("docx"), None);
        assert!(BookFormat::Epub.is_readable());
        assert!(BookFormat::Txt.is_readable());
        assert!(!BookFormat::Pdf.is_readable());
    }

    #[test]
    fn id_is_stable_and_depends_on_key() {
        assert_eq!(id_for_key("c:/books/a.txt"), id_for_key("c:/books/a.txt"));
        assert_ne!(id_for_key("c:/books/a.txt"), id_for_key("c:/books/b.txt"));
        assert_eq!(id_for_key("c:/books/a.txt").len(), 16);
    }

    #[test]
    fn path_key_lowercases_only_on_windows() {
        // 用真实存在的临时文件：既走 canonicalize 成功分支，又不依赖 C:\Books 是否存在。
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Novel.TXT");
        std::fs::write(&path, b"x").unwrap();
        let key = normalize_path_key(&path);
        if cfg!(windows) {
            assert_eq!(key, key.to_lowercase());
            assert!(key.ends_with("novel.txt"));
        } else {
            assert!(key.ends_with("Novel.TXT"));
        }
        assert_eq!(key, normalize_path_key(&path));
    }

    #[test]
    fn absolute_dir_keeps_absolute_and_joins_relative() {
        let absolute = if cfg!(windows) {
            Path::new("C:/books")
        } else {
            Path::new("/books")
        };
        assert_eq!(absolute_dir(absolute), absolute);
        let relative = absolute_dir(Path::new("books"));
        assert!(relative.is_absolute(), "相对路径必须转成绝对路径");
        assert!(relative.ends_with("books"));
    }

    #[test]
    fn from_file_fills_metadata_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("斗破苍穹.txt");
        std::fs::write(&path, "第一章\n正文").unwrap();

        let book = Book::from_file(&path, BookOrigin::Imported).expect("book");
        assert_eq!(book.title, "斗破苍穹");
        assert_eq!(book.format, BookFormat::Txt);
        assert_eq!(book.origin, BookOrigin::Imported);
        assert_eq!(book.size_bytes, "第一章\n正文".len() as u64);
        assert_eq!(book.id, id_for_key(&book.path_key));
        assert_eq!(book.status(), ShelfStatus::Unread);
        assert!(!book.missing);
        assert!(!book.favorite);
    }

    #[test]
    fn from_file_rejects_unsupported_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.docx");
        std::fs::write(&path, b"x").unwrap();
        assert!(Book::from_file(&path, BookOrigin::Downloaded).is_err());
    }

    #[test]
    fn status_follows_progress() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.epub");
        std::fs::write(&path, b"x").unwrap();
        let mut book = Book::from_file(&path, BookOrigin::Downloaded).expect("book");

        assert_eq!(book.status(), ShelfStatus::Unread);
        book.progress.updated_unix = 10;
        book.progress.percent = 12.0;
        assert_eq!(book.status(), ShelfStatus::Reading);
        book.progress.finished = true;
        assert_eq!(book.status(), ShelfStatus::Finished);
        assert_eq!(book.status_key(), "Library.status.finished");
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib library::book:: -- --nocapture`
Expected: 编译失败（类型不存在）。

- [ ] **Step 3: 实现**

`src/library/mod.rs`（**只声明本任务已有的模块**，后续任务逐行补 `pub mod` 与 re-export；
提前声明不存在的模块会直接编译失败）：

```rust
//! 书架数据层：书籍模型、持久化、扫描、元数据、封面、查询。
//!
//! 不依赖任何 GUI 类型；`desktop` 层只通过本模块的公开 API 读写书架。

pub mod book;

pub use book::{
    Book, BookFormat, BookOrigin, Bookmark, ReadingProgress, ShelfStatus, absolute_dir, hash64,
    id_for_key, mtime_secs, normalize_path_key, now_unix,
};

pub use crate::config::ReadingMode;
```

`src/lib.rs` 在 `pub mod js;` 之后按字母序插入：

```rust
pub mod library;
```

（`pub mod reader;` 在 Task 11 创建模块时再加，避免编译失败。）

`src/library/book.rs`：

```rust
//! 书籍模型：格式 / 来源 / 阅读进度 / 书签，以及路径规范化与稳定 ID。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rust_i18n::t;
use serde::{Deserialize, Serialize};

use crate::config::ReadingMode;

/// 库内书籍标识与元数据。字段顺序即 `library.json` 的键顺序。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Book {
    /// 稳定 ID：`path_key` 的 FNV-1a 64 位哈希（16 hex），用于封面文件名与 UI 键。
    #[serde(default)]
    pub id: String,
    /// 规范化查找键：绝对路径，Windows 下转小写；不展示给用户。
    #[serde(default)]
    pub path_key: String,
    /// 展示用绝对路径（保留用户原始大小写）。
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub format: BookFormat,
    #[serde(default)]
    pub origin: BookOrigin,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub intro: Option<String>,
    /// 封面缓存文件名（相对 `covers_dir`，形如 `<id>-<内容哈希>.<ext>`）；None = 生成封面。
    #[serde(default)]
    pub cover_file: Option<String>,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub file_mtime: i64,
    #[serde(default)]
    pub added_unix: i64,
    #[serde(default)]
    pub last_opened_unix: Option<i64>,
    /// 用户手动改过元数据 → 扫描时不再自动覆盖 title / author / cover。
    #[serde(default)]
    pub metadata_edited: bool,
    /// 从书架移除（仅对扫描来源生效，扫描不会再加回）。
    #[serde(default)]
    pub hidden: bool,
    /// 文件当前不存在。运行时由扫描更新，持久化便于离线展示。
    #[serde(default)]
    pub missing: bool,
    /// TXT 编码覆盖；None = 自动探测。
    #[serde(default)]
    pub encoding: Option<String>,
    /// 首次成功打开后写入。
    #[serde(default)]
    pub chapter_count: Option<u32>,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub progress: ReadingProgress,
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
}

impl Book {
    /// 由磁盘文件构造新记录。
    ///
    /// # Errors
    ///
    /// 文件不存在 / 不是普通文件 / 后缀不支持时返回可直接进 toast 的文案：
    /// 后两种走 `t!` 本地化，IO 失败透传系统原始文案。
    #[tracing::instrument(name = "library::Book::from_file", skip_all, fields(path = %path.display()))]
    pub fn from_file(path: &Path, origin: BookOrigin) -> Result<Self, String> {
        let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
        if !metadata.is_file() {
            return Err(t!("Library.error.not_a_file").to_string());
        }
        let format = BookFormat::from_path(path)
            .ok_or_else(|| t!("Library.error.unsupported_format").to_string())?;
        let path_key = normalize_path_key(path);
        Ok(Self {
            id: id_for_key(&path_key),
            path_key,
            path: display_path(path),
            format,
            origin,
            title: file_stem(path),
            author: String::new(),
            intro: None,
            cover_file: None,
            size_bytes: metadata.len(),
            file_mtime: mtime_secs(&metadata),
            added_unix: now_unix(),
            last_opened_unix: None,
            metadata_edited: false,
            hidden: false,
            missing: false,
            encoding: None,
            chapter_count: None,
            favorite: false,
            progress: ReadingProgress::default(),
            bookmarks: Vec::new(),
        })
    }

    /// 派生状态（不持久化）。
    #[must_use]
    pub fn status(&self) -> ShelfStatus {
        if self.progress.finished {
            ShelfStatus::Finished
        } else if self.progress.updated_unix > 0 {
            ShelfStatus::Reading
        } else {
            ShelfStatus::Unread
        }
    }

    /// 状态文案 key（`Library.status.*`），由 UI 层翻译。
    #[must_use]
    pub const fn status_key(&self) -> &'static str {
        match self.status() {
            ShelfStatus::Unread => "Library.status.unread",
            ShelfStatus::Reading => "Library.status.reading",
            ShelfStatus::Finished => "Library.status.finished",
        }
    }
}

/// 书籍格式。其他格式首版只展示、不在应用内阅读。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BookFormat {
    /// 纯文本。
    #[default]
    Txt,
    /// EPUB 电子书。
    Epub,
    /// Markdown。
    Markdown,
    /// HTML / HTM。
    Html,
    /// ZIP 打包的 HTML。
    Zip,
    /// PDF。
    Pdf,
}

impl BookFormat {
    /// `txt` / `epub` / `md|markdown` / `html|htm` / `zip` / `pdf`；未知后缀返回 `None`。
    ///
    /// 白名单 [`crate::core::library::SUPPORTED_LIBRARY_EXTS`] 只决定「扫描时收哪些文件」，
    /// 这里额外接受 `htm` / `markdown` 两个别名，供导入与重新定位用。
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "txt" => Some(Self::Txt),
            "epub" => Some(Self::Epub),
            "md" | "markdown" => Some(Self::Markdown),
            "html" | "htm" => Some(Self::Html),
            "zip" => Some(Self::Zip),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }

    /// 取路径后缀并按 [`Self::from_extension`] 解析。
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension().and_then(|value| value.to_str())?;
        Self::from_extension(ext)
    }

    /// 首版只有 TXT / EPUB 能在应用内阅读。
    #[must_use]
    pub const fn is_readable(self) -> bool {
        matches!(self, Self::Txt | Self::Epub)
    }

    /// 徽标与文件对话框过滤用的标签（技术名词，不翻译）。
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Txt => "TXT",
            Self::Epub => "EPUB",
            Self::Markdown => "MD",
            Self::Html => "HTML",
            Self::Zip => "ZIP",
            Self::Pdf => "PDF",
        }
    }
}

/// 书籍来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BookOrigin {
    /// 下载目录扫描到的文件。
    #[default]
    Downloaded,
    /// 用户从任意位置导入（只记录路径，不复制文件）。
    Imported,
}

/// 书架派生状态（不持久化）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShelfStatus {
    /// 从未打开。
    Unread,
    /// 读过但没读完。
    Reading,
    /// 已读完。
    Finished,
}

/// 阅读进度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingProgress {
    /// 0-based 章节下标。
    #[serde(default)]
    pub chapter: usize,
    /// 章内相对进度 0.0..=1.0。滚动 = offset/max_offset；翻页 = page_index/(page_count-1)。
    #[serde(default)]
    pub chapter_ratio: f32,
    /// 全书百分比 0..=100；`chapter_count` 未知时为 0。
    #[serde(default)]
    pub percent: f32,
    /// 上次阅读模式，重开时优先恢复。
    #[serde(default)]
    pub mode: ReadingMode,
    #[serde(default)]
    pub updated_unix: i64,
    #[serde(default)]
    pub finished: bool,
}

impl Default for ReadingProgress {
    fn default() -> Self {
        Self {
            chapter: 0,
            chapter_ratio: 0.0,
            percent: 0.0,
            mode: ReadingMode::Scroll,
            updated_unix: 0,
            finished: false,
        }
    }
}

/// 章节内书签。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// `<chapter>-<created_unix_millis>`。
    #[serde(default)]
    pub id: String,
    /// 0-based 章节下标。
    #[serde(default)]
    pub chapter: usize,
    /// 章内相对进度。
    #[serde(default)]
    pub chapter_ratio: f32,
    /// 添加时当前段落前 40 个字符。
    #[serde(default)]
    pub excerpt: String,
    #[serde(default)]
    pub created_unix: i64,
}

/// FNV-1a 64 位哈希：稳定、跨平台、无外部依赖。
#[must_use]
pub fn hash64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// `path_key` → 16 位小写 hex 的稳定 ID。
#[must_use]
pub fn id_for_key(path_key: &str) -> String {
    format!("{:016x}", hash64(path_key.as_bytes()))
}

/// 规范化路径键：优先 `canonicalize`，失败退回「绝对路径」，Windows 下转小写。
#[must_use]
pub fn normalize_path_key(path: &Path) -> String {
    let absolute = std::fs::canonicalize(path).unwrap_or_else(|_| absolute_path(path));
    let text = absolute.to_string_lossy().into_owned();
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    }
}

/// 当前 Unix 秒。
#[must_use]
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs().cast_signed())
}

/// 文件修改时间（Unix 秒）；读不到时为 0。
#[must_use]
pub fn mtime_secs(metadata: &std::fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_secs().cast_signed())
}

/// 把可能是相对路径的目录转成绝对路径；**不** `canonicalize`（目录可能还不存在，
/// 而 `canonicalize` 对不存在的路径会失败）。
#[must_use]
pub fn absolute_dir(path: &Path) -> PathBuf {
    absolute_path(path)
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
}

fn display_path(path: &Path) -> String {
    absolute_path(path).to_string_lossy().into_owned()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}
```

注意：`ReadingMode` 继续在 `library/mod.rs` re-export（`pub use crate::config::ReadingMode;`），不要在 `book.rs` 里重复 `use` 后又 `pub use`，否则 clippy `pub_use` 会报重复导入 —— 直接在 `mod.rs` 一处导出。

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib library::book:: -- --nocapture`
Expected: PASS（6 个测试）。

- [ ] **Step 5: 提交**

```bash
git add src/library src/lib.rs
git commit -m "feat(library): 书籍模型与路径 ID"
```

---

### Task 3: `library::query` 书架查询

**Files:**
- Create: `src/library/query.rs`
- Modify: `src/library/mod.rs`（已在 Task 2 写好 `pub mod query;`）

**Interfaces:**
- Consumes: `Book` / `ShelfStatus`。
- Produces: `ShelfStatusFilter`（`Default = All`）、`ShelfSort`（`Default = LastOpened`）、`ShelfQuery`（`Default`）、`visible_books(&[Book], &ShelfQuery) -> Vec<&Book>`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::library::book::{BookOrigin, BookFormat, ReadingProgress};

    fn book(id: &str, title: &str, author: &str, opened: Option<i64>) -> Book {
        Book {
            id: id.to_string(),
            path_key: id.to_string(),
            path: format!("C:/books/{id}.txt"),
            format: BookFormat::Txt,
            origin: BookOrigin::Downloaded,
            title: title.to_string(),
            author: author.to_string(),
            intro: None,
            cover_file: None,
            size_bytes: 100,
            file_mtime: 0,
            added_unix: 0,
            last_opened_unix: opened,
            metadata_edited: false,
            hidden: false,
            missing: false,
            encoding: None,
            chapter_count: None,
            favorite: false,
            progress: ReadingProgress::default(),
            bookmarks: Vec::new(),
        }
    }

    #[test]
    fn text_matches_title_author_and_file_name() {
        let mut b = book("id1", "斗破苍穹", "天蚕土豆", None);
        let query = ShelfQuery { text: "土豆".to_string(), ..ShelfQuery::default() };
        assert!(query.matches(&b));
        let query = ShelfQuery { text: "id1".to_string(), ..ShelfQuery::default() };
        assert!(query.matches(&b));
        let query = ShelfQuery { text: "斗破".to_string(), ..ShelfQuery::default() };
        assert!(query.matches(&b));
        let query = ShelfQuery { text: "不存在".to_string(), ..ShelfQuery::default() };
        assert!(!query.matches(&b));
        b.author.clear();
        assert!(!ShelfQuery { text: "土豆".into(), ..ShelfQuery::default() }.matches(&b));
    }

    #[test]
    fn status_and_favorite_filters_combine() {
        let mut unread = book("a", "A", "", None);
        unread.favorite = true;
        let mut reading = book("b", "B", "", Some(5));
        reading.progress.updated_unix = 5;
        let mut finished = book("c", "C", "", Some(5));
        finished.progress.updated_unix = 5;
        finished.progress.finished = true;
        let books = vec![unread, reading, finished];

        let ids = |status, favorite_only| {
            visible_books(
                &books,
                &ShelfQuery { status, favorite_only, ..ShelfQuery::default() },
            )
            .iter()
            .map(|b| b.id.as_str())
            .collect::<Vec<_>>()
        };

        // 默认按最近阅读排：未打开过的书（None）排最后，同值按 id 升序。
        assert_eq!(ids(ShelfStatusFilter::All, false), vec!["b", "c", "a"]);
        assert_eq!(ids(ShelfStatusFilter::Unread, false), vec!["a"]);
        assert_eq!(ids(ShelfStatusFilter::Reading, false), vec!["b"]);
        assert_eq!(ids(ShelfStatusFilter::Finished, false), vec!["c"]);
        assert_eq!(ids(ShelfStatusFilter::All, true), vec!["a"]);
    }

    #[test]
    fn hidden_books_are_excluded() {
        let shown = book("a", "A", "", None);
        let mut hidden = book("b", "B", "", None);
        hidden.hidden = true;
        hidden.favorite = true;
        let books = vec![shown, hidden];

        // 隐藏书不参与任何查询，包括「收藏」筛选。
        assert_eq!(visible_books(&books, &ShelfQuery::default()).len(), 1);
        assert!(
            visible_books(
                &books,
                &ShelfQuery { favorite_only: true, ..ShelfQuery::default() }
            )
            .is_empty()
        );
    }

    #[test]
    fn sorts_by_last_opened_then_id() {
        let books = vec![
            book("b", "B", "", Some(10)),
            book("a", "A", "", None),
            book("c", "C", "", Some(20)),
            book("d", "D", "", Some(10)),
        ];
        let sorted = visible_books(
            &books,
            &ShelfQuery { sort: ShelfSort::LastOpened, ..ShelfQuery::default() },
        );
        let ids: Vec<_> = sorted.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, vec!["c", "b", "d", "a"]);
    }

    #[test]
    fn sorts_by_title_and_size() {
        let mut books = vec![
            book("1", "beta", "", None),
            book("2", "Alpha", "", None),
            book("3", "gamma", "", None),
        ];
        books[0].size_bytes = 30;
        books[1].size_bytes = 10;
        books[2].size_bytes = 20;

        let by_title: Vec<_> = visible_books(
            &books,
            &ShelfQuery { sort: ShelfSort::Title, ..ShelfQuery::default() },
        )
        .iter()
        .map(|b| b.title.as_str())
        .collect();
        assert_eq!(by_title, vec!["Alpha", "beta", "gamma"]);

        let by_size: Vec<_> = visible_books(
            &books,
            &ShelfQuery { sort: ShelfSort::Size, ..ShelfQuery::default() },
        )
        .iter()
        .map(|b| b.id.as_str())
        .collect();
        assert_eq!(by_size, vec!["1", "3", "2"]);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib library::query:: -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

先在 `src/library/mod.rs` 加：

```rust
pub mod query;

pub use query::{ShelfQuery, ShelfSort, ShelfStatusFilter, visible_books};
```

再写 `src/library/query.rs`：

```rust
//! 书架筛选与排序：纯函数，不持有状态。

use super::book::{Book, ShelfStatus};

/// 书架状态筛选（与 `ShelfStatus` 区分：`All` 不是一种书籍状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShelfStatusFilter {
    /// 不按状态过滤。
    #[default]
    All,
    /// 未读。
    Unread,
    /// 在读。
    Reading,
    /// 已读。
    Finished,
}

impl ShelfStatusFilter {
    /// 该筛选是否接受这本书。
    #[must_use]
    pub fn accepts(self, book: &Book) -> bool {
        match self {
            Self::All => true,
            Self::Unread => book.status() == ShelfStatus::Unread,
            Self::Reading => book.status() == ShelfStatus::Reading,
            Self::Finished => book.status() == ShelfStatus::Finished,
        }
    }

    /// 文案 key（`Library.filter.*`）。
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::All => "Library.filter.all",
            Self::Unread => "Library.filter.unread",
            Self::Reading => "Library.filter.reading",
            Self::Finished => "Library.filter.finished",
        }
    }
}

/// 书架排序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShelfSort {
    /// 最近阅读，未打开的书排在后面。
    #[default]
    LastOpened,
    /// 最近添加。
    AddedAt,
    /// 书名（ASCII 大小写不敏感）。
    Title,
    /// 文件大小，从大到小。
    Size,
}

impl ShelfSort {
    /// 全部可选项，UI 下拉按此顺序渲染。
    pub const ALL: [Self; 4] = [Self::LastOpened, Self::AddedAt, Self::Title, Self::Size];

    /// 文案 key（`Library.sort.*`）。
    #[must_use]
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::LastOpened => "Library.sort.last_opened",
            Self::AddedAt => "Library.sort.added_at",
            Self::Title => "Library.sort.title",
            Self::Size => "Library.sort.size",
        }
    }
}

/// 书架视图查询：搜索 + 状态筛选 + 收藏开关 + 排序。
#[derive(Debug, Clone, Default)]
pub struct ShelfQuery {
    /// 已 `trim`；空串 = 不过滤。
    pub text: String,
    pub status: ShelfStatusFilter,
    pub favorite_only: bool,
    pub sort: ShelfSort,
}

impl ShelfQuery {
    /// 命中书名 / 作者 / 文件名（ASCII 大小写不敏感）；收藏开关要求 `book.favorite`。
    /// 隐藏书（`hidden`）任何查询都不返回 —— 「移出书架」的扫描来源靠这条生效。
    #[must_use]
    pub fn matches(&self, book: &Book) -> bool {
        if book.hidden {
            return false;
        }
        if self.favorite_only && !book.favorite {
            return false;
        }
        if !self.status.accepts(book) {
            return false;
        }
        let needle = self.text.trim();
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_ascii_lowercase();
        let file_name = std::path::Path::new(&book.path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        book.title.to_ascii_lowercase().contains(&needle)
            || book.author.to_ascii_lowercase().contains(&needle)
            || file_name.to_ascii_lowercase().contains(&needle)
    }
}

/// 过滤 + 排序，返回借用而非拷贝。
#[must_use]
pub fn visible_books<'a>(books: &'a [Book], query: &ShelfQuery) -> Vec<&'a Book> {
    let mut out: Vec<&Book> = books.iter().filter(|book| query.matches(book)).collect();
    match query.sort {
        ShelfSort::LastOpened => out.sort_by(|a, b| {
            b.last_opened_unix
                .cmp(&a.last_opened_unix)
                .then_with(|| a.id.cmp(&b.id))
        }),
        ShelfSort::AddedAt => out.sort_by(|a, b| {
            b.added_unix
                .cmp(&a.added_unix)
                .then_with(|| a.id.cmp(&b.id))
        }),
        ShelfSort::Title => out.sort_by(|a, b| {
            a.title
                .to_ascii_lowercase()
                .cmp(&b.title.to_ascii_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        }),
        ShelfSort::Size => out.sort_by(|a, b| {
            b.size_bytes
                .cmp(&a.size_bytes)
                .then_with(|| a.id.cmp(&b.id))
        }),
    }
    out
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib library::query:: -- --nocapture`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src/library/query.rs
git commit -m "feat(library): 书架筛选排序查询"
```

---

### Task 4: `library::cover` 封面缓存与生成配色

**Files:**
- Create: `src/library/cover.rs`

**Interfaces:**
- Consumes: `book::{hash64}`。
- Produces:
  - `sniff_extension(&[u8]) -> Option<&'static str>`
  - `cover_file_name(id: &str, bytes: &[u8]) -> Option<String>`
  - `write_cover(dir: &Path, id: &str, bytes: &[u8]) -> Option<String>`
  - `remove_cover_file(dir: &Path, file: Option<&str>)`
  - `cover_palette(id: &str) -> (f32, [u8; 3], [u8; 3])`
  - `rename_cover_for_relink(dir: &Path, old_id: &str, new_id: &str, file: Option<&str>) -> Option<String>`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];
    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 0, 0];
    const GIF: &[u8] = b"GIF89a";
    const WEBP: &[u8] = b"RIFF\x00\x00\x00\x00WEBPVP8 ";

    #[test]
    fn sniffs_known_magic_bytes() {
        assert_eq!(sniff_extension(PNG), Some("png"));
        assert_eq!(sniff_extension(JPEG), Some("jpg"));
        assert_eq!(sniff_extension(GIF), Some("gif"));
        assert_eq!(sniff_extension(WEBP), Some("webp"));
        assert_eq!(sniff_extension(b"not an image"), None);
        assert_eq!(sniff_extension(&[]), None);
    }

    #[test]
    fn cover_name_contains_id_and_content_hash() {
        let a = cover_file_name("abc", PNG).expect("png");
        let b = cover_file_name("abc", PNG).expect("png");
        let c = cover_file_name("abc", JPEG).expect("jpg");
        assert_eq!(a, b);
        assert!(a.starts_with("abc-"));
        assert!(a.ends_with(".png"));
        assert!(c.ends_with(".jpg"));
        assert_ne!(a.replace(".png", ""), c.replace(".jpg", ""));
        assert_eq!(cover_file_name("abc", b"nope"), None);
    }

    #[test]
    fn writes_and_removes_cover() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_cover(dir.path(), "id1", PNG).expect("written");
        assert!(dir.path().join(&file).exists());
        remove_cover_file(dir.path(), Some(&file));
        assert!(!dir.path().join(&file).exists());
        // 目录穿越文件名的调用必须被忽略
        remove_cover_file(dir.path(), Some("../evil.png"));
    }

    #[test]
    fn palette_is_deterministic() {
        let first = cover_palette("book-id");
        let second = cover_palette("book-id");
        assert_eq!(first, second);
        assert!((0.0..360.0).contains(&first.0));
        assert_ne!(first.1, first.2);
        assert_ne!(cover_palette("other-id").0, first.0);
    }

    #[test]
    fn relink_renames_cover_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = write_cover(dir.path(), "old", PNG).expect("written");
        let renamed = rename_cover_for_relink(dir.path(), "old", "new", Some(&file)).expect("renamed");
        assert!(renamed.starts_with("new-"));
        assert!(dir.path().join(&renamed).exists());
        assert!(!dir.path().join(&file).exists());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib library::cover:: -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

在 `src/library/mod.rs` 加：

```rust
pub mod cover;

pub use cover::{
    cover_file_name, cover_palette, remove_cover_file, rename_cover_for_relink, write_cover,
};
```

再写 `src/library/cover.rs`：

```rust
//! 封面缓存：魔数嗅探、带内容哈希的文件名、落盘 / 清理，以及生成封面的确定配色。

use std::path::Path;

use super::book::hash64;

/// 按魔数嗅探图片扩展名；不认识的格式返回 `None`（封面直接丢弃）。
#[must_use]
pub fn sniff_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("jpg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

/// 封面缓存文件名：`<id>-<内容哈希>.<ext>`。
///
/// 带内容哈希是为了让 GPUI 的图片缓存随内容失效：同一本书换了封面，
/// 文件名随之变化，不需要手动清缓存。
#[must_use]
pub fn cover_file_name(id: &str, bytes: &[u8]) -> Option<String> {
    let ext = sniff_extension(bytes)?;
    Some(format!("{id}-{:016x}.{ext}", hash64(bytes)))
}

/// 把封面字节写入缓存目录，返回文件名；目录不可写或格式不认识时返回 `None`。
pub fn write_cover(dir: &Path, id: &str, bytes: &[u8]) -> Option<String> {
    let file = cover_file_name(id, bytes)?;
    std::fs::create_dir_all(dir).ok()?;
    std::fs::write(dir.join(&file), bytes).ok()?;
    Some(file)
}

/// 删除一个封面缓存文件；文件名来自 `library.json`，先拒绝路径分隔符防目录穿越。
pub fn remove_cover_file(dir: &Path, file: Option<&str>) {
    let Some(file) = file else { return };
    if file.contains('/') || file.contains('\\') || file.contains("..") {
        tracing::warn!("忽略可疑的封面文件名: {file}");
        return;
    }
    let _ = std::fs::remove_file(dir.join(file));
}

/// 重新定位后把封面文件名里的 id 换掉（内容哈希与扩展名保持不变）。
///
/// 返回新文件名；没有封面或改名失败时返回 `None`。
pub fn rename_cover_for_relink(
    dir: &Path,
    old_id: &str,
    new_id: &str,
    file: Option<&str>,
) -> Option<String> {
    let file = file?;
    let suffix = file.strip_prefix(&format!("{old_id}-"))?;
    let new_file = format!("{new_id}-{suffix}");
    std::fs::rename(dir.join(file), dir.join(&new_file)).ok()?;
    Some(new_file)
}

/// 生成封面的确定配色：`(色相角度, 顶部 RGB, 底部 RGB)`。
///
/// 色相由书籍 ID 哈希决定，同一本书任何一次渲染颜色都相同；两段色同色系、明度不同，
/// 保证白色首字在两段上都可读。
#[must_use]
pub fn cover_palette(id: &str) -> (f32, [u8; 3], [u8; 3]) {
    let hue = (hash64(id.as_bytes()) % 360) as f32;
    let top = hsl_to_rgb(hue, 0.46, 0.58);
    let bottom = hsl_to_rgb((hue + 18.0) % 360.0, 0.52, 0.36);
    (hue, top, bottom)
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = hue / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = lightness - c / 2.0;
    [r1, g1, b1].map(|value| ((value + m).clamp(0.0, 1.0) * 255.0).round() as u8)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib library::cover:: -- --nocapture`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src/library/cover.rs
git commit -m "feat(library): 封面缓存与生成配色"
```

---

### Task 5: `tools::load::epub::read_metadata`

**Files:**
- Modify: `src/tools/load/epub.rs`

**Interfaces:**
- Consumes: 现有 `ZipArchive`、`manifest_items` / `spine_ids` / `cover_manifest_id` / `dc_text` / `resolve_path` / `read_entry`。
- Produces:
  - `pub struct EpubMetadata { title, author, intro, language: Option<String>, cover: Option<Vec<u8>> }`
  - `pub fn read_metadata(path: &Path) -> Result<EpubMetadata, ToolError>`
  - 私有 `struct Package { base: String, opf: String, items: Vec<ManifestItem> }` + `fn open_package(...)`

- [ ] **Step 1: 写失败测试**

在 `src/tools/load/epub.rs` 的 `mod tests` 里追加（`write_epub` 已存在）：

```rust
    #[test]
    fn read_metadata_reads_opf_without_touching_spine() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("书.epub");
        write_epub(&path);

        let meta = read_metadata(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("测试书"));
        assert_eq!(meta.author.as_deref(), Some("作者甲"));
        assert_eq!(meta.intro.as_deref(), Some("一本书"));
        assert_eq!(meta.language.as_deref(), Some("zh-CN"));
        assert!(meta.cover.as_ref().is_some_and(|bytes| !bytes.is_empty()));
    }

    #[test]
    fn read_metadata_reports_broken_container() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("坏.epub");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("mimetype", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.finish().unwrap();

        assert!(matches!(
            read_metadata(&path),
            Err(crate::tools::ToolError::EpubStructure(_))
        ));
    }
```

`use super::*;` 已在测试模块里，`read_metadata` 会随之可见。

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib tools::load::epub:: -- --nocapture`
Expected: 编译失败，`read_metadata` 不存在。

- [ ] **Step 3: 实现 package 抽取与 `read_metadata`**

在 `ManifestItem` 之后追加：

```rust
/// 只读元数据的结果；封面是原始字节，由调用方决定落盘位置。
#[derive(Debug, Clone, Default)]
pub struct EpubMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub language: Option<String>,
    pub cover: Option<Vec<u8>>,
}

/// container.xml + OPF 解析后的公共部分，正文加载与元数据读取共用。
struct Package {
    base: String,
    opf: String,
    items: Vec<ManifestItem>,
}

/// 读 container.xml → OPF → manifest。DRM / 结构损坏在这里报错。
fn open_package(archive: &mut ZipArchive<std::fs::File>) -> Result<Package, ToolError> {
    if archive.by_name("META-INF/encryption.xml").is_ok() {
        return Err(ToolError::EpubStructure(
            "不支持 DRM 加密的 EPUB".to_string(),
        ));
    }
    let container =
        String::from_utf8_lossy(&read_entry(archive, "META-INF/container.xml")?).into_owned();
    let opf_path = ROOTFILE_RE
        .captures(&container)
        .and_then(|cap| attr(&cap[0], "full-path"))
        .ok_or_else(|| ToolError::EpubStructure("container.xml 缺少 rootfile".to_string()))?;
    let opf = String::from_utf8_lossy(&read_entry(archive, &opf_path)?).into_owned();
    let base = opf_path
        .rsplit_once('/')
        .map(|(dir, _file)| dir.to_string())
        .unwrap_or_default();
    let items = manifest_items(&opf);
    Ok(Package { base, opf, items })
}

/// 只读 EPUB 元数据与封面，不解析 spine 正文 —— 书架扫描用它，避免为了书名读整本书。
///
/// # Errors
///
/// 文件打不开、不是合法 ZIP、缺 container.xml / OPF、或带 DRM 加密时返回
/// [`ToolError::EpubStructure`] / [`ToolError::Io`]。
pub fn read_metadata(path: &Path) -> Result<EpubMetadata, ToolError> {
    let file = std::fs::File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| ToolError::EpubStructure(error.to_string()))?;
    let package = open_package(&mut archive)?;
    let cover = cover_manifest_id(&package.opf, &package.items)
        .and_then(|id| package.items.iter().find(|item| item.id == id))
        .and_then(|item| {
            let name = resolve_path(&package.base, &item.href);
            let bytes = read_entry(&mut archive, &name).ok()?;
            (!bytes.is_empty()).then_some(bytes)
        });
    Ok(EpubMetadata {
        title: dc_text(&package.opf, "title"),
        author: dc_text(&package.opf, "creator"),
        intro: dc_text(&package.opf, "description"),
        language: dc_text(&package.opf, "language"),
        cover,
    })
}
```

把 `load` 的前半段改成复用 `open_package`（行为不变）：

```rust
pub(super) fn load(path: &Path) -> Result<SourceBook, ToolError> {
    let file = std::fs::File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| ToolError::EpubStructure(error.to_string()))?;
    let package = open_package(&mut archive)?;
    let spine = spine_ids(&package.opf);
    let cover_id = cover_manifest_id(&package.opf, &package.items);

    let mut chapters = Vec::new();
    for idref in spine {
        let Some(item) = package.items.iter().find(|item| item.id == idref) else {
            continue;
        };
        if !item.media_type.contains("html") {
            continue;
        }
        let name = resolve_path(&package.base, &item.href);
        let html = String::from_utf8_lossy(&read_entry(&mut archive, &name)?).into_owned();
        let title = extract_tag_text(&html, "h1")
            .or_else(|| extract_tag_text(&html, "h2"))
            .or_else(|| extract_tag_text(&html, "title"))
            .unwrap_or_else(|| format!("第 {} 章", chapters.len() + 1));
        let mut paragraphs = html_to_paragraphs(body_fragment(&html));
        if paragraphs
            .first()
            .is_some_and(|paragraph| paragraph.trim() == title.trim())
        {
            paragraphs.remove(0);
        }
        if paragraphs.is_empty() {
            continue;
        }
        chapters.push(SourceChapter { title, paragraphs });
    }
    if chapters.is_empty() {
        return Err(ToolError::EpubStructure(
            "EPUB 中没有可读取的正文".to_string(),
        ));
    }

    let cover = cover_id
        .and_then(|id| package.items.iter().find(|item| item.id == id))
        .and_then(|item| {
            let name = resolve_path(&package.base, &item.href);
            let bytes = read_entry(&mut archive, &name).ok()?;
            (!bytes.is_empty()).then_some(bytes)
        });

    Ok(SourceBook {
        title: dc_text(&package.opf, "title").unwrap_or_else(|| stem(path)),
        author: dc_text(&package.opf, "creator").unwrap_or_default(),
        intro: dc_text(&package.opf, "description"),
        language: dc_text(&package.opf, "language").unwrap_or_else(|| "zh".to_string()),
        cover,
        chapters,
    })
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib tools::load::epub:: -- --nocapture`
Expected: PASS（原有 2 个 + 新增 2 个）。

- [ ] **Step 5: 提交**

```bash
git add src/tools/load/epub.rs
git commit -m "feat(tools): EPUB 元数据只读解析"
```

---

### Task 6: `library::meta` 元数据提取

**Files:**
- Create: `src/library/meta.rs`

**Interfaces:**
- Consumes: `BookFormat`、`tools::load::epub::read_metadata`、`tools::encoding::detect_file_encoding`。
- Produces:
  - `ExtractedMeta { title, author, intro: Option<String>, cover: Option<Vec<u8>>, encoding: Option<String> }` + `Default`。
  - `extract(path: &Path, format: BookFormat) -> ExtractedMeta`。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::Path;

    use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0];

    fn write_epub(path: &Path) {
        let zip = ZipLibrary::new().unwrap();
        let mut builder = EpubBuilder::new(zip).unwrap();
        builder.metadata("title", "元数据书名").unwrap();
        builder.metadata("author", "作者乙").unwrap();
        builder
            .add_cover_image("cover.png", PNG, "image/png")
            .unwrap();
        builder
            .add_content(
                EpubContent::new(
                    "c1.xhtml",
                    "<html><body><p>正文</p></body></html>".as_bytes(),
                )
                .title("第一章"),
            )
            .unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        builder.generate(&mut file).unwrap();
    }

    #[test]
    fn extracts_epub_metadata_and_cover() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("book.epub");
        write_epub(&path);

        let meta = extract(&path, BookFormat::Epub);
        assert_eq!(meta.title.as_deref(), Some("元数据书名"));
        assert_eq!(meta.author.as_deref(), Some("作者乙"));
        assert!(meta.cover.is_some_and(|bytes| !bytes.is_empty()));
        assert!(meta.encoding.is_none());
    }

    #[test]
    fn broken_epub_falls_back_to_file_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("坏书.epub");
        std::fs::write(&path, b"not a zip").unwrap();

        let meta = extract(&path, BookFormat::Epub);
        assert_eq!(meta.title.as_deref(), Some("坏书"));
        assert!(meta.author.is_none());
        assert!(meta.cover.is_none());
    }

    #[test]
    fn txt_reports_detected_encoding() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("小说.txt");
        std::fs::write(&path, "第一章 开始\n正文内容").unwrap();

        let meta = extract(&path, BookFormat::Txt);
        assert_eq!(meta.title.as_deref(), Some("小说"));
        assert_eq!(meta.encoding.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn other_formats_only_use_file_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("文档.pdf");
        std::fs::write(&path, b"%PDF-1.7").unwrap();

        let meta = extract(&path, BookFormat::Pdf);
        assert_eq!(meta.title.as_deref(), Some("文档"));
        assert!(meta.cover.is_none());
        assert!(meta.encoding.is_none());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib library::meta:: -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

在 `src/library/mod.rs` 加：

```rust
pub mod meta;

pub use meta::{ExtractedMeta, extract};
```

再写 `src/library/meta.rs`：

```rust
//! 元数据提取：EPUB 读 OPF，TXT 探测编码；其他格式只从文件名推断标题。
//!
//! 这里只做「没有副作用」的提取：封面字节原样返回，写盘由后台工作线程负责，
//! 这样 `drain` 回到 UI 线程时不需要碰磁盘。

use std::path::Path;

use super::book::BookFormat;

/// 提取结果；封面是原始字节。
#[derive(Debug, Clone, Default)]
pub struct ExtractedMeta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub cover: Option<Vec<u8>>,
    pub encoding: Option<String>,
}

/// 按格式提取元数据。任何失败都降级成「文件名做标题」，不返回错误 ——
/// 一本元数据坏掉的书仍然应该出现在书架上。
pub fn extract(path: &Path, format: BookFormat) -> ExtractedMeta {
    match format {
        BookFormat::Epub => extract_epub(path),
        BookFormat::Txt => ExtractedMeta {
            title: file_stem(path),
            encoding: crate::tools::encoding::detect_file_encoding(path)
                .ok()
                .map(|encoding| encoding.name().to_string()),
            ..ExtractedMeta::default()
        },
        _ => ExtractedMeta {
            title: file_stem(path),
            ..ExtractedMeta::default()
        },
    }
}

fn extract_epub(path: &Path) -> ExtractedMeta {
    match crate::tools::load::epub::read_metadata(path) {
        Ok(meta) => ExtractedMeta {
            title: meta.title,
            author: meta.author,
            intro: meta.intro,
            cover: meta.cover,
            encoding: None,
        },
        Err(error) => {
            tracing::warn!("读取 EPUB 元数据失败 {}: {error}", path.display());
            ExtractedMeta {
                title: file_stem(path),
                ..ExtractedMeta::default()
            }
        }
    }
}

fn file_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_string)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib library::meta:: -- --nocapture`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src/library/meta.rs src/library/mod.rs
git commit -m "feat(library): 书籍元数据提取"
```

---

### Task 7: `library::scan` 目录扫描

**Files:**
- Create: `src/library/scan.rs`

**Interfaces:**
- Consumes: `BookFormat::from_path`。
- Produces: `ScannedFile { path: PathBuf, size_bytes: u64, mtime: i64 }`、`scan_download_dir(&Path) -> io::Result<Vec<ScannedFile>>`（按 mtime 倒序）。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn scans_supported_files_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"abc").unwrap();
        std::fs::write(dir.path().join("b.EPUB"), b"epub").unwrap();
        std::fs::write(dir.path().join("c.docx"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("d.txt"), b"x").unwrap();

        let files = scan_download_dir(dir.path()).unwrap();
        let mut names: Vec<_> = files
            .iter()
            .map(|file| file.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["a.txt", "b.EPUB"]);
        assert_eq!(
            files
                .iter()
                .find(|file| file.path.ends_with("a.txt"))
                .unwrap()
                .size_bytes,
            3
        );
        assert!(files.iter().all(|file| file.mtime > 0));
    }

    #[test]
    fn missing_directory_is_an_error() {
        assert!(scan_download_dir(Path::new("Z:/definitely/missing/dir")).is_err());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib library::scan:: -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

在 `src/library/mod.rs` 加：

```rust
pub mod scan;

pub use scan::{ScannedFile, scan_download_dir};
```

再写 `src/library/scan.rs`：

```rust
//! 下载目录扫描：只读直接子文件，按扩展名白名单过滤。无状态、无 UI。

use std::path::{Path, PathBuf};

use super::book::{BookFormat, mtime_secs};

/// 扫描到的单个文件。
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime: i64,
}

/// 扫描目录（不递归），只保留首版书库支持的扩展名，按修改时间倒序。
///
/// # Errors
///
/// 目录不可读、或某个条目的元信息读取失败时返回 [`std::io::Error`]。
#[tracing::instrument(name = "library::scan_download_dir", skip_all, fields(dir = %dir.display()))]
pub fn scan_download_dir(dir: &Path) -> std::io::Result<Vec<ScannedFile>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() || BookFormat::from_path(&path).is_none() {
            continue;
        }
        let metadata = entry.metadata()?;
        out.push(ScannedFile {
            path,
            size_bytes: metadata.len(),
            mtime: mtime_secs(&metadata),
        });
    }
    out.sort_by_key(|file| std::cmp::Reverse(file.mtime));
    Ok(out)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib library::scan:: -- --nocapture`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src/library/scan.rs src/library/mod.rs
git commit -m "feat(library): 下载目录扫描"
```

---

### Task 8: `LibraryStore` 加载 / 保存

**Files:**
- Create: `src/library/store.rs`

**Interfaces:**
- Consumes: `Book`、`id_for_key`、`now_unix`、`db::write_atomically`、`AppError`。
- Produces:
  - `LibraryStore`（`Default`）、`LIBRARY_VERSION: u32 = 1`。
  - `LibraryStore::load(&Path) -> (Self, Option<String>)`
  - `LibraryStore::save(&self, &Path) -> AppResult<()>`
  - `books() -> &[Book]`、`get(&str) -> Option<&Book>`、`is_dirty()`、`clear_dirty()`
  - 私有 `touch()` / `normalize_ids()`，供后续任务复用。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;
    use crate::library::book::{BookFormat, BookOrigin};

    fn sample_book(dir: &std::path::Path, name: &str) -> Book {
        let path = dir.join(name);
        std::fs::write(&path, b"hello").unwrap();
        Book::from_file(&path, BookOrigin::Downloaded).unwrap()
    }

    #[test]
    fn missing_file_loads_empty_without_warning() {
        let dir = tempfile::tempdir().unwrap();
        let (store, warning) = LibraryStore::load(&dir.path().join("library.json"));
        assert!(store.books().is_empty());
        assert!(warning.is_none());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        let mut store = LibraryStore::default();
        let mut book = sample_book(dir.path(), "书.txt");
        book.favorite = true;
        book.metadata_edited = true;
        book.title = "改过的书名".to_string();
        store.books.push(book.clone());

        store.save(&file).unwrap();
        let (loaded, warning) = LibraryStore::load(&file);
        assert!(warning.is_none());
        assert_eq!(loaded.books().len(), 1);
        let restored = loaded.get(&book.id).unwrap();
        assert_eq!(restored.title, "改过的书名");
        assert_eq!(restored.format, BookFormat::Txt);
        assert!(restored.favorite);
        assert!(restored.metadata_edited);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        std::fs::write(&file, b"{ not json").unwrap();

        let (store, warning) = LibraryStore::load(&file);
        assert!(store.books().is_empty());
        assert!(warning.is_some());
        assert!(!file.exists(), "损坏文件应被改名备份");
        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("library.json.corrupt-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn duplicate_ids_are_reassigned() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        let json = r#"{
            "version": 1,
            "books": [
                {"id": "same", "path_key": "a", "title": "A"},
                {"id": "same", "path_key": "b", "title": "B"},
                {"id": "same", "path_key": "a", "title": "A2"}
            ]
        }"#;
        std::fs::write(&file, json).unwrap();

        let (store, _) = LibraryStore::load(&file);
        let ids: Vec<_> = store.books().iter().map(|book| book.id.as_str()).collect();
        assert_eq!(ids.len(), 3);
        assert_ne!(ids[0], ids[1]);
        assert_eq!(ids[0], id_for_key("a"));
        // 同一个 path_key 的重复条目走 `-<suffix>` 兜底，两条都不能丢。
        assert_eq!(ids[2], format!("{}-1", id_for_key("a")));
    }

    #[test]
    fn future_version_refuses_to_save() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        std::fs::write(&file, r#"{"version": 99, "books": []}"#).unwrap();

        let (store, warning) = LibraryStore::load(&file);
        assert!(warning.is_some());
        let before = std::fs::read_to_string(&file).unwrap();
        assert!(store.save(&file).is_err());
        // 拒写必须是真的没动磁盘：write_atomically 会先删目标再 rename。
        assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
    }

    #[test]
    fn unreadable_file_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("library.json");
        // 同名目录：读它必然失败（走读错误分支），而且无法被 rename 备份掉。
        std::fs::create_dir(&file).unwrap();

        let (store, warning) = LibraryStore::load(&file);
        assert!(warning.is_some());
        assert!(store.save(&file).is_err());
        assert!(file.is_dir(), "读失败后那份文件必须原样留着");
    }

    #[test]
    fn dirty_flag_starts_clean() {
        let dir = tempfile::tempdir().unwrap();
        let (mut store, _) = LibraryStore::load(&dir.path().join("library.json"));
        assert!(!store.is_dirty());
        store.touch();
        assert!(store.is_dirty());
        store.clear_dirty();
        assert!(!store.is_dirty());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib library::store:: -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

在 `src/library/mod.rs` 加：

```rust
pub mod store;

pub use store::{LIBRARY_VERSION, LibraryStore};
```

再写 `src/library/store.rs`：

```rust
//! 书架持久化：`library.json` 的加载 / 保存 / 损坏兜底。
//!
//! 本文件只负责「记录集合」本身；扫描合并、导入、进度等业务变更在后续任务里追加。
//! store 不启动线程、不持有 channel —— 扫描与元数据由 `desktop/model` 调度。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rust_i18n::t;
use serde::{Deserialize, Serialize};

use super::book::{Book, id_for_key, now_unix};
use crate::error::{AppError, AppResult};

/// 当前书架文件版本。
pub const LIBRARY_VERSION: u32 = 1;

/// `library.json` 的根结构。
#[derive(Debug, Serialize, Deserialize)]
struct LibraryFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    books: Vec<Book>,
}

/// 书架内存状态 + 脏标记。
#[derive(Debug, Default)]
pub struct LibraryStore {
    books: Vec<Book>,
    dirty: bool,
    /// 文件版本高于本程序支持的版本：只读，拒绝覆盖，避免把新版数据写回旧格式。
    future_version: bool,
    /// 读取失败、或损坏文件改名备份失败：磁盘上还躺着一份没备份过的文件，
    /// 而 `write_atomically` 会先删除目标再 rename，保存就等于抹掉它 —— 故一律拒写。
    readonly: bool,
}

impl LibraryStore {
    /// 读取书架文件。
    ///
    /// 返回值第二项是给用户看的提示（损坏备份 / 版本过高），`None` 表示一切正常。
    /// 损坏时把原文件改名为 `library.json.corrupt-<unix>` 再以空库启动，
    /// 保证用户数据不会被下一次保存直接抹掉。
    #[tracing::instrument(name = "library::LibraryStore::load", skip_all, fields(path = %path.display()))]
    pub fn load(path: &Path) -> (Self, Option<String>) {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (Self::default(), None);
            }
            Err(error) => {
                return (
                    Self {
                        readonly: true,
                        ..Self::default()
                    },
                    Some(
                        t!("Library.error.load_failed", detail = error.to_string()).to_string(),
                    ),
                );
            }
        };
        match serde_json::from_slice::<LibraryFile>(&bytes) {
            Ok(file) => {
                let mut store = Self {
                    books: file.books,
                    dirty: false,
                    future_version: file.version > LIBRARY_VERSION,
                };
                store.normalize_ids();
                let warning = store.future_version.then(|| {
                    t!(
                        "Library.error.future_version",
                        version = file.version,
                        supported = LIBRARY_VERSION
                    )
                    .to_string()
                });
                (store, warning)
            }
            Err(error) => {
                let backup = unique_backup_path(path);
                let rename_error = std::fs::rename(path, &backup).err();
                let mut message = t!(
                    "Library.error.corrupt_backup",
                    path = backup.display().to_string(),
                    detail = error.to_string()
                )
                .to_string();
                let backup_failed = rename_error.is_some();
                if let Some(rename_error) = rename_error {
                    message.push_str(&t!(
                        "Library.error.backup_failed",
                        detail = rename_error.to_string()
                    ));
                }
                (
                    Self {
                        readonly: backup_failed,
                        ..Self::default()
                    },
                    Some(message),
                )
            }
        }
    }

    /// 把书架写回磁盘（原子写）。
    ///
    /// # Errors
    ///
    /// 版本过高拒写、序列化失败、目录创建或写盘失败时返回 [`AppError`]。
    #[tracing::instrument(name = "library::LibraryStore::save", skip_all, fields(path = %path.display()))]
    pub fn save(&self, path: &Path) -> AppResult<()> {
        if self.readonly {
            return Err(AppError::business(
                t!("Library.error.save_refused_after_load_failure").to_string(),
            ));
        }
        if self.future_version {
            return Err(AppError::business(
                t!("Library.error.save_refused").to_string(),
            ));
        }
        let file = LibraryFile {
            version: LIBRARY_VERSION,
            books: self.books.clone(),
        };
        let json =
            serde_json::to_vec_pretty(&file).map_err(|error| AppError::internal(error.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| {
                    AppError::io_msg(&error, t!("Library.error.create_dir_failed"))
                })?;
        }
        crate::db::write_atomically(path, &json)
            .map_err(|error| AppError::io_msg(&error, t!("Library.error.write_failed")))
    }

    /// 全部书籍（含隐藏项，UI 自行过滤）。
    #[must_use]
    pub fn books(&self) -> &[Book] {
        &self.books
    }

    /// 按 ID 取书。
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Book> {
        self.books.iter().find(|book| book.id == id)
    }

    /// 是否有未落盘的变更。
    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// 保存成功后清除脏标记。
    pub const fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// 标记有变更。后续任务的写方法统一调它。
    pub(super) const fn touch(&mut self) {
        self.dirty = true;
    }

    /// 重新按 `path_key` 计算 ID，并消解重复 ID（极低概率的哈希碰撞 / 手改文件）。
    fn normalize_ids(&mut self) {
        let mut seen: HashSet<String> = HashSet::new();
        for book in &mut self.books {
            let base = if book.path_key.is_empty() {
                book.id.clone()
            } else {
                id_for_key(&book.path_key)
            };
            let base = if base.is_empty() {
                id_for_key(&format!("index:{}", seen.len()))
            } else {
                base
            };
            let mut candidate = base.clone();
            let mut suffix = 1;
            while !seen.insert(candidate.clone()) {
                candidate = format!("{base}-{suffix}");
                suffix += 1;
            }
            book.id = candidate;
        }
    }
}

/// 损坏文件的备份名：`library.json.corrupt-<unix>`，已被占用就追加 `-1`、`-2`…
///
/// 同一秒内再次损坏覆盖上一份备份就白备份了（Unix 的 `rename` 会静默替换目标）。
fn unique_backup_path(path: &Path) -> PathBuf {
    let base = path.with_extension(format!("json.corrupt-{}", now_unix()));
    let mut candidate = base.clone();
    for suffix in 1..1000 {
        if !candidate.exists() {
            return candidate;
        }
        candidate = PathBuf::from(format!("{}-{suffix}", base.display()));
    }
    base
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib library::store:: -- --nocapture`
Expected: PASS（6 个测试）。

- [ ] **Step 5: 提交**

```bash
git add src/library/store.rs src/library/mod.rs
git commit -m "feat(library): 书架持久化与损坏兜底"
```

---

### Task 9: `LibraryStore` 扫描合并 / 导入 / 元数据回写

**Files:**
- Modify: `src/library/store.rs`

**Interfaces:**
- Consumes: `ScannedFile`、`Book::from_file`、`normalize_path_key`。
- Produces:
  - `AppliedMeta { title, author, intro: Option<String>, cover_file: Option<String>, encoding: Option<String> }`（`Default`）
  - `ImportOutcome { added: usize, existing: usize, failed: Vec<(PathBuf, String)>, meta_ids: Vec<String> }`（`Default`）
  - `merge_scan(&mut self, &[ScannedFile]) -> Vec<String>`
  - `import_files(&mut self, &[PathBuf]) -> ImportOutcome`
  - `apply_meta(&mut self, id: &str, AppliedMeta)`
  - `edit_metadata(&mut self, id, title, author, cover_file: Option<String>)`
  - `mark_missing(&mut self, id, bool)`、`set_hidden(&mut self, id, bool)`、`remove(&mut self, id) -> Option<Book>`

- [ ] **Step 1: 写失败测试**

追加到 `store.rs` 的 `mod tests`：

```rust
    fn scanned(path: &std::path::Path) -> crate::library::scan::ScannedFile {
        let metadata = std::fs::metadata(path).unwrap();
        crate::library::scan::ScannedFile {
            path: path.to_path_buf(),
            size_bytes: metadata.len(),
            mtime: crate::library::book::mtime_secs(&metadata),
        }
    }

    #[test]
    fn merge_adds_new_and_keeps_progress() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();

        let meta_ids = store.merge_scan(&[scanned(&path)]);
        assert_eq!(store.books().len(), 1);
        assert_eq!(meta_ids.len(), 1);
        assert_eq!(store.books()[0].origin, BookOrigin::Downloaded);
        assert!(store.is_dirty());

        let id = store.books()[0].id.clone();
        store.books[0].progress.chapter = 3;
        store.clear_dirty();

        let again = store.merge_scan(&[scanned(&path)]);
        assert!(again.is_empty(), "文件未变不应重复提取元数据");
        assert_eq!(store.get(&id).unwrap().progress.chapter, 3);
        assert!(!store.is_dirty());
    }

    #[test]
    fn merge_marks_missing_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        store.merge_scan(&[scanned(&path)]);
        let id = store.books()[0].id.clone();

        store.merge_scan(&[]);
        assert!(store.get(&id).unwrap().missing);

        store.merge_scan(&[scanned(&path)]);
        assert!(!store.get(&id).unwrap().missing);
    }

    #[test]
    fn merge_skips_hidden_books() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        store.merge_scan(&[scanned(&path)]);
        let id = store.books()[0].id.clone();
        store.set_hidden(&id, true);

        store.merge_scan(&[scanned(&path)]);
        assert!(store.get(&id).unwrap().hidden);
    }

    #[test]
    fn imported_missing_checks_disk_not_scan_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("imported.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let outcome = store.import_files(std::slice::from_ref(&path));
        assert_eq!(outcome.added, 1);
        let id = outcome.meta_ids[0].clone();

        store.merge_scan(&[]);
        assert!(!store.get(&id).unwrap().missing, "不在下载目录不等于失效");

        std::fs::remove_file(&path).unwrap();
        store.merge_scan(&[]);
        assert!(store.get(&id).unwrap().missing);
    }

    #[test]
    fn import_counts_existing_and_failures() {
        let dir = tempfile::tempdir().unwrap();
        let ok = dir.path().join("ok.txt");
        std::fs::write(&ok, b"one").unwrap();
        let bad = dir.path().join("bad.docx");
        std::fs::write(&bad, b"x").unwrap();
        let mut store = LibraryStore::default();

        let first = store.import_files(std::slice::from_ref(&ok));
        assert_eq!((first.added, first.existing), (1, 0));

        let second = store.import_files(&[ok, bad]);
        assert_eq!((second.added, second.existing), (0, 1));
        assert_eq!(second.failed.len(), 1);
        assert_eq!(store.books().len(), 1);
    }

    #[test]
    fn apply_meta_respects_manual_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let id = store.merge_scan(&[scanned(&path)]).remove(0);

        store.apply_meta(
            &id,
            AppliedMeta {
                title: Some("自动标题".to_string()),
                author: Some("自动作者".to_string()),
                ..AppliedMeta::default()
            },
        );
        assert_eq!(store.get(&id).unwrap().title, "自动标题");

        store.edit_metadata(&id, "手改标题", "手改作者", None);
        store.apply_meta(
            &id,
            AppliedMeta {
                title: Some("再次自动".to_string()),
                ..AppliedMeta::default()
            },
        );
        let book = store.get(&id).unwrap();
        assert_eq!(book.title, "手改标题");
        assert_eq!(book.author, "手改作者");
        assert!(book.metadata_edited);
    }

    #[test]
    fn remove_drops_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let id = store.merge_scan(&[scanned(&path)]).remove(0);
        assert!(store.remove(&id).is_some());
        assert!(store.books().is_empty());
        assert!(store.remove(&id).is_none());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib library::store::tests::merge -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

在 `store.rs` 顶部 use 补：

```rust
use std::path::PathBuf;

use super::book::{Book, BookFormat, BookOrigin, id_for_key, normalize_path_key, now_unix};
use super::scan::ScannedFile;
```

在 `LibraryStore` 之前追加类型：

```rust
/// 后台工作线程把封面写入 `covers_dir` 之后的结果；drain 只做内存回写，不碰磁盘。
#[derive(Debug, Clone, Default)]
pub struct AppliedMeta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub cover_file: Option<String>,
    pub encoding: Option<String>,
}

/// 一次导入（可能多选）的结果统计。
#[derive(Debug, Default)]
pub struct ImportOutcome {
    pub added: usize,
    pub existing: usize,
    /// 失败项及错误文案（文件不存在 / 不支持的后缀）。
    pub failed: Vec<(PathBuf, String)>,
    /// 需要后台提取元数据的书籍 id。
    pub meta_ids: Vec<String>,
}
```

在 `impl LibraryStore` 内追加方法：

```rust
    /// 合并一次目录扫描结果，返回需要提取元数据的书籍 id。
    ///
    /// 规则见设计文档 3.1：新文件入队、变化文件按需重新提取、隐藏项跳过、
    /// 扫描结果里缺席的下载书标失效（记录保留，文件放回后自动恢复）。
    pub fn merge_scan(&mut self, files: &[ScannedFile]) -> Vec<String> {
        let mut meta_ids = Vec::new();
        let present: HashSet<String> = files
            .iter()
            .map(|file| normalize_path_key(&file.path))
            .collect();

        for file in files {
            let key = normalize_path_key(&file.path);
            if let Some(book) = self
                .books
                .iter_mut()
                .find(|book| book.path_key == key)
            {
                if book.hidden {
                    continue;
                }
                let changed = book.size_bytes != file.size_bytes
                    || book.file_mtime != file.mtime
                    || book.missing;
                if changed {
                    book.size_bytes = file.size_bytes;
                    book.file_mtime = file.mtime;
                    book.missing = false;
                    self.dirty = true;
                    if !book.metadata_edited {
                        meta_ids.push(book.id.clone());
                    }
                }
                continue;
            }
            match Book::from_file(&file.path, BookOrigin::Downloaded) {
                Ok(book) if !self.books.iter().any(|other| other.path_key == book.path_key) => {
                    meta_ids.push(book.id.clone());
                    self.books.push(book);
                    self.dirty = true;
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!("扫描到的文件无法建书 {}: {error}", file.path.display());
                }
            }
        }

        for book in &mut self.books {
            let missing = match book.origin {
                BookOrigin::Downloaded => !present.contains(&book.path_key),
                BookOrigin::Imported => !Path::new(&book.path).exists(),
            };
            if book.missing != missing {
                book.missing = missing;
                self.dirty = true;
            }
        }
        meta_ids
    }

    /// 导入一批文件（只记录路径，不复制）。已存在则取消隐藏并刷新文件信息，保留进度。
    pub fn import_files(&mut self, paths: &[PathBuf]) -> ImportOutcome {
        let mut outcome = ImportOutcome::default();
        for path in paths {
            if BookFormat::from_path(path).is_none() {
                outcome
                    .failed
                    .push((path.clone(), t!("Library.error.unsupported_format").to_string()));
                continue;
            }
            let book = match Book::from_file(path, BookOrigin::Imported) {
                Ok(book) => book,
                Err(error) => {
                    outcome.failed.push((path.clone(), error));
                    continue;
                }
            };
            if let Some(existing) = self
                .books
                .iter_mut()
                .find(|other| other.path_key == book.path_key)
            {
                existing.hidden = false;
                existing.missing = false;
                existing.size_bytes = book.size_bytes;
                existing.file_mtime = book.file_mtime;
                outcome.existing += 1;
                self.dirty = true;
                continue;
            }
            outcome.meta_ids.push(book.id.clone());
            self.books.push(book);
            outcome.added += 1;
            self.dirty = true;
        }
        outcome
    }

    /// 回写后台提取的元数据；手动编辑过的书不接受自动覆盖。
    pub fn apply_meta(&mut self, id: &str, meta: AppliedMeta) {
        let Some(book) = self.books.iter_mut().find(|book| book.id == id) else {
            return;
        };
        if book.metadata_edited {
            return;
        }
        let mut changed = false;
        if let Some(title) = meta.title.filter(|title| !title.trim().is_empty())
            && book.title != title
        {
            book.title = title;
            changed = true;
        }
        if let Some(author) = meta.author
            && book.author != author
        {
            book.author = author;
            changed = true;
        }
        if let Some(intro) = meta.intro
            && book.intro.as_deref() != Some(intro.as_str())
        {
            book.intro = Some(intro);
            changed = true;
        }
        if let Some(cover_file) = meta.cover_file
            && book.cover_file.as_deref() != Some(cover_file.as_str())
        {
            book.cover_file = Some(cover_file);
            changed = true;
        }
        if let Some(encoding) = meta.encoding
            && book.encoding.is_none()
        {
            book.encoding = Some(encoding);
            changed = true;
        }
        if changed {
            self.dirty = true;
        }
    }

    /// 手动编辑元数据；`cover_file = None` 表示移除封面。
    pub fn edit_metadata(&mut self, id: &str, title: &str, author: &str, cover_file: Option<String>) {
        let Some(book) = self.books.iter_mut().find(|book| book.id == id) else {
            return;
        };
        book.title = title.trim().to_string();
        book.author = author.trim().to_string();
        book.cover_file = cover_file;
        book.metadata_edited = true;
        self.dirty = true;
    }

    /// 更新失效标记。
    pub fn mark_missing(&mut self, id: &str, missing: bool) {
        if let Some(book) = self.books.iter_mut().find(|book| book.id == id)
            && book.missing != missing
        {
            book.missing = missing;
            self.dirty = true;
        }
    }

    /// 移出 / 放回书架（仅改变可见性，不碰磁盘）。
    pub fn set_hidden(&mut self, id: &str, hidden: bool) {
        if let Some(book) = self.books.iter_mut().find(|book| book.id == id)
            && book.hidden != hidden
        {
            book.hidden = hidden;
            self.dirty = true;
        }
    }

    /// 彻底删除记录（删除磁盘文件后调用）。
    pub fn remove(&mut self, id: &str) -> Option<Book> {
        let index = self.books.iter().position(|book| book.id == id)?;
        self.dirty = true;
        Some(self.books.remove(index))
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib library::store:: -- --nocapture`
Expected: PASS（13 个测试）。

- [ ] **Step 5: 提交**

```bash
git add src/library/store.rs
git commit -m "feat(library): 扫描合并、导入与元数据回写"
```

---

### Task 10: `LibraryStore` 进度 / 书签 / 收藏 / 重新定位

**Files:**
- Modify: `src/library/store.rs`, `src/library/book.rs`

**Interfaces:**
- Consumes: `ReadingMode`、`ReadingProgress`、`Bookmark`。
- Produces:
  - `Bookmark::new(chapter, chapter_ratio, excerpt) -> Bookmark`
  - `relink(&mut self, old_id: &str, new_path: &Path) -> AppResult<String>`
  - `set_favorite`、`record_progress(id, chapter, ratio, mode, finished)`、`add_bookmark` / `remove_bookmark`

- [ ] **Step 1: 写失败测试**

`book.rs` 测试追加：

```rust
    #[test]
    fn bookmark_ids_are_unique_per_creation() {
        let first = Bookmark::new(2, 0.5, "摘录".to_string());
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = Bookmark::new(2, 0.5, "摘录".to_string());
        assert_eq!(first.chapter, 2);
        assert_ne!(first.id, second.id);
        assert!(first.created_unix > 0);
    }
```

`store.rs` 测试追加：

```rust
    #[test]
    fn progress_computes_percent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let id = store.merge_scan(&[scanned(&path)]).remove(0);
        store.books[0].chapter_count = Some(10);

        store.record_progress(&id, 4, 0.5, crate::config::ReadingMode::Page, false);
        let book = store.get(&id).unwrap();
        assert_eq!(book.progress.chapter, 4);
        assert!((book.progress.percent - 45.0).abs() < 0.01);
        assert_eq!(book.progress.mode, crate::config::ReadingMode::Page);
        assert!(book.last_opened_unix.is_some());
        assert!(!book.progress.finished);

        store.record_progress(&id, 9, 1.0, crate::config::ReadingMode::Scroll, true);
        assert!((store.get(&id).unwrap().progress.percent - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn bookmark_dedupes_same_spot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let id = store.merge_scan(&[scanned(&path)]).remove(0);

        assert!(store.add_bookmark(&id, crate::library::Bookmark::new(1, 0.4, "甲".into())));
        assert!(!store.add_bookmark(&id, crate::library::Bookmark::new(1, 0.402, "乙".into())));
        assert!(store.add_bookmark(&id, crate::library::Bookmark::new(1, 0.6, "丙".into())));
        assert_eq!(store.get(&id).unwrap().bookmarks.len(), 2);

        let bookmark_id = store.get(&id).unwrap().bookmarks[0].id.clone();
        store.remove_bookmark(&id, &bookmark_id);
        assert_eq!(store.get(&id).unwrap().bookmarks.len(), 1);
    }

    #[test]
    fn relink_keeps_progress_and_rejects_format_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let old_path = dir.path().join("old.txt");
        std::fs::write(&old_path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let id = store.merge_scan(&[scanned(&old_path)]).remove(0);
        store.books[0].progress.chapter = 7;
        store.books[0].missing = true;

        let wrong = dir.path().join("new.epub");
        std::fs::write(&wrong, b"x").unwrap();
        assert!(store.relink(&id, &wrong).is_err());

        let new_path = dir.path().join("new.txt");
        std::fs::write(&new_path, b"two").unwrap();
        let new_id = store.relink(&id, &new_path).unwrap();
        assert_ne!(new_id, id);
        let book = store.get(&new_id).unwrap();
        assert_eq!(book.progress.chapter, 7);
        assert!(!book.missing);
        assert_eq!(book.size_bytes, 3);
    }

    #[test]
    fn favorite_toggles_and_marks_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"one").unwrap();
        let mut store = LibraryStore::default();
        let id = store.merge_scan(&[scanned(&path)]).remove(0);
        store.clear_dirty();

        store.set_favorite(&id, true);
        assert!(store.get(&id).unwrap().favorite);
        assert!(store.is_dirty());
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib library::store::tests::progress -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

`book.rs` 的 `Bookmark` 下追加：

```rust
impl Bookmark {
    /// 用当前时间生成唯一 ID（`<chapter>-<毫秒时间戳>`）。
    #[must_use]
    pub fn new(chapter: usize, chapter_ratio: f32, excerpt: String) -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_millis());
        Self {
            id: format!("{chapter}-{millis}"),
            chapter,
            chapter_ratio,
            excerpt,
            created_unix: now_unix(),
        }
    }
}
```

`store.rs` 顶部 use 补：

```rust
use super::book::{Bookmark, ReadingProgress};
use crate::config::ReadingMode;
```

在 `impl LibraryStore` 内追加：

```rust
    /// 收藏 / 取消收藏。
    pub fn set_favorite(&mut self, id: &str, favorite: bool) {
        if let Some(book) = self.books.iter_mut().find(|book| book.id == id)
            && book.favorite != favorite
        {
            book.favorite = favorite;
            self.dirty = true;
        }
    }

    /// 记录阅读位置。比例统一夹到 0..=1，`finished` 时百分比直接 100。
    ///
    /// `percent = (chapter + ratio) / chapter_count * 100`；章节数未知时记 0，
    /// 等阅读器首次打开成功写回 `chapter_count` 后自然恢复。
    pub fn record_progress(
        &mut self,
        id: &str,
        chapter: usize,
        chapter_ratio: f32,
        mode: ReadingMode,
        finished: bool,
    ) {
        let Some(book) = self.books.iter_mut().find(|book| book.id == id) else {
            return;
        };
        let ratio = chapter_ratio.clamp(0.0, 1.0);
        let percent = if finished {
            100.0
        } else {
            book.chapter_count.filter(|count| *count > 0).map_or(0.0, |count| {
                ((chapter as f32 + ratio) / count as f32 * 100.0).clamp(0.0, 100.0)
            })
        };
        book.progress = ReadingProgress {
            chapter,
            chapter_ratio: ratio,
            percent,
            mode,
            updated_unix: now_unix(),
            finished,
        };
        book.last_opened_unix = Some(now_unix());
        self.dirty = true;
    }

    /// 添加书签；同一章同一相对位置（误差 < 0.5%）视为重复，返回 `false`。
    pub fn add_bookmark(&mut self, id: &str, bookmark: Bookmark) -> bool {
        let Some(book) = self.books.iter_mut().find(|book| book.id == id) else {
            return false;
        };
        let duplicate = book.bookmarks.iter().any(|existing| {
            existing.chapter == bookmark.chapter
                && (existing.chapter_ratio - bookmark.chapter_ratio).abs() < 0.005
        });
        if duplicate {
            return false;
        }
        book.bookmarks.push(bookmark);
        book.bookmarks.sort_by(|a, b| {
            a.chapter
                .cmp(&b.chapter)
                .then_with(|| a.chapter_ratio.total_cmp(&b.chapter_ratio))
        });
        self.dirty = true;
        true
    }

    /// 删除书签。
    pub fn remove_bookmark(&mut self, id: &str, bookmark_id: &str) {
        let Some(book) = self.books.iter_mut().find(|book| book.id == id) else {
            return;
        };
        let before = book.bookmarks.len();
        book.bookmarks.retain(|bookmark| bookmark.id != bookmark_id);
        if book.bookmarks.len() != before {
            self.dirty = true;
        }
    }

    /// 重新定位到新文件：重算 ID / 路径键，保留进度与书签。返回新 ID。
    ///
    /// 封面文件改名由桌面层负责（store 不持有 `covers_dir`）。
    ///
    /// # Errors
    ///
    /// 书籍不存在、格式与原来不同、新文件已在书架中时返回 [`AppError`]。
    pub fn relink(&mut self, old_id: &str, new_path: &Path) -> AppResult<String> {
        let index = self
            .books
            .iter()
            .position(|book| book.id == old_id)
            .ok_or_else(|| AppError::not_found(t!("Library.error.book_not_found").to_string()))?;
        let format = BookFormat::from_path(new_path).ok_or_else(|| {
            AppError::business(t!("Library.error.unsupported_format").to_string())
        })?;
        if format != self.books[index].format {
            return Err(AppError::business(
                t!("Library.error.format_mismatch").to_string(),
            ));
        }
        let fresh = Book::from_file(new_path, self.books[index].origin)
            .map_err(AppError::business)?;
        if self
            .books
            .iter()
            .any(|book| book.path_key == fresh.path_key && book.id != old_id)
        {
            return Err(AppError::conflict(
                t!("Library.error.already_on_shelf").to_string(),
            ));
        }
        let book = &mut self.books[index];
        book.id = fresh.id;
        book.path_key = fresh.path_key;
        book.path = fresh.path;
        book.size_bytes = fresh.size_bytes;
        book.file_mtime = fresh.file_mtime;
        book.missing = false;
        book.hidden = false;
        let new_id = book.id.clone();
        self.dirty = true;
        Ok(new_id)
    }
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib library:: -- --nocapture`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add src/library/store.rs src/library/book.rs
git commit -m "feat(library): 阅读进度、书签与重新定位"
```

---

### Task 11: `reader::content` 内容加载

**Files:**
- Create: `src/reader/mod.rs`, `src/reader/content.rs`
- Modify: `src/lib.rs`（`pub mod reader;`）

**Interfaces:**
- Consumes: `library::BookFormat`、`tools::load::{load_path, LoadOptions}`、`tools::encoding::{parse_encoding, detect_file_encoding}`。
- Produces:
  - `ReaderBook { title, author, chapters: Vec<ReaderChapter>, encoding: Option<String> }`
  - `ReaderChapter { title, paragraphs: Vec<String> }`
  - `ReaderError`（`thiserror`，带 `message_key()` / `detail()`）
  - `load_book(path: &Path, format: BookFormat, encoding: Option<&str>) -> Result<ReaderBook, ReaderError>`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    const TXT: &str = "第一章 开始\n正文一\n\n第二章 继续\n正文二\n";

    #[test]
    fn loads_utf8_txt_with_chapters() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("书.txt");
        std::fs::write(&path, TXT).unwrap();

        let book = load_book(&path, BookFormat::Txt, None).unwrap();
        assert_eq!(book.title, "书");
        assert_eq!(book.chapters.len(), 2);
        assert!(book.chapters[0].title.contains("第一章"));
        assert_eq!(book.chapters[0].paragraphs, vec!["正文一"]);
        assert_eq!(book.encoding.as_deref(), Some("UTF-8"));
    }

    #[test]
    fn decodes_gbk_txt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gbk.txt");
        let (bytes, _, _) = encoding_rs::GBK.encode(TXT);
        std::fs::write(&path, &bytes).unwrap();

        let book = load_book(&path, BookFormat::Txt, None).unwrap();
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].paragraphs, vec!["正文一"]);
        assert!(book.encoding.is_some());
    }

    #[test]
    fn explicit_encoding_overrides_detection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gbk.txt");
        let (bytes, _, _) = encoding_rs::GBK.encode(TXT);
        std::fs::write(&path, &bytes).unwrap();

        let book = load_book(&path, BookFormat::Txt, Some("gbk")).unwrap();
        assert_eq!(book.chapters[0].paragraphs, vec!["正文一"]);
        assert_eq!(book.encoding.as_deref(), Some("GBK"));
    }

    #[test]
    fn rejects_unknown_encoding_label() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, TXT).unwrap();

        assert!(matches!(
            load_book(&path, BookFormat::Txt, Some("klingon")),
            Err(ReaderError::InvalidEncoding(_))
        ));
    }

    #[test]
    fn rejects_unsupported_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let pdf = dir.path().join("a.pdf");
        std::fs::write(&pdf, b"%PDF-1.7").unwrap();
        assert!(matches!(
            load_book(&pdf, BookFormat::Pdf, None),
            Err(ReaderError::UnsupportedFormat(BookFormat::Pdf))
        ));
        assert!(matches!(
            load_book(&dir.path().join("nope.txt"), BookFormat::Txt, None),
            Err(ReaderError::MissingFile)
        ));
    }

    #[test]
    fn empty_txt_is_no_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, "").unwrap();
        assert!(matches!(
            load_book(&path, BookFormat::Txt, None),
            Err(ReaderError::NoContent)
        ));
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib reader::content:: -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

`src/reader/mod.rs`（本任务只声明已有模块）：

```rust
//! 阅读器核心：内容加载、分页排版、书内搜索。全部 UI 中立。

pub mod content;

pub use content::{ReaderBook, ReaderChapter, ReaderError, load_book};
```

`src/lib.rs` 在 `pub mod parser;` 之后插入：

```rust
pub mod reader;
```

`src/reader/content.rs`：

```rust
//! 阅读内容加载：TXT / EPUB → 章节化纯文本。首版只放行这两种格式。

use std::path::Path;

use super::super::library::BookFormat;
use crate::tools::ToolError;
use crate::tools::load::{LoadOptions, load_path};

/// 阅读器内的整本书。
#[derive(Debug, Clone)]
pub struct ReaderBook {
    pub title: String,
    pub author: String,
    pub chapters: Vec<ReaderChapter>,
    /// TXT 实际使用的编码名；EPUB 为 `None`。
    pub encoding: Option<String>,
}

/// 单章正文。
#[derive(Debug, Clone)]
pub struct ReaderChapter {
    pub title: String,
    pub paragraphs: Vec<String>,
}

/// 打开书籍时的错误。
#[derive(Debug, thiserror::Error)]
pub enum ReaderError {
    /// 格式不在首版可读白名单内。
    #[error("暂不支持在应用内阅读该格式")]
    UnsupportedFormat(BookFormat),
    /// 文件已被移动或删除。
    #[error("文件已被移动或删除")]
    MissingFile,
    /// 读盘失败。
    #[error("读取文件失败: {0}")]
    Io(#[from] std::io::Error),
    /// 解码失败。
    #[error("无法解码: {0}")]
    Decode(BookFormat, String),
    /// EPUB 结构 / DRM 问题。
    #[error("EPUB 文件无法打开: {0}")]
    Epub(String),
    /// 文件没有正文。
    #[error("文件中没有可阅读的正文")]
    NoContent,
    /// 用户指定的编码标签无法识别。
    #[error("未知编码: {0}")]
    InvalidEncoding(String),
}

impl ReaderError {
    /// 对应的 i18n key（`Reader.error.*`）。
    #[must_use]
    pub const fn message_key(&self) -> &'static str {
        match self {
            Self::UnsupportedFormat(_) => "Reader.error.unsupported",
            Self::MissingFile => "Reader.error.missing",
            Self::Io(_) => "Reader.error.io",
            Self::Decode(_, _) => "Reader.error.decode",
            Self::Epub(_) => "Reader.error.epub",
            Self::NoContent => "Reader.error.no_content",
            Self::InvalidEncoding(_) => "Reader.error.encoding",
        }
    }

    /// 技术细节（文件名 / 结构错误），UI 用弱化小字展示。
    #[must_use]
    pub fn detail(&self) -> String {
        self.to_string()
    }
}

/// 打开一本书并章节化。
///
/// # Errors
///
/// 格式不可读、文件缺失、编码标签非法、解码 / EPUB 结构失败、无正文时返回 [`ReaderError`]。
#[tracing::instrument(name = "reader::load_book", skip_all, fields(path = %path.display()))]
pub fn load_book(
    path: &Path,
    format: BookFormat,
    encoding: Option<&str>,
) -> Result<ReaderBook, ReaderError> {
    if !format.is_readable() {
        return Err(ReaderError::UnsupportedFormat(format));
    }
    if !path.is_file() {
        return Err(ReaderError::MissingFile);
    }
    let explicit = crate::tools::encoding::parse_encoding(encoding).map_err(|_| {
        ReaderError::InvalidEncoding(encoding.unwrap_or_default().to_string())
    })?;
    let detected = if format == BookFormat::Txt {
        crate::tools::encoding::detect_file_encoding(path)
            .ok()
            .map(|value| value.name().to_string())
    } else {
        None
    };
    let source = load_path(
        path,
        &LoadOptions {
            source_encoding: explicit,
            chapter_regex: None,
        },
    )
    .map_err(|error| map_error(format, error))?;

    let chapters: Vec<ReaderChapter> = source
        .chapters
        .into_iter()
        .map(|chapter| ReaderChapter {
            title: chapter.title,
            paragraphs: chapter.paragraphs,
        })
        .collect();
    if chapters
        .iter()
        .all(|chapter| chapter.paragraphs.iter().all(|line| line.trim().is_empty()))
    {
        return Err(ReaderError::NoContent);
    }
    Ok(ReaderBook {
        title: source.title,
        author: source.author,
        chapters,
        encoding: explicit.map(|value| value.name().to_string()).or(detected),
    })
}

fn map_error(format: BookFormat, error: ToolError) -> ReaderError {
    match error {
        ToolError::Io(error) => ReaderError::Io(error),
        ToolError::Decode(message) => ReaderError::Decode(format, message),
        ToolError::EpubStructure(message) => ReaderError::Epub(message),
        ToolError::NoContent => ReaderError::NoContent,
        ToolError::UnsupportedFormat(_) => ReaderError::UnsupportedFormat(format),
        other => ReaderError::Decode(format, other.to_string()),
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib reader::content:: -- --nocapture`
Expected: PASS（6 个测试）。

- [ ] **Step 5: 提交**

```bash
git add src/reader src/lib.rs
git commit -m "feat(reader): TXT/EPUB 内容加载"
```

---

### Task 12: `reader::layout` 分页纯函数

**Files:**
- Create: `src/reader/layout.rs`
- Modify: `src/reader/mod.rs`

**Interfaces:**
- Consumes: 无（只用 `std`）。
- Produces:
  - `ParagraphMetrics { lines: usize, spacing_after: f32 }`
  - `ReaderLayout`（排版参数 + `line_height_px()` / `fingerprint()` / `display_text()`）
  - `LayoutFingerprint`
  - `paginate(&[ParagraphMetrics], line_height, viewport_height) -> Vec<f32>`
  - `content_height(&[ParagraphMetrics], line_height) -> f32`
  - `ratio_for_page(page_index, page_count) -> f32`、`page_for_ratio(ratio, page_count) -> usize`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    fn metrics(lines: &[usize], spacing: f32) -> Vec<ParagraphMetrics> {
        lines
            .iter()
            .map(|lines| ParagraphMetrics {
                lines: *lines,
                spacing_after: spacing,
            })
            .collect()
    }

    #[test]
    fn empty_input_has_one_page_at_zero() {
        assert_eq!(paginate(&[], 20.0, 100.0), vec![0.0]);
    }

    #[test]
    fn exact_fit_is_single_page() {
        // 5 行 × 20px = 100px，正好等于视口
        assert_eq!(paginate(&metrics(&[5], 0.0), 20.0, 100.0), vec![0.0]);
    }

    #[test]
    fn breaks_on_line_boundaries() {
        // 10 行、每页 5 行 → 两页，第二页从 100px 开始
        assert_eq!(paginate(&metrics(&[10], 0.0), 20.0, 100.0), vec![0.0, 100.0]);
    }

    #[test]
    fn splits_paragraph_across_pages() {
        // 第 1 段 3 行 + 第 2 段 4 行，每页 4 行（80px）：
        // 第 1 页收 0/20/40/60 四行，第 2 页从 80px 开始
        assert_eq!(paginate(&metrics(&[3, 4], 0.0), 20.0, 80.0), vec![0.0, 80.0]);
    }

    #[test]
    fn paragraph_spacing_counts_only_at_paragraph_end() {
        // 每段 2 行、段距 10px，页高 50px：
        // 第 1 页放 2 行（40px）+ 段距 10px = 50px，第二段从 50px 开始
        assert_eq!(
            paginate(&metrics(&[2, 2], 10.0), 20.0, 50.0),
            vec![0.0, 50.0]
        );
    }

    #[test]
    fn viewport_shorter_than_one_line_still_progresses() {
        let pages = paginate(&metrics(&[3], 0.0), 20.0, 5.0);
        assert_eq!(pages, vec![0.0, 20.0, 40.0]);
    }

    #[test]
    fn content_height_sums_lines_and_spacing() {
        // (2*20 + 8) + (3*20 + 8) = 116
        assert!((content_height(&metrics(&[2, 3], 8.0), 20.0) - 116.0).abs() < f32::EPSILON);
        assert!((content_height(&[], 20.0) - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ratio_and_page_convert_both_ways() {
        assert!((ratio_for_page(0, 5) - 0.0).abs() < f32::EPSILON);
        assert!((ratio_for_page(4, 5) - 1.0).abs() < f32::EPSILON);
        assert!((ratio_for_page(2, 5) - 0.5).abs() < f32::EPSILON);
        assert_eq!(page_for_ratio(1.0, 5), 4);
        assert_eq!(page_for_ratio(0.5, 5), 2);
        assert_eq!(page_for_ratio(0.0, 0), 0);
        assert_eq!(page_for_ratio(9.0, 1), 0);
    }

    #[test]
    fn layout_prefixes_indent_and_fingerprints() {
        let mut layout = ReaderLayout::new(19.0, 1.8, 12.0, 720.0, 600.0, true, String::new());
        assert_eq!(layout.display_text("正文"), "　　正文");
        let first = layout.fingerprint(3);
        layout.indent = false;
        assert_ne!(first, layout.fingerprint(3));
        assert_eq!(layout.display_text("正文"), "正文");
        assert!((layout.line_height_px() - 34.2).abs() < 0.01);
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib reader::layout:: -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

`src/reader/mod.rs` 加：

```rust
pub mod layout;

pub use layout::{LayoutFingerprint, ParagraphMetrics, ReaderLayout, paginate};
```

`src/reader/layout.rs`：

```rust
//! 阅读排版：分页纯函数与进度比例换算。
//!
//! 分页只依赖「每段显示几行、段后间距」，不碰文本内容，因此可以在 UI 层测量、
//! 在这里用纯函数验证，两边共用同一份行数定义。

use std::borrow::Cow;

/// 单个段落的测量结果。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParagraphMetrics {
    /// 该段落换行后占的显示行数（至少 1）。
    pub lines: usize,
    /// 段落之后的间距；最后一段为 0。
    pub spacing_after: f32,
}

/// 当前章节的排版参数。尺寸单位统一用 f32 像素，UI 层再转 `gpui::Pixels`。
#[derive(Debug, Clone, PartialEq)]
pub struct ReaderLayout {
    pub font_size: f32,
    /// 行高倍数（相对字号）。
    pub line_height: f32,
    pub paragraph_spacing: f32,
    pub content_width: f32,
    pub viewport_height: f32,
    pub indent: bool,
    pub font_family: String,
}

/// 分页结果何时失效：内容宽度 / 视口高度 / 排版参数 / 章节任一变化。
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutFingerprint {
    pub chapter: usize,
    pub content_width: f32,
    pub viewport_height: f32,
    pub font_size: f32,
    pub line_height: f32,
    pub paragraph_spacing: f32,
    pub indent: bool,
    pub font_family: String,
}

impl ReaderLayout {
    /// 首行缩进前缀（两个全角空格，UTF-8 下 6 字节）。
    pub const INDENT: &'static str = "　　";

    /// 构造排版参数；`line_height` 传倍数，构造后 `line_height_px()` 得到实际像素。
    #[must_use]
    pub fn new(
        font_size: f32,
        line_height: f32,
        paragraph_spacing: f32,
        content_width: f32,
        viewport_height: f32,
        indent: bool,
        font_family: String,
    ) -> Self {
        Self {
            font_size,
            line_height,
            paragraph_spacing,
            content_width,
            viewport_height,
            indent,
            font_family,
        }
    }

    /// 单行实际高度（px）。
    #[must_use]
    pub fn line_height_px(&self) -> f32 {
        self.font_size * self.line_height
    }

    /// 渲染 / 测量共用的段落文本（缩进前缀只在这里加，搜索偏移另算）。
    #[must_use]
    pub fn display_text<'a>(&self, paragraph: &'a str) -> Cow<'a, str> {
        if self.indent {
            Cow::Owned(format!("{}{paragraph}", Self::INDENT))
        } else {
            Cow::Borrowed(paragraph)
        }
    }

    /// 当前排版参数的分页指纹。
    #[must_use]
    pub fn fingerprint(&self, chapter: usize) -> LayoutFingerprint {
        LayoutFingerprint {
            chapter,
            content_width: self.content_width,
            viewport_height: self.viewport_height,
            font_size: self.font_size,
            line_height: self.line_height,
            paragraph_spacing: self.paragraph_spacing,
            indent: self.indent,
            font_family: self.font_family.clone(),
        }
    }
}

/// 计算每页起始的纵向偏移（首项恒为 0，升序）。
///
/// 页边界永远落在行边界上：某一行放不进当前页时，下一页从这一行的顶部开始，
/// 因此不会出现半行截断。段距只在段落真正结束时计入，页边界处不额外留白。
#[must_use]
pub fn paginate(
    paragraphs: &[ParagraphMetrics],
    line_height: f32,
    viewport_height: f32,
) -> Vec<f32> {
    let line_height = line_height.max(1.0);
    let mut tops = Vec::new();
    let mut cursor = 0.0;
    for paragraph in paragraphs {
        for _ in 0..paragraph.lines.max(1) {
            tops.push(cursor);
            cursor += line_height;
        }
        cursor += paragraph.spacing_after.max(0.0);
    }
    let mut starts = vec![0.0];
    if tops.is_empty() {
        return starts;
    }
    let viewport = viewport_height.max(line_height);
    let mut start = 0.0;
    for top in tops {
        if top > start && top + line_height > start + viewport {
            start = top;
            starts.push(top);
        }
    }
    starts
}

/// 整章内容高度：行数 × 行高 + 段距总和。
#[must_use]
pub fn content_height(paragraphs: &[ParagraphMetrics], line_height: f32) -> f32 {
    paragraphs.iter().fold(0.0, |sum, paragraph| {
        sum + paragraph.lines.max(1) as f32 * line_height.max(1.0)
            + paragraph.spacing_after.max(0.0)
    })
}

/// 页号 → 章内相对进度；最后一页固定为 1.0，只有一页时为 0。
#[must_use]
pub fn ratio_for_page(page_index: usize, page_count: usize) -> f32 {
    if page_count <= 1 {
        return 0.0;
    }
    (page_index.min(page_count - 1) as f32) / (page_count - 1) as f32
}

/// 章内相对进度 → 最接近的页号。
#[must_use]
pub fn page_for_ratio(ratio: f32, page_count: usize) -> usize {
    if page_count <= 1 {
        return 0;
    }
    let ratio = ratio.clamp(0.0, 1.0);
    ((ratio * (page_count - 1) as f32).round() as usize).min(page_count - 1)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib reader::layout:: -- --nocapture`
Expected: PASS（9 个测试）。

- [ ] **Step 5: 提交**

```bash
git add src/reader/layout.rs src/reader/mod.rs
git commit -m "feat(reader): 分页与进度换算纯函数"
```

---

### Task 13: `reader::search` 书内搜索

**Files:**
- Create: `src/reader/search.rs`
- Modify: `src/reader/mod.rs`

**Interfaces:**
- Consumes: `ReaderBook`、`ReaderChapter`。
- Produces:
  - `SearchGroup { chapter, title, hits: Vec<SearchHit> }`
  - `SearchHit { paragraph, byte_offset, byte_len, snippet, snippet_match: Range<usize> }`
  - `HITS_PER_CHAPTER = 20`、`HITS_TOTAL = 1000`
  - `find_hits(text, needle) -> Vec<(usize, usize)>`
  - `search_chapter(chapter, title, paragraphs, needle, budget) -> Option<SearchGroup>`
  - `run(book, needle, cancel: &AtomicBool, on_progress) -> Vec<SearchGroup>`

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::reader::content::{ReaderBook, ReaderChapter};

    fn book() -> ReaderBook {
        ReaderBook {
            title: "书".to_string(),
            author: String::new(),
            encoding: None,
            chapters: vec![
                ReaderChapter {
                    title: "第一章".to_string(),
                    paragraphs: vec!["Hello world hello".to_string(), "无关".to_string()],
                },
                ReaderChapter {
                    title: "第二章".to_string(),
                    paragraphs: vec!["再见 世界".to_string()],
                },
            ],
        }
    }

    #[test]
    fn ascii_match_is_case_insensitive_with_original_offsets() {
        let hits = find_hits("Hello world hello", "hello");
        assert_eq!(hits, vec![(0, 5), (12, 5)]);
    }

    #[test]
    fn chinese_match_keeps_byte_offsets() {
        let hits = find_hits("再见 世界", "世界");
        assert_eq!(hits, vec![(7, 6)]);
    }

    #[test]
    fn search_chapter_groups_hits_and_snippets() {
        let group = search_chapter(0, "第一章", &["Hello world hello".to_string()], "hello", 10)
            .expect("group");
        assert_eq!(group.hits.len(), 2);
        assert_eq!(group.hits[0].paragraph, 0);
        assert_eq!(group.hits[0].byte_offset, 0);
        assert_eq!(group.hits[0].byte_len, 5);
        let (snippet, range) = snippet("你好 Hello 世界", 7, 5);
        assert_eq!(&snippet[range], "Hello");
    }

    #[test]
    fn per_chapter_and_total_limits_apply() {
        let long = "a".repeat(40);
        let group = search_chapter(0, "章", &[long], "a", 5).expect("group");
        assert_eq!(group.hits.len(), 5);

        let book = book();
        let groups = run(&book, "hello", &AtomicBool::new(false), |_, _| {});
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].hits.len(), 2);
    }

    #[test]
    fn cancelled_search_returns_nothing() {
        let book = book();
        let cancelled = AtomicBool::new(true);
        assert!(run(&book, "hello", &cancelled, |_, _| {}).is_empty());
    }

    #[test]
    fn empty_needle_or_no_match_returns_empty() {
        let book = book();
        assert!(run(&book, "   ", &AtomicBool::new(false), |_, _| {}).is_empty());
        assert!(run(&book, "不存在", &AtomicBool::new(false), |_, _| {}).is_empty());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib reader::search:: -- --nocapture`
Expected: 编译失败。

- [ ] **Step 3: 实现**

`src/reader/mod.rs` 加：

```rust
pub mod search;

pub use search::{HITS_PER_CHAPTER, HITS_TOTAL, SearchGroup, SearchHit, run as run_search};
```

`src/reader/search.rs`：

```rust
//! 书内搜索：在原始文本上按字节匹配，保留原始字节偏移供高亮使用。
//!
//! ASCII 大小写不敏感（`A` 与 `a` 视为相同），中文按字节精确匹配。UTF-8 的多字节
//! 序列不含 ASCII 字节，所以逐字节比较不会把一个汉字拆开。

use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};

use super::content::ReaderBook;

/// 单章最多返回的命中数。
pub const HITS_PER_CHAPTER: usize = 20;
/// 一次搜索最多返回的命中数。
pub const HITS_TOTAL: usize = 1000;
/// 命中前后各保留的字符数。
const SNIPPET_RADIUS: usize = 24;

/// 一章内的命中分组。
#[derive(Debug, Clone)]
pub struct SearchGroup {
    pub chapter: usize,
    pub title: String,
    pub hits: Vec<SearchHit>,
}

/// 单条命中。
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub paragraph: usize,
    /// 段落内字节偏移。
    pub byte_offset: usize,
    pub byte_len: usize,
    /// 展示用上下文。
    pub snippet: String,
    /// `byte_offset` 在 `snippet` 中的区间。
    pub snippet_match: Range<usize>,
}

/// 在文本中找出所有命中，返回 `(字节偏移, 字节长度)`。
#[must_use]
pub fn find_hits(text: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle_len = needle.len();
    if needle_len == 0 {
        return Vec::new();
    }
    let haystack = text.as_bytes();
    let target = needle.as_bytes();
    text.char_indices()
        .map(|(offset, _)| offset)
        .filter(|offset| matches_at(haystack, *offset, target))
        .map(|offset| (offset, needle_len))
        .collect()
}

fn matches_at(haystack: &[u8], offset: usize, needle: &[u8]) -> bool {
    let Some(window) = haystack.get(offset..offset + needle.len()) else {
        return false;
    };
    window.iter().zip(needle).all(|(left, right)| {
        if left.is_ascii() && right.is_ascii() {
            left.eq_ignore_ascii_case(right)
        } else {
            left == right
        }
    })
}

/// 取命中附近的上下文，返回 `(片段, 片段内命中区间)`。
#[must_use]
pub fn snippet(text: &str, offset: usize, len: usize) -> (String, Range<usize>) {
    let starts: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
    if starts.is_empty() || offset >= text.len() {
        return (text.to_string(), 0..0);
    }
    let char_index = starts.partition_point(|start| *start <= offset).saturating_sub(1);
    let match_chars = text[offset..offset + len].chars().count();
    let from_char = char_index.saturating_sub(SNIPPET_RADIUS);
    let to_char = char_index + match_chars + SNIPPET_RADIUS;
    let start = starts[from_char];
    let end = starts.get(to_char).copied().unwrap_or(text.len());
    let relative = offset - start;
    (text[start..end].to_string(), relative..relative + len)
}

/// 在单章里搜索，`budget` 是本轮搜索剩余的命中额度。
#[must_use]
pub fn search_chapter(
    chapter: usize,
    title: &str,
    paragraphs: &[String],
    needle: &str,
    budget: usize,
) -> Option<SearchGroup> {
    let limit = budget.min(HITS_PER_CHAPTER);
    if limit == 0 {
        return None;
    }
    let mut hits = Vec::new();
    for (paragraph_index, paragraph) in paragraphs.iter().enumerate() {
        for (byte_offset, byte_len) in find_hits(paragraph, needle) {
            let (snippet, snippet_match) = snippet(paragraph, byte_offset, byte_len);
            hits.push(SearchHit {
                paragraph: paragraph_index,
                byte_offset,
                byte_len,
                snippet,
                snippet_match,
            });
            if hits.len() >= limit {
                break;
            }
        }
        if hits.len() >= limit {
            break;
        }
    }
    (!hits.is_empty()).then(|| SearchGroup {
        chapter,
        title: title.to_string(),
        hits,
    })
}

/// 顺序扫描全书，每章结束后回调 `(已完成章数, 总章数)`；`cancel` 置位时提前结束。
#[must_use]
pub fn run(
    book: &ReaderBook,
    needle: &str,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(usize, usize),
) -> Vec<SearchGroup> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let total_chapters = book.chapters.len();
    let mut groups = Vec::new();
    let mut total_hits = 0;
    for (index, chapter) in book.chapters.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) || total_hits >= HITS_TOTAL {
            break;
        }
        if let Some(group) = search_chapter(
            index,
            &chapter.title,
            &chapter.paragraphs,
            needle,
            HITS_TOTAL - total_hits,
        ) {
            total_hits += group.hits.len();
            groups.push(group);
        }
        on_progress(index + 1, total_chapters);
    }
    groups
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib reader::search:: -- --nocapture`
Expected: PASS（6 个测试）。

- [ ] **Step 5: 提交**

```bash
git add src/reader/search.rs src/reader/mod.rs
git commit -m "feat(reader): 书内搜索"
```

---

### Task 14: 桌面 model 层：书架状态、扫描 / 元数据调度

**Files:**
- Modify: `src/desktop/model/library_state.rs`（整体重写）
- Modify: `src/desktop/model/library.rs`（整体重写）
- Modify: `src/desktop/model/mod.rs`, `src/desktop/model/app_model.rs`, `src/desktop/model/events.rs`
- Delete: `src/desktop/model/ops/library.rs`，并从 `src/desktop/model/ops/mod.rs` 删掉 `mod library;` / `pub use library::*;`
- Modify: `src/core/library.rs`（删除 `LibraryEntry` 及其测试，保留常量）

**Interfaces:**
- Consumes: `LibraryStore`、`scan_download_dir`、`meta::extract`、`cover::{write_cover, remove_cover_file, rename_cover_for_relink}`、`ShelfQuery`。
- Produces（`desktop::model`）：
  - `LibraryState { store, books_version, query, meta_queue, scan_rx, meta_rx, scan_in_flight, meta_in_flight, last_error, last_save_unix, scanned_dir }` + `drain_scan()` / `drain_meta()` / `bump()` / `enqueue_meta()`。
  - `ScanEvent`、`MetaEvent`。
  - `AppModel::{mark_book_opened, book_snapshot, record_reading_progress, set_reading_mode, add_reading_bookmark, remove_reading_bookmark, flush_library, persist_library, refresh_library_async, spawn_meta_worker, import_library_files, update_book_metadata, hide_book, delete_book_file, relink_book}`。
  - `LibraryStore::{mark_opened, set_cover_file}`（本任务追加到 store）。

- [ ] **Step 1: 写失败测试**

把 `src/desktop/model/library_state.rs` 整体替换为「类型 + 测试 + 空实现骨架」，测试如下：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn drain_scan_merges_files_and_bumps_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"x").unwrap();
        let (tx, rx) = smol::channel::unbounded::<ScanEvent>();
        let mut state = LibraryState::new(LibraryStore::default());
        state.scan_rx = Some(rx);
        state.scan_in_flight = true;
        smol::block_on(tx.send(Ok(vec![ScannedFile {
            path,
            size_bytes: 1,
            mtime: 1,
        }])))
        .unwrap();
        drop(tx);

        assert!(state.drain_scan());
        assert_eq!(state.store.books().len(), 1);
        assert_eq!(state.books_version, 1);
        assert!(!state.scan_in_flight);
        assert!(state.scan_rx.is_none());
        assert_eq!(state.meta_queue.len(), 1);
    }

    #[test]
    fn drain_scan_records_error() {
        let (tx, rx) = smol::channel::unbounded::<ScanEvent>();
        let mut state = LibraryState::new(LibraryStore::default());
        state.scan_rx = Some(rx);
        smol::block_on(tx.send(Err(crate::error::AppError::internal("boom".into())))).unwrap();
        drop(tx);

        assert!(state.drain_scan());
        assert!(state.last_error.is_some());
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --lib desktop::model::library_state:: -- --nocapture`
Expected: 编译失败（`LibraryState::new` / 字段不存在）。

- [ ] **Step 3: 实现 `LibraryState`**

```rust
//! 书架页状态：`LibraryStore` + 后台扫描 / 元数据通道 + 查询条件。
//!
//! 这里只排空 channel、更新 store 内存；落盘由 `AppModel::persist_library` 负责。

use std::collections::VecDeque;
use std::path::PathBuf;

use smol::channel::Receiver;

use crate::error::AppResult;
use crate::library::{AppliedMeta, Book, LibraryStore, ScannedFile, ShelfQuery, now_unix};

/// 后台扫描结果。
pub type ScanEvent = AppResult<Vec<ScannedFile>>;

/// 单本书的元数据提取结果（封面已由工作线程落盘）。
#[derive(Debug)]
pub struct MetaEvent {
    pub id: String,
    pub outcome: Result<AppliedMeta, String>,
}

/// 书架页状态。
pub struct LibraryState {
    pub store: LibraryStore,
    /// store 内容每次变化 +1；UI 用它判定是否需要重绘。
    pub books_version: u64,
    pub query: ShelfQuery,
    /// 待提取元数据的书籍 id，按入队顺序出队。
    pub meta_queue: VecDeque<String>,
    pub scan_rx: Option<Receiver<ScanEvent>>,
    pub meta_rx: Option<Receiver<MetaEvent>>,
    pub scan_in_flight: bool,
    pub meta_in_flight: bool,
    pub last_error: Option<String>,
    /// 上次进度落盘时间（unix 秒）。
    pub last_save_unix: i64,
    /// 上次扫描的目录，用于「下载目录变了才重扫」。
    pub scanned_dir: Option<PathBuf>,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self::new(LibraryStore::default())
    }
}

impl LibraryState {
    /// 用已加载的 store 构造。
    #[must_use]
    pub fn new(store: LibraryStore) -> Self {
        Self {
            store,
            books_version: 0,
            query: ShelfQuery::default(),
            meta_queue: VecDeque::new(),
            scan_rx: None,
            meta_rx: None,
            scan_in_flight: false,
            meta_in_flight: false,
            last_error: None,
            last_save_unix: now_unix(),
            scanned_dir: None,
        }
    }

    /// 书籍集合变化：让 UI 缓存与书架页失效。
    pub fn bump(&mut self) {
        self.books_version = self.books_version.wrapping_add(1);
    }

    /// 排空扫描通道；返回是否有进展。
    pub fn drain_scan(&mut self) -> bool {
        let mut any = false;
        let Some(rx) = self.scan_rx.as_ref() else {
            return false;
        };
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    any = true;
                    self.scan_in_flight = false;
                    self.scan_rx = None;
                    match event {
                        Ok(files) => {
                            let meta_ids = self.store.merge_scan(&files);
                            self.meta_queue.extend(meta_ids);
                            self.bump();
                            self.last_error = None;
                        }
                        Err(error) => self.last_error = Some(error.message()),
                    }
                }
                Err(smol::channel::TryRecvError::Empty) => break,
                Err(smol::channel::TryRecvError::Closed) => {
                    self.scan_rx = None;
                    self.scan_in_flight = false;
                    break;
                }
            }
        }
        any
    }

    /// 排空元数据通道；返回是否有进展。
    pub fn drain_meta(&mut self) -> bool {
        let mut any = false;
        let Some(rx) = self.meta_rx.as_ref() else {
            return false;
        };
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    any = true;
                    self.meta_rx = None;
                    self.meta_in_flight = false;
                    match event.outcome {
                        Ok(meta) => self.store.apply_meta(&event.id, meta),
                        Err(error) => {
                            tracing::warn!("提取元数据失败 {}: {error}", event.id);
                        }
                    }
                    self.bump();
                }
                Err(smol::channel::TryRecvError::Empty) => break,
                Err(smol::channel::TryRecvError::Closed) => {
                    self.meta_rx = None;
                    self.meta_in_flight = false;
                    break;
                }
            }
        }
        any
    }

    /// 入队待提取元数据的书籍。
    pub fn enqueue_meta(&mut self, ids: Vec<String>) {
        self.meta_queue.extend(ids);
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --lib desktop::model::library_state:: -- --nocapture`
Expected: PASS（2 个测试）。

- [ ] **Step 5: 给 `LibraryStore` 补两个方法**

在 `src/library/store.rs` 的 `impl LibraryStore` 内追加：

```rust
    /// 打开成功后写回章节数 / 编码 / 最近打开时间。
    pub fn mark_opened(&mut self, id: &str, chapter_count: u32, encoding: Option<String>) {
        let Some(book) = self.books.iter_mut().find(|book| book.id == id) else {
            return;
        };
        book.chapter_count = Some(chapter_count.max(1));
        if book.encoding.is_none()
            && let Some(encoding) = encoding
        {
            book.encoding = Some(encoding);
        }
        book.last_opened_unix = Some(now_unix());
        self.dirty = true;
    }

    /// 直接替换封面文件名（重新定位改名后调用）。
    pub fn set_cover_file(&mut self, id: &str, cover_file: Option<String>) {
        if let Some(book) = self.books.iter_mut().find(|book| book.id == id) {
            book.cover_file = cover_file;
            self.dirty = true;
        }
    }
```

- [ ] **Step 6: 重写 `src/desktop/model/library.rs`**

```rust
//! `AppModel` 书架与阅读进度方法：扫描调度、元数据工作线程、导入 / 编辑 / 删除、进度落盘。

use std::path::{Path, PathBuf};

use rust_i18n::t;

use super::{AppModel, LibraryState, MetaEvent, ScanEvent};
use crate::error::AppError;
use crate::library::{AppliedMeta, Book, ReadingMode, ReadingProgress};
```

方法实现：

```rust
impl AppModel {
    /// 扫描下载目录（异步）。重复触发由 `scan_in_flight` 拦截。
    pub fn refresh_library_async(&mut self) {
        if self.library.scan_in_flight {
            return;
        }
        // 与书架页 `maybe_auto_scan` 用同一个规范化函数，否则「相对路径 vs 绝对路径」
        // 永远不相等 → 每帧重扫。
        let dir = crate::library::absolute_dir(Path::new(&self.config.download.download_path));
        self.library.scanned_dir = Some(dir.clone());
        self.library.last_error = None;
        if !dir.exists() {
            return;
        }
        let (tx, rx) = smol::channel::unbounded::<ScanEvent>();
        self.library.scan_rx = Some(rx);
        self.library.scan_in_flight = true;
        let runtime = self.runtime;
        runtime.spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                crate::library::scan_download_dir(&dir)
            })
            .await;
            let event = match result {
                Ok(Ok(files)) => Ok(files),
                Ok(Err(error)) => Err(AppError::io_msg(&error, t!("Library.scan_failed"))),
                Err(error) => Err(AppError::internal(error.to_string())),
            };
            let _ = tx.send(event).await;
        });
    }

    /// 启动一个元数据工作线程；同一时刻只有一个，drain 结束后再启动下一个。
    pub fn spawn_meta_worker(&mut self) {
        if self.library.meta_in_flight {
            return;
        }
        loop {
            let Some(id) = self.library.meta_queue.pop_front() else {
                return;
            };
            let Some(book) = self.library.store.get(&id).cloned() else {
                continue;
            };
            let (tx, rx) = smol::channel::unbounded::<MetaEvent>();
            self.library.meta_rx = Some(rx);
            self.library.meta_in_flight = true;
            let covers_dir = self.paths.covers_dir.clone();
            let path = PathBuf::from(&book.path);
            let format = book.format;
            let runtime = self.runtime;
            runtime.spawn(async move {
                let outcome = tokio::task::spawn_blocking(move || {
                    let meta = crate::library::meta::extract(&path, format);
                    let cover_file = meta
                        .cover
                        .as_deref()
                        .and_then(|bytes| crate::library::write_cover(&covers_dir, &id, bytes));
                    AppliedMeta {
                        title: meta.title,
                        author: meta.author,
                        intro: meta.intro,
                        cover_file,
                        encoding: meta.encoding,
                    }
                })
                .await
                .map_err(|error| error.to_string());
                let _ = tx.send(MetaEvent { id, outcome }).await;
            });
            return;
        }
    }

    /// 书架落盘；失败只推一次错误，避免 100ms 循环刷屏。
    pub fn persist_library(&mut self) {
        match self.library.store.save(&self.paths.library_file) {
            Ok(()) => {
                self.library.store.clear_dirty();
                self.library.last_save_unix = crate::library::now_unix();
            }
            Err(error) => {
                let message = error.message();
                if self.library.last_error.as_deref() != Some(message.as_str()) {
                    self.library.last_error = Some(message.clone());
                    self.push_error(message);
                }
            }
        }
    }

    /// 关闭阅读器 / 退出应用时强制落盘。
    pub fn flush_library(&mut self) {
        self.persist_library();
        self.persist_settings();
    }

    /// 取一本书的快照；书籍已不存在时返回 `None`。
    #[must_use]
    pub fn book_snapshot(&self, id: &str) -> Option<Book> {
        self.library.store.get(id).cloned()
    }

    /// 阅读器打开成功后记录章节数与编码。
    pub fn mark_book_opened(&mut self, id: &str, chapter_count: u32, encoding: Option<String>) {
        self.library.store.mark_opened(id, chapter_count, encoding);
        self.library.bump();
    }

    /// 记录阅读位置（只改内存 + 标脏，由 drain 每 5s 落盘）。
    pub fn record_reading_progress(&mut self, id: &str, progress: ReadingProgress) {
        self.library.store.record_progress(
            id,
            progress.chapter,
            progress.chapter_ratio,
            progress.mode,
            progress.finished,
        );
        self.library.bump();
    }

    /// 只更新阅读方式（保留当前章节位置）。
    pub fn set_reading_mode(&mut self, id: &str, mode: ReadingMode) {
        let Some(book) = self.library.store.get(id) else {
            return;
        };
        let (chapter, ratio, finished) = (
            book.progress.chapter,
            book.progress.chapter_ratio,
            book.progress.finished,
        );
        self.library
            .store
            .record_progress(id, chapter, ratio, mode, finished);
        self.library.bump();
    }

    /// 添加书签；重复位置返回 `false`。立即落盘。
    pub fn add_reading_bookmark(&mut self, id: &str, bookmark: crate::library::Bookmark) -> bool {
        let added = self.library.store.add_bookmark(id, bookmark);
        if added {
            self.library.bump();
            self.persist_library();
        }
        added
    }

    /// 删除书签并立即落盘。
    pub fn remove_reading_bookmark(&mut self, id: &str, bookmark_id: &str) {
        self.library.store.remove_bookmark(id, bookmark_id);
        self.library.bump();
        self.persist_library();
    }

    /// 导入一批文件（引用式，不复制）。完成后启动元数据提取。
    pub fn import_library_files(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        let outcome = self.library.store.import_files(&paths);
        self.library.enqueue_meta(outcome.meta_ids);
        self.library.bump();
        self.persist_library();
        for (path, error) in &outcome.failed {
            self.push_warning(
                t!(
                    "Library.import.failed_item",
                    file = path.display().to_string(),
                    err = error
                )
                .to_string(),
            );
        }
        if outcome.added + outcome.existing > 0 {
            self.push_success(
                t!(
                    "Library.import.done",
                    added = &outcome.added.to_string(),
                    existing = &outcome.existing.to_string()
                )
                .to_string(),
            );
        }
        self.spawn_meta_worker();
    }

    /// 手动编辑元数据；替换封面时删除旧缓存文件。
    pub fn update_book_metadata(
        &mut self,
        id: &str,
        title: &str,
        author: &str,
        cover_file: Option<String>,
    ) {
        let old_cover = self
            .library
            .store
            .get(id)
            .and_then(|book| book.cover_file.clone());
        if old_cover != cover_file
            && let Some(old_cover) = old_cover
        {
            crate::library::remove_cover_file(&self.paths.covers_dir, Some(&old_cover));
        }
        self.library
            .store
            .edit_metadata(id, title, author, cover_file);
        self.library.bump();
        self.persist_library();
    }

    /// 移出书架：扫描来源只隐藏（重扫不会再加回），导入来源直接删记录。
    pub fn hide_book(&mut self, id: &str) {
        let Some(book) = self.library.store.get(id).cloned() else {
            return;
        };
        match book.origin {
            crate::library::BookOrigin::Downloaded => self.library.store.set_hidden(id, true),
            crate::library::BookOrigin::Imported => {
                crate::library::remove_cover_file(
                    &self.paths.covers_dir,
                    book.cover_file.as_deref(),
                );
                self.library.store.remove(id);
            }
        }
        self.library.bump();
        self.persist_library();
    }

    /// 删除磁盘文件并移除记录。
    pub fn delete_book_file(&mut self, id: &str) {
        let Some(book) = self.library.store.get(id).cloned() else {
            return;
        };
        if let Err(error) = std::fs::remove_file(&book.path) {
            self.push_error(
                t!("Library.delete.failed", err = error.to_string()).to_string(),
            );
            return;
        }
        crate::library::remove_cover_file(&self.paths.covers_dir, book.cover_file.as_deref());
        self.library.store.remove(id);
        self.library.bump();
        self.persist_library();
    }

    /// 重新定位失效文件；封面缓存文件名跟随新 ID 改名。
    ///
    /// # Errors
    ///
    /// 书籍不存在、格式不一致、目标文件已在书架中时返回 [`AppError`]。
    pub fn relink_book(&mut self, id: &str, new_path: &Path) -> Result<String, AppError> {
        let old_cover = self
            .library
            .store
            .get(id)
            .and_then(|book| book.cover_file.clone());
        let new_id = self.library.store.relink(id, new_path)?;
        let renamed = crate::library::cover::rename_cover_for_relink(
            &self.paths.covers_dir,
            id,
            &new_id,
            old_cover.as_deref(),
        );
        if old_cover.is_some() {
            self.library.store.set_cover_file(&new_id, renamed);
        }
        self.library.bump();
        self.persist_library();
        Ok(new_id)
    }
}
```

- [ ] **Step 7: 接上 `mod.rs` / `app_model.rs` / `events.rs`**

`src/desktop/model/mod.rs`：

```rust
pub use library_state::{LibraryState, MetaEvent, ScanEvent};
```

删除 `pub use library_state::{LibraryEntry, LibraryState, scan_library_dir};`。

`src/desktop/model/app_model.rs`：

```rust
use crate::library::LibraryStore;
```

构造段在 `load_tasks_from_file` 之后加：

```rust
        let (library_store, library_warning) = LibraryStore::load(&paths.library_file);
```

`let model = Self {` 改成 `let mut model = Self {`，`library: LibraryState::new(library_store),`，并在 `Ok((model, rx))` 之前：

```rust
        if let Some(warning) = library_warning {
            model.push_error(warning);
        }
        Ok((model, rx))
```

`src/desktop/model/events.rs` 把原来的 `any |= model.library.drain_scan();` 两行替换为：

```rust
    // 2b. 书架：扫描 / 元数据通道 + 进度节流落盘（5 秒最多一次）。
    any |= model.library.drain_scan();
    any |= model.library.drain_meta();
    model.spawn_meta_worker();
    if model.library.store.is_dirty()
        && crate::library::now_unix() - model.library.last_save_unix >= 5
    {
        model.persist_library();
        any = true;
    }
```

- [ ] **Step 8: 删除旧书库代码，书架页换成占位实现**

- 删除 `src/desktop/model/ops/library.rs`；`ops/mod.rs` 去掉 `mod library;` 与 `pub use library::*;`。
- `src/core/library.rs`：删掉 `LibraryEntry` 与依赖它的 5 个测试，只保留 `SUPPORTED_LIBRARY_EXTS` 和扩展名测试；模块文档改成「下载文件扩展名白名单」。
- 全仓 `grep -rn "LibraryEntry" src/` 必须无结果。

旧书架页（`toolbar.rs` / `row.rs` / `delegate.rs` / `library_page.rs`）全部引用 `LibraryEntry` 与
`filter_text` / `filter_ext` / `entries`，本任务删除它们后已无法编译。**本步把书架页整体换成占位
实现**，Task 16 再用卡片网格替换 —— 这样每个任务结束时仓库都能编译、测试全绿。

```bash
git rm src/desktop/pages/library/toolbar.rs \
       src/desktop/pages/library/row.rs \
       src/desktop/pages/library/delegate.rs
```

`src/desktop/pages/library/mod.rs` 整体替换为：

```rust
//! Library 页面：本地书库。
//!
//! Task 14 只保留占位页（旧 `LibraryEntry` 实现已随书库重写删除），
//! Task 16 替换为卡片书架。

mod library_page;

pub use library_page::{LibraryPage, OpenReaderFn};
```

`src/desktop/pages/library/library_page.rs` 整体替换为：

```rust
//! 书架页占位实现：Task 16 用卡片网格整体替换。
//!
//! 存在的意义是让「删除 `LibraryEntry` / 旧 ops」能单独编译通过。占位页只显示页面标题与
//! 下载目录，并保存 `RootView` 注入的打开回调（Task 16 才会真正用到）。

use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div,
};
use rust_i18n::t;

use crate::desktop::components::PageHeader;
use crate::desktop::model::AppModel;

/// 书架卡片点击 → 打开阅读器。由 `RootView` 注入，避免页面反向持有根视图。
pub type OpenReaderFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// 书架页（占位）。
pub struct LibraryPage {
    model: Entity<AppModel>,
    /// Task 16 之前只存不用 —— 前导下划线避开 `dead_code`。
    _open_reader: Option<OpenReaderFn>,
}

impl LibraryPage {
    /// 建页；`RootView` 一次性创建，跨页切换保持状态。
    pub fn new(model: Entity<AppModel>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            model,
            _open_reader: None,
        }
    }

    /// 由 `RootView` 注入打开阅读器的回调。
    pub fn set_open_reader_handler(&mut self, handler: OpenReaderFn) {
        self._open_reader = Some(handler);
    }
}

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let download_path = self.model.read(cx).config.download.download_path.clone();
        div()
            .size_full()
            .p_6()
            .bg(cx.theme().background)
            .child(PageHeader::new(t!("Library.page_title").into()).subtitle(download_path.into()))
    }
}
```

- [ ] **Step 9: 全量校验**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿。若 `dead_code` 报警，说明某个方法还没被 UI 调用 —— 在对应 Task 里接上，不要加 `#[allow(dead_code)]`。

- [ ] **Step 10: 提交**

```bash
git add src/desktop/model src/core/library.rs
git commit -m "feat(desktop): 书架状态与后台元数据调度"
```

---

### Task 15: 阅读器外壳与 `RootView` 集成

**Files:**
- Create: `src/desktop/pages/reader/mod.rs`, `src/desktop/pages/reader/reader_page.rs`, `src/desktop/pages/reader/theme.rs`
- Modify: `src/desktop/pages/mod.rs`, `src/desktop/root.rs`

**Interfaces:**
- Consumes: `AppModel::{book_snapshot, mark_book_opened, record_reading_progress, set_reading_mode, flush_library}`、`reader::{load_book, ReaderError}`。
- Produces:
  - `pages::ReaderPage`
  - `ReaderPage::{new, is_immersive, close_and_flush, set_close_handler}`
  - `OpenReaderFn = Rc<dyn Fn(&str, &mut Window, &mut App)>`（书架页用它打开阅读器）
  - `RootView::{open_reader, close_reader}`

- [ ] **Step 1: 写最小状态机**

`src/desktop/pages/reader/mod.rs`：

```rust
//! 应用内阅读器：覆盖式视图，不作为第 7 个导航页。

pub mod reader_page;
pub mod theme;

pub use reader_page::{CloseReaderFn, LoadState, ReaderPage};
pub use theme::reader_colors;
```

`src/desktop/pages/reader/theme.rs`：

```rust
//! 阅读主题 → 具体颜色。`src/reader` 保持 UI 中立，颜色只在桌面层出现。

use gpui_kit::{Hsla, Rgba, hsla};
use gpui_kit::component::ActiveTheme as _;

use crate::config::ReaderTheme;

/// 正文与阅读器外壳的前景 / 背景 / 高亮色。
#[derive(Debug, Clone, Copy)]
pub struct ReaderColors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub muted: Hsla,
    pub highlight: Hsla,
    pub active_highlight: Hsla,
}

/// 按主题给出配色；`ReaderTheme::App` 跟随应用主题。
pub fn reader_colors(theme: ReaderTheme, cx: &gpui_kit::App) -> ReaderColors {
    match theme {
        ReaderTheme::Paper => palette(0xf7f4ec, 0x2b2a26, 0xf3d98b, 0xe8b93f),
        ReaderTheme::Sepia => palette(0xf0e2c8, 0x4a3b24, 0xe9cf95, 0xd9a94a),
        ReaderTheme::Night => palette(0x1c1d21, 0xc9c9c4, 0x4a4a20, 0xb8a54a),
        ReaderTheme::Green => palette(0xdce8d5, 0x24331f, 0xbcd98f, 0x8fbf5a),
        ReaderTheme::App => ReaderColors {
            background: cx.theme().background,
            foreground: cx.theme().foreground,
            muted: cx.theme().muted_foreground,
            highlight: cx.theme().selection,
            active_highlight: cx.theme().accent,
        },
    }
}

fn palette(background: u32, foreground: u32, highlight: u32, active_highlight: u32) -> ReaderColors {
    ReaderColors {
        background: rgb(background),
        foreground: rgb(foreground),
        muted: hsla(0.0, 0.0, 0.55, 1.0),
        highlight: rgb(highlight),
        active_highlight: rgb(active_highlight),
    }
}

fn rgb(value: u32) -> Hsla {
    let red = ((value >> 16) & 0xff) as f32 / 255.0;
    let green = ((value >> 8) & 0xff) as f32 / 255.0;
    let blue = (value & 0xff) as f32 / 255.0;
    Rgba { r: red, g: green, b: blue, a: 1.0 }.into()
}
```

`src/desktop/pages/reader/reader_page.rs`：

```rust
//! 阅读器页面：加载状态机 + 顶栏 + 正文骨架。
//!
//! 滚动 / 翻页的完整实现分别在 Task 18 / 19 里替换 `render_chapter`；
//! 本任务先把「从书架打开 → 加载 → 显示正文 → 关闭落盘」的闭环跑通。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, Context, Entity, FocusHandle, FontWeight, IntoElement, ParentElement, Render, SharedString,
    Styled, Window, div,
};
use rust_i18n::t;

use super::theme::reader_colors;
use crate::config::{ReaderTheme, ReadingMode};
use crate::desktop::model::AppModel;
use crate::library::{Book, BookFormat, ReadingProgress};
use crate::reader::{ReaderBook, ReaderError, load_book};

/// 关闭阅读器：由 `RootView` 注入（需要同时改 RootView 状态与全屏标记）。
pub type CloseReaderFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// 打开状态。
pub enum LoadState {
    Loading,
    Ready,
    Error(ReaderError),
}

/// 阅读器页面。
pub struct ReaderPage {
    model: Entity<AppModel>,
    book_id: String,
    book: Book,
    content: Option<ReaderBook>,
    load_state: LoadState,
    mode: ReadingMode,
    chapter: usize,
    chapter_ratio: f32,
    immersive: bool,
    generation: u64,
    progress_dirty: bool,
    focus: FocusHandle,
    close_handler: Option<CloseReaderFn>,
}

impl ReaderPage {
    /// 打开一本书并启动后台加载。
    pub fn new(
        model: Entity<AppModel>,
        book: Book,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut page = Self {
            model,
            book_id: book.id.clone(),
            mode: book.progress.mode,
            chapter: book.progress.chapter,
            chapter_ratio: book.progress.chapter_ratio,
            book,
            content: None,
            load_state: LoadState::Loading,
            immersive: false,
            generation: 1,
            progress_dirty: false,
            focus,
            close_handler: None,
        };
        page.spawn_load(cx);
        page
    }

    /// 注入关闭回调（`RootView` 调用）。
    pub fn set_close_handler(&mut self, handler: CloseReaderFn) {
        self.close_handler = Some(handler);
    }

    /// 是否处于沉浸模式（`RootView` 据此隐藏 TitleBar）。
    #[must_use]
    pub const fn is_immersive(&self) -> bool {
        self.immersive
    }

    /// 落盘进度与阅读设置；随后由 `RootView` 丢弃本实体。
    pub fn close_and_flush(&mut self, cx: &mut Context<Self>) {
        self.record_progress(cx);
        self.progress_dirty = false;
        self.model.update(cx, |model, _| model.flush_library());
    }

    fn spawn_load(&mut self, cx: &mut Context<Self>) {
        let generation = self.generation;
        let path = PathBuf::from(&self.book.path);
        let format = self.book.format;
        let encoding = self.book.encoding.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_book(&path, format, encoding.as_deref()) })
                .await;
            let _ = this.update(cx, |page, cx| page.finish_load(generation, result, cx));
        })
        .detach();
    }

    fn finish_load(
        &mut self,
        generation: u64,
        result: Result<ReaderBook, ReaderError>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        match result {
            Ok(content) => {
                let chapter_count = content.chapters.len().max(1);
                self.chapter = self.chapter.min(chapter_count - 1);
                self.chapter_ratio = self.chapter_ratio.clamp(0.0, 1.0);
                let encoding = content.encoding.clone();
                self.content = Some(content);
                self.load_state = LoadState::Ready;
                let book_id = self.book_id.clone();
                self.model.update(cx, |model, _| {
                    model.mark_book_opened(&book_id, chapter_count as u32, encoding);
                });
                self.record_progress(cx);
            }
            Err(error) => self.load_state = LoadState::Error(error),
        }
        cx.notify();
    }

    /// 当前章节快照（章节下标已夹在合法范围）。
    fn current_chapter(&self) -> Option<&crate::reader::ReaderChapter> {
        let content = self.content.as_ref()?;
        content.chapters.get(self.chapter)
    }

    fn record_progress(&mut self, cx: &mut Context<Self>) {
        let progress = ReadingProgress {
            chapter: self.chapter,
            chapter_ratio: self.chapter_ratio,
            percent: 0.0,
            mode: self.mode,
            updated_unix: 0,
            finished: self.book.progress.finished,
        };
        let book_id = self.book_id.clone();
        self.model
            .update(cx, |model, _| model.record_reading_progress(&book_id, progress));
        self.progress_dirty = true;
    }

    fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        self.mode = match self.mode {
            ReadingMode::Scroll => ReadingMode::Page,
            ReadingMode::Page => ReadingMode::Scroll,
        };
        self.book.progress.mode = self.mode;
        let book_id = self.book_id.clone();
        let mode = self.mode;
        self.model
            .update(cx, |model, _| model.set_reading_mode(&book_id, mode));
        cx.notify();
    }

    fn goto_chapter(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(content) = self.content.as_ref() else {
            return;
        };
        let count = content.chapters.len();
        let next = self.chapter.saturating_add_signed(delta).min(count - 1);
        if next != self.chapter {
            self.chapter = next;
            self.chapter_ratio = 0.0;
            self.record_progress(cx);
            cx.notify();
        }
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let close_handler = self.close_handler.clone();
        let mode_label = match self.mode {
            ReadingMode::Scroll => t!("Reader.mode.page"),
            ReadingMode::Page => t!("Reader.mode.scroll"),
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("reader-back")
                    .label(t!("Reader.action.back"))
                    .on_click(move |_event, window, cx| {
                        if let Some(handler) = &close_handler {
                            handler(window, cx);
                        }
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .text_center()
                    .font_weight(FontWeight::MEDIUM)
                    .child(SharedString::from(self.book.title.clone())),
            )
            .child(
                Button::new("reader-mode")
                    .label(mode_label)
                    .on_click(cx.listener(|this, _event, _window, cx| this.toggle_mode(cx))),
            )
    }

    fn render_error(&self, error: &ReaderError) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .p_8()
            .child(div().text_lg().child(t!(error.message_key()).to_string()))
            .child(div().text_sm().child(error.detail()))
    }

    fn render_chapter(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(content) = self.content.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let colors = reader_colors(self.book_theme(cx), &*cx);
        let Some(chapter) = content.chapters.get(self.chapter) else {
            return div().flex_1().into_any_element();
        };
        let title = chapter.title.clone();
        let paragraphs = chapter.paragraphs.clone();
        let count = content.chapters.len();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(
                div()
                    .id("reader-chapter-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_6()
                    .child(
                        // 居中靠 flex 而不是 `mx_auto`：gpui 没有 auto margin 样式。
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .justify_center()
                            .child(
                                div()
                                    .w_full()
                                    .max_w(gpui_kit::px(720.0))
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(
                                        div().text_xl().font_weight(FontWeight::BOLD).child(title),
                                    )
                                    .children(paragraphs.into_iter().map(|line| div().child(line))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("reader-prev-chapter")
                            .label(t!("Reader.action.prev_chapter"))
                            .disabled(self.chapter == 0)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.goto_chapter(-1, cx);
                            })),
                    )
                    .child(div().text_sm().child(t!(
                        "Reader.chapter_indicator",
                        current = self.chapter + 1,
                        total = count
                    )))
                    .child(
                        Button::new("reader-next-chapter")
                            .label(t!("Reader.action.next_chapter"))
                            .disabled(self.chapter + 1 >= count)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.goto_chapter(1, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn book_theme(&self, cx: &Context<Self>) -> ReaderTheme {
        self.model.read(cx).config.reader.theme
    }
}

impl Render for ReaderPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body: gpui_kit::AnyElement = match &self.load_state {
            LoadState::Loading => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(t!("Reader.loading").to_string())
                .into_any_element(),
            LoadState::Error(error) => self.render_error(error).into_any_element(),
            LoadState::Ready => self.render_chapter(cx).into_any_element(),
        };
        div()
            .track_focus(&self.focus)
            .key_context("Reader")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_toolbar(cx))
            .child(body)
    }
}
```

- [ ] **Step 2: 注册页面**

`src/desktop/pages/mod.rs` 加 `pub mod reader;` 与 `pub use reader::ReaderPage;`。

- [ ] **Step 3: 接上 `RootView`**

`src/desktop/root.rs` 改动：

```rust
use std::rc::Rc;

use super::pages::{LibraryPage, ReaderPage, SearchPage, SettingsPage, SourcesPage, TasksPage, ToolsPage};
```

结构体加字段：

```rust
    /// 打开的阅读器；`Some` 时覆盖主区域并隐藏侧栏。
    reader: Option<Entity<ReaderPage>>,
```

`new()` 末尾初始化 `reader: None`。构造 `library_page` 之后、`Self { ... }` 之前注入打开回调：

```rust
        // 书架卡片点击 → RootView::open_reader。用弱引用避免 RootView ↔ LibraryPage 循环持有。
        let root_handle = cx.entity().downgrade();
        library_page.update(cx, |page, _cx| {
            page.set_open_reader_handler(Rc::new(move |book_id, window, cx| {
                let _ = root_handle.update(cx, |root, cx| root.open_reader(book_id, window, cx));
            }));
        });
```

新增方法：

```rust
    /// 打开阅读器；书已不在书架或格式不可读时推错误通知。
    pub fn open_reader(&mut self, book_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            self.model.update(cx, |model, _| {
                model.push_error(t!("Library.error.book_not_found").to_string())
            });
            return;
        };
        if !book.format.is_readable() {
            self.model.update(cx, |model, _| {
                model.push_warning(t!("Library.status.unsupported").to_string());
            });
            return;
        }
        let model = self.model.clone();
        let reader = cx.new(|cx| ReaderPage::new(model, book, window, cx));
        let root_handle = cx.entity().downgrade();
        reader.update(cx, |page, _cx| {
            page.set_close_handler(Rc::new(move |window, cx| {
                let _ = root_handle.update(cx, |root, cx| root.close_reader(window, cx));
            }));
        });
        self.reader = Some(reader);
        cx.notify();
    }

    /// 关闭阅读器：落盘进度、恢复窗口装饰。
    pub fn close_reader(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(reader) = self.reader.take() else {
            return;
        };
        let immersive = reader.read(cx).is_immersive();
        reader.update(cx, |page, cx| page.close_and_flush(cx));
        if immersive {
            window.toggle_fullscreen();
        }
        cx.notify();
    }
```

`render()` 开头插入阅读器分支（放在排空通知之后）：

```rust
        if let Some(reader) = self.reader.clone() {
            let immersive = reader.read(cx).is_immersive();
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    div()
                        .track_focus(&self.focus)
                        .key_context(KEY_CONTEXT)
                        .flex()
                        .flex_col()
                        .size_full()
                        .when(!immersive, |this| this.child(self.render_title_bar(cx)))
                        .child(div().flex_1().min_h_0().child(reader)),
                )
                .into_any_element();
        }
```

原有 `render()` 末尾的表达式也要改成 `.into_any_element()`：函数现在有两条返回路径，返回类型会被推断成 `AnyElement`。

导航旁路：`bind_nav_actions` 的 listener 改成先关阅读器。以 `ShowSearch` 为例（其余 5 个 action、`NextPage` / `PrevPage` 同样处理）：

```rust
        .on_action(cx.listener(|this, _: &ShowSearch, window, cx| {
            this.close_reader(window, cx);
            this.navigate(NavPage::Search, cx);
        }))
```

`ToggleSidebar` 保持原样（阅读器模式下侧栏本就隐藏，切换值无害）。再加退出前落盘：

```rust
        .on_action(cx.listener(|this, _: &super::Quit, window, cx| {
            this.close_reader(window, cx);
            this.model.update(cx, |model, _| model.flush_library());
            cx.quit();
        }))
```

注意 `crate::desktop::Quit` 已在 `desktop` re-export，用 `super::Quit` 即可。

- [ ] **Step 4: 编译并手动验证**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo run
```

操作：搜索页无变化；书架卡片此时还不能打开（Task 16 接入）。用 `cargo test` 确认没有回归。

- [ ] **Step 5: 提交**

```bash
git add src/desktop/pages src/desktop/root.rs
git commit -m "feat(reader): 阅读器外壳与 RootView 集成"
```
---

### Task 16: 书架页（卡片网格）

**Files:**
- Create: `src/desktop/pages/library/card.rs`
- Modify: `src/desktop/pages/library/library_page.rs`（整体替换占位实现）
- Modify: `src/desktop/pages/library/mod.rs`
- Modify: `src/desktop/model/library.rs`（追加 `toggle_book_favorite`）

**Interfaces:**
- Consumes: `library::{visible_books, ShelfQuery, ShelfSort, ShelfStatusFilter, cover_palette, absolute_dir}`、`AppModel::{refresh_library_async, import_library_files, persist_library}`、`LibraryStore::set_favorite`。
- Produces:
  - `pages::library::OpenReaderFn`（Task 14 占位页已定义，本任务保留同签名）
  - `LibraryPage::{new, set_open_reader_handler}`
  - `card::{BookCard, CardData}`
  - `AppModel::toggle_book_favorite(&mut self, id: &str)`

**设计要点（写代码前先读一遍）：**

- 卡片网格用 `flex_wrap` 手写，**不**用 gpui-kit 的 `List`：`List` 要求等高行、
  自带虚拟滚动，与封面网格的行列关系对不上。
- 收藏星标是卡片内部的按钮，点击必须 `cx.stop_propagation()`，否则会连带触发外层卡片的
  「打开阅读器」。
- 卡片只渲染 `CardData`（10 个字段的只读快照），不 clone 整本 `Book`（含书签数组）。
- 「加载更多」是页面本地状态 `visible_limit`，筛选 / 排序 / 搜索变化时回到 `BATCH`。

- [ ] **Step 1: 给 `AppModel` 加收藏开关**

`src/desktop/model/library.rs` 的 `impl AppModel` 内追加（放在 `mark_book_opened` 之后）：

```rust
    /// 切换收藏并立即落盘（书架卡片的星标）。
    pub fn toggle_book_favorite(&mut self, id: &str) {
        let Some(book) = self.library.store.get(id) else {
            return;
        };
        let next = !book.favorite;
        self.library.store.set_favorite(id, next);
        self.library.bump();
        self.persist_library();
    }
```

- [ ] **Step 2: 写卡片**

新建 `src/desktop/pages/library/card.rs`：

```rust
//! 书架卡片：封面（真实封面或生成封面）、格式徽标、收藏星标、书名 / 作者、进度条。

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, ClickEvent, ElementId, FontWeight, Hsla, Icon, IconName, IntoElement, ObjectFit,
    ParentElement, RenderOnce, Styled, Window, div, img, linear_color_stop, linear_gradient, px,
    relative, rgb, rgba,
};
use rust_i18n::t;

use crate::library::{Book, cover_palette};

/// 封面尺寸（2:3）：140×210。
const COVER_W: f32 = 140.0;
const COVER_H: f32 = 210.0;

/// 卡片渲染所需的只读快照 —— 每帧只取这些字段，不 clone 整本 `Book`。
pub(super) struct CardData {
    pub id: String,
    pub title: String,
    pub author: String,
    pub format_label: &'static str,
    /// `Book::status_key()` 的结果，UI 层再翻译。
    pub status_key: &'static str,
    pub percent: f32,
    pub favorite: bool,
    pub missing: bool,
    pub cover_path: Option<PathBuf>,
    /// 生成封面的渐变色（顶部 / 底部）。
    pub cover_top: Hsla,
    pub cover_bottom: Hsla,
}

impl CardData {
    /// 由 `Book` 与封面缓存目录构造；缓存文件已不在磁盘上时退回生成封面。
    pub(super) fn from_book(book: &Book, covers_dir: &Path) -> Self {
        let cover_path = book.cover_file.as_deref().and_then(|file| {
            // `library.json` 可能被手工改过：带路径分隔符的名字一律不读（防目录穿越）。
            if file.contains('/') || file.contains('\\') {
                return None;
            }
            let path = covers_dir.join(file);
            path.is_file().then_some(path)
        });
        let (_, top, bottom) = cover_palette(&book.id);
        Self {
            id: book.id.clone(),
            title: book.title.clone(),
            author: book.author.clone(),
            format_label: book.format.label(),
            status_key: book.status_key(),
            percent: book.progress.percent,
            favorite: book.favorite,
            missing: book.missing,
            cover_path,
            cover_top: hsla_from(top),
            cover_bottom: hsla_from(bottom),
        }
    }
}

/// 书架卡片。
#[derive(IntoElement)]
pub(super) struct BookCard {
    data: CardData,
    on_open: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
    on_favorite: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
}

impl BookCard {
    pub(super) fn new(
        data: CardData,
        on_open: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
        on_favorite: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
    ) -> Self {
        Self {
            data,
            on_open,
            on_favorite,
        }
    }
}

impl RenderOnce for BookCard {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Self {
            data,
            on_open,
            on_favorite,
        } = self;

        // ---- 1. 封面区：先铺生成封面（渐变 + 书名首字），有真实封面时再盖上去 ----
        let mut cover = div()
            .relative()
            .w(px(COVER_W))
            .h(px(COVER_H))
            .rounded_md()
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .bg(linear_gradient(
                135.0,
                linear_color_stop(data.cover_top, 0.0),
                linear_color_stop(data.cover_bottom, 1.0),
            ))
            .child(
                // 白色首字落在两段深色渐变上，对比度够。
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(0xffffff))
                    .child(first_char(&data.title)),
            );

        if let Some(path) = data.cover_path.as_deref() {
            cover = cover.child(
                div()
                    .absolute()
                    .inset_0()
                    .child(img(path).size_full().object_fit(ObjectFit::Cover)),
            );
        }

        // 左上格式徽标：TXT / EPUB 是技术名词，不翻译。
        cover = cover.child(
            div()
                .absolute()
                .top_1()
                .left_1()
                .px_1()
                .rounded_sm()
                .bg(rgba(0x00000099))
                .text_xs()
                .text_color(rgb(0xffffff))
                .child(data.format_label),
        );

        // 右上收藏星标：点击必须阻止冒泡，否则会触发外层卡片的「打开阅读器」。
        let favorite_icon = Icon::new(if data.favorite {
            IconName::StarFill
        } else {
            IconName::Star
        })
        .text_color(if data.favorite {
            rgb(0xfbbf24)
        } else {
            rgb(0xffffff)
        });
        cover = cover.child(
            div()
                .id(ElementId::Name(format!("book-fav-{}", data.id).into()))
                .absolute()
                .top_1()
                .right_1()
                .w(px(22.))
                .h(px(22.))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x00000066))
                .cursor_pointer()
                .child(favorite_icon)
                .on_click(move |event, window, cx| (on_favorite)(event, window, cx)),
        );

        if data.missing {
            cover = cover.child(
                div()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xffffff))
                            .child(t!("Library.card.missing").to_string()),
                    ),
            );
        }

        // ---- 2. 文本区：书名 2 行 + 作者 1 行 + 进度条 + 状态 ----
        let mut card = div()
            .id(ElementId::Name(format!("book-card-{}", data.id).into()))
            .w(px(COVER_W))
            .flex()
            .flex_col()
            .gap_1()
            .cursor_pointer()
            .on_click(move |event, window, cx| (on_open)(event, window, cx))
            .child(cover)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .line_clamp(2)
                    .child(data.title.clone()),
            );

        if !data.author.is_empty() {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(data.author.clone()),
            );
        }

        let percent = data.percent.clamp(0.0, 100.0);
        card.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .h(px(3.))
                        .w_full()
                        .rounded_full()
                        .bg(cx.theme().muted)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(cx.theme().primary)
                                .w(relative(percent / 100.0)),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("{} · {percent:.0}%", t!(data.status_key))),
                ),
        )
    }
}

/// 书名首字（生成封面用）。
fn first_char(title: &str) -> String {
    title.chars().next().map(String::from).unwrap_or_default()
}

/// `[r, g, b]` → `Hsla`。
fn hsla_from([red, green, blue]: [u8; 3]) -> Hsla {
    rgb(u32::from_be_bytes([0, red, green, blue])).into()
}
```

- [ ] **Step 3: 写书架页**

`src/desktop/pages/library/library_page.rs` 整体替换为：

```rust
//! 书架页：封面卡片网格 + 搜索 / 状态筛选 / 收藏开关 / 排序 / 导入 / 重新扫描。
//!
//! - 数据全部来自 `AppModel.library.store`，每帧用 `library::visible_books` 过滤排序
//!   （书架规模是「个人书库」量级，纯计算过滤比维护缓存简单且不会失效）；
//! - 卡片点击 → `RootView::open_reader`，通过 `set_open_reader_handler` 注入的回调，
//!   页面不持有根视图（避免循环强引用）；
//! - 扫描 / 元数据提取都在后台跑，drain 循环回传后窗口自动重绘。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, IntoElement, ParentElement, Point,
    Render, ScrollHandle, Styled, Window, div,
};
use rust_i18n::t;

use super::card::{BookCard, CardData};
use crate::desktop::components::{EmptyState, PageHeader};
use crate::desktop::model::AppModel;
use crate::library::{ShelfQuery, ShelfSort, ShelfStatusFilter, absolute_dir, visible_books};

/// 书架卡片点击 → 打开阅读器。由 `RootView` 注入。
pub type OpenReaderFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// 每批渲染的卡片数；「加载更多」每次追加一批。
const BATCH: usize = 60;

/// 书架页面 entity。
pub struct LibraryPage {
    /// `dialogs` 子模块也要读写它。
    pub(super) model: Entity<AppModel>,
    /// 搜索框 state 必须由页面持有，否则每次重绘丢焦点与光标。
    search_input: Entity<InputState>,
    scroll: ScrollHandle,
    /// 当前渲染的卡片上限。
    visible_limit: usize,
    /// 打开阅读器的回调（`dialogs` 的「继续阅读」也要用）。
    pub(super) open_reader: Option<OpenReaderFn>,
}

impl LibraryPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 后台扫描 / 元数据 / 阅读进度都会改 `AppModel`：观察到就重绘，
        // 不依赖 gpui 的「渲染期读过该 entity」隐式失效。
        cx.observe(&model, |_this, _model, cx| cx.notify()).detach();

        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("Library.search_placeholder")));
        cx.subscribe_in(&search_input, window, |this, _state, event, _window, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let text = this.search_input.read(cx).value().to_string();
            let changed = this.model.update(cx, |model, _cx| {
                let changed = model.library.query.text != text;
                model.library.query.text = text;
                changed
            });
            if changed {
                this.reset_view(cx);
            }
        })
        .detach();

        Self {
            model,
            search_input,
            scroll: ScrollHandle::new(),
            visible_limit: BATCH,
            open_reader: None,
        }
    }

    /// 由 `RootView` 注入打开阅读器的回调。
    pub fn set_open_reader_handler(&mut self, handler: OpenReaderFn) {
        self.open_reader = Some(handler);
    }

    /// 打开一本书；没有注入回调（理论上不会发生）时静默忽略。
    ///
    /// `pub(super)`：Task 17 的 `dialogs` 子模块里的「继续阅读」也要调它。
    pub(super) fn open_book(&self, book_id: &str, window: &mut Window, cx: &mut App) {
        if let Some(open) = self.open_reader.as_ref() {
            open(book_id, window, cx);
        }
    }

    /// 筛选 / 排序 / 搜索变化后回到顶部并重置批次。
    fn reset_view(&mut self, cx: &mut Context<Self>) {
        self.visible_limit = BATCH;
        self.scroll.set_offset(Point::default());
        cx.notify();
    }

    /// 首次进入 / 下载目录变化时自动扫一次（重复调用由 `scan_in_flight` 拦掉）。
    fn maybe_auto_scan(&mut self, cx: &mut Context<Self>) {
        let wanted = absolute_dir(std::path::Path::new(
            &self.model.read(cx).config.download.download_path,
        ));
        if self.model.read(cx).library.scanned_dir.as_ref() != Some(&wanted) {
            self.model.update(cx, |model, _cx| model.refresh_library_async());
        }
    }

    /// 重新扫描下载目录。
    fn rescan(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, _cx| model.refresh_library_async());
        cx.notify();
    }

    /// 打开系统文件对话框导入本地文件（只记录路径，不复制文件）。
    fn pick_and_import(&self, cx: &Context<Self>) {
        let model = self.model.clone();
        let page = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            let files = rfd::AsyncFileDialog::new()
                .add_filter(t!("Library.import.filter_books").as_ref(), &["txt", "epub"])
                .add_filter(t!("Library.import.filter_all").as_ref(), &["*"])
                .set_title(t!("Library.import.dialog_title").as_ref())
                .pick_files()
                .await;
            let Some(files) = files else {
                return;
            };
            let paths: Vec<PathBuf> = files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect();
            let _ = page.update(async_cx, |_page, cx| {
                model.update(cx, |model, _cx| model.import_library_files(paths));
                cx.notify();
            });
        })
        .detach();
    }

    /// 状态筛选：四个按钮互斥，选中任一状态同时关掉「收藏」开关。
    fn set_status_filter(&mut self, filter: ShelfStatusFilter, cx: &mut Context<Self>) {
        let changed = self.model.update(cx, |model, _cx| {
            let query = &mut model.library.query;
            let changed = query.status != filter || query.favorite_only;
            query.status = filter;
            query.favorite_only = false;
            changed
        });
        if changed {
            self.reset_view(cx);
        }
    }

    /// 「收藏」是独立开关：打开时状态回到「全部」，否则两个筛选叠加会看着像空架。
    fn toggle_favorites(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, _cx| {
            let query = &mut model.library.query;
            query.favorite_only = !query.favorite_only;
            if query.favorite_only {
                query.status = ShelfStatusFilter::All;
            }
        });
        self.reset_view(cx);
    }

    /// 排序循环切换：最近阅读 → 最近添加 → 书名 → 大小 → …
    fn cycle_sort(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, _cx| {
            let current = model.library.query.sort;
            let index = ShelfSort::ALL
                .iter()
                .position(|sort| *sort == current)
                .unwrap_or(0);
            model.library.query.sort = ShelfSort::ALL[(index + 1) % ShelfSort::ALL.len()];
        });
        self.reset_view(cx);
    }

    /// 清空搜索 + 筛选（空结果页的「清除筛选」按钮）。
    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.model
            .update(cx, |model, _cx| model.library.query = ShelfQuery::default());
        self.reset_view(cx);
    }

    /// 单个状态筛选按钮：当前生效时用主色，其余 ghost。
    fn status_button(
        &self,
        id: &'static str,
        filter: ShelfStatusFilter,
        status: ShelfStatusFilter,
        favorite_only: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        let active = !favorite_only && status == filter;
        let button = Button::new(id).label(t!(filter.label_key()).to_string());
        let button = if active { button.primary() } else { button.ghost() };
        button.on_click(cx.listener(move |this, _ev, _window, cx| {
            this.set_status_filter(filter, cx);
        }))
    }

    /// 收藏开关按钮。
    fn favorite_button(&self, favorite_only: bool, cx: &mut Context<Self>) -> Button {
        let icon = Icon::new(if favorite_only {
            IconName::StarFill
        } else {
            IconName::Star
        });
        let button = Button::new("shelf-filter-favorites")
            .icon(icon)
            .label(t!("Library.filter.favorites").to_string());
        let button = if favorite_only {
            button.primary()
        } else {
            button.ghost()
        };
        button.on_click(cx.listener(|this, _ev, _window, cx| this.toggle_favorites(cx)))
    }

    /// 排序按钮（点一次切下一个）。
    fn sort_button(&self, sort: ShelfSort, cx: &mut Context<Self>) -> Button {
        Button::new("shelf-sort")
            .ghost()
            .icon(Icon::new(IconName::SortAscending))
            .label(t!(sort.label_key()).to_string())
            .on_click(cx.listener(|this, _ev, _window, cx| this.cycle_sort(cx)))
    }

    /// 搜索框 + 状态筛选 + 收藏开关 + 排序。
    fn render_filters(
        &self,
        sort: ShelfSort,
        status: ShelfStatusFilter,
        favorite_only: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.search_input).cleanable(true)),
            )
            .child(self.status_button(
                "shelf-filter-all",
                ShelfStatusFilter::All,
                status,
                favorite_only,
                cx,
            ))
            .child(self.status_button(
                "shelf-filter-reading",
                ShelfStatusFilter::Reading,
                status,
                favorite_only,
                cx,
            ))
            .child(self.status_button(
                "shelf-filter-unread",
                ShelfStatusFilter::Unread,
                status,
                favorite_only,
                cx,
            ))
            .child(self.status_button(
                "shelf-filter-finished",
                ShelfStatusFilter::Finished,
                status,
                favorite_only,
                cx,
            ))
            .child(self.favorite_button(favorite_only, cx))
            .child(self.sort_button(sort, cx))
    }

    /// 空态：整库为空 → 「导入 EPUB / TXT」；筛选无结果 → 「清除筛选」。
    fn render_empty(&self, has_books: bool, cx: &mut Context<Self>) -> AnyElement {
        let empty = if has_books {
            EmptyState::new(
                IconName::Search,
                t!("Library.empty_filtered_title").into(),
            )
            .subtitle(t!("Library.empty_filtered_subtitle").into())
        } else {
            EmptyState::new(IconName::Library, t!("Library.empty_title").into())
                .subtitle(t!("Library.empty_subtitle").into())
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(empty)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("shelf-empty-import")
                            .primary()
                            .icon(Icon::new(IconName::Plus))
                            .label(t!("Library.action_import"))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.pick_and_import(cx);
                            })),
                    )
                    .child(
                        Button::new("shelf-empty-reset")
                            .ghost()
                            .label(t!("Library.action_clear_filters"))
                            .on_click(cx.listener(|this, _ev, window, cx| {
                                this.clear_filters(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 卡片网格 + 「加载更多」；整块纵向滚动。
    fn render_grid(
        &self,
        cards: Vec<CardData>,
        total: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let more = total > cards.len();
        let mut grid = div().flex().flex_wrap().gap_5().w_full().pb_2();
        for data in cards {
            let book_id = data.id.clone();
            let on_open: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> =
                Rc::new(cx.listener(move |this, _ev: &ClickEvent, window, cx| {
                    this.open_book(&book_id, window, cx);
                }));
            let favorite_id = data.id.clone();
            let on_favorite: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> = Rc::new(
                cx.listener(move |this, _ev: &ClickEvent, _window, cx| {
                    // 星标在卡片内部：不阻止冒泡会连带打开阅读器。
                    cx.stop_propagation();
                    this.model.update(cx, |model, _cx| {
                        model.toggle_book_favorite(&favorite_id);
                    });
                    cx.notify();
                }),
            );
            grid = grid.child(BookCard::new(data, on_open, on_favorite));
        }

        div()
            .id("shelf-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(grid)
            .when(more, |this| {
                this.child(
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .pt_3()
                        .child(
                            Button::new("shelf-load-more")
                                .ghost()
                                .label(t!("Library.action_load_more"))
                                .on_click(cx.listener(|this, _ev, _window, cx| {
                                    this.visible_limit += BATCH;
                                    cx.notify();
                                })),
                        ),
                )
            })
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }
}

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.maybe_auto_scan(cx);

        let limit = self.visible_limit;
        let (cards, total, has_books, scan_error, scan_in_flight, download_dir, sort, status, favorite_only) =
            self.model.read_with(cx, |model, _cx| {
                let covers_dir = model.paths.covers_dir.clone();
                let visible = visible_books(model.library.store.books(), &model.library.query);
                let total = visible.len();
                let cards: Vec<CardData> = visible
                    .iter()
                    .take(limit)
                    .map(|book| CardData::from_book(book, &covers_dir))
                    .collect();
                let has_books = !model.library.store.books().is_empty();
                (
                    cards,
                    total,
                    has_books,
                    model.library.last_error.clone(),
                    model.library.scan_in_flight,
                    model.config.download.download_path.clone(),
                    model.library.query.sort,
                    model.library.query.status,
                    model.library.query.favorite_only,
                )
            });

        let body: AnyElement = if total == 0 {
            self.render_empty(has_books, cx)
        } else {
            self.render_grid(cards, total, cx)
        };

        v_flex()
            .size_full()
            .p_6()
            .gap_3()
            .child(
                PageHeader::new(t!("Library.page_title").into())
                    .subtitle(
                        t!(
                            "Library.subtitle",
                            count = &total.to_string(),
                            dir = &download_dir
                        )
                        .into(),
                    )
                    .action(
                        Button::new("shelf-import")
                            .primary()
                            .icon(Icon::new(IconName::Plus))
                            .label(t!("Library.action_import"))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.pick_and_import(cx);
                            })),
                    )
                    .action(
                        Button::new("shelf-rescan")
                            .ghost()
                            .icon(Icon::new(IconName::Redo))
                            .label(t!("Library.action_rescan"))
                            .loading(scan_in_flight)
                            .disabled(scan_in_flight)
                            .on_click(cx.listener(|this, _ev, _window, cx| this.rescan(cx))),
                    ),
            )
            .child(self.render_filters(sort, status, favorite_only, cx))
            .when_some(scan_error, |this, error| {
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .child(format!("{}: {error}", t!("Library.scan_failed"))),
                )
            })
            .child(body)
    }
}
```

`src/desktop/pages/library/mod.rs` 整体替换为：

```rust
//! Library 页面：本地书库（卡片书架）。
//!
//! - 卡片网格手写（`flex_wrap`），不用 `List`：虚拟列表要求等高行，与封面网格对不上。
//! - 扫描 / 元数据提取在后台线程，drain 循环回传后自动重绘。
//! - 卡片点击通过 `RootView` 注入的回调打开阅读器。

mod card;
mod library_page;

pub use library_page::{LibraryPage, OpenReaderFn};
```

- [ ] **Step 4: 编译 + 测试**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿。若界面出现 `Library.subtitle` 之类的原始 key —— `rust_i18n` 在运行期查表，
编译期不会失败；这些 key 在 Task 23 补齐，此处只要求编译与测试通过。

- [ ] **Step 5: 手工验收（GUI）**

Run: `cargo run`

1. 进入「书架」：应看到下载目录里已有的 TXT / EPUB 以卡片出现，首次进入自动扫描；
   底部「N 本书 · <目录>」副标题正确。
2. 搜索框输入书名片段 → 网格实时过滤；清空后恢复。
3. 依次点「全部 / 在读 / 未读 / 已读」与「收藏」按钮：选中态高亮且互斥；「收藏」打开时
   状态回到「全部」。
4. 点「排序」按钮：文案在 最近阅读 → 最近添加 → 书名 → 大小 之间循环，顺序随之变化。
5. 点卡片 → 阅读器覆盖打开（Task 15 的骨架页）；关闭后回到书架。
6. 点星标：只切换收藏，**不**打开阅读器（`stop_propagation` 生效）。
7. 「导入」选 1–2 个 TXT → 卡片出现并弹成功 toast；「重新扫描」按钮在扫描期间转圈并禁用。

- [ ] **Step 6: 提交**

```bash
git add src/desktop/pages/library src/desktop/model/library.rs
git commit -m "feat(desktop): 书架卡片网格与筛选排序"
```

---

---

### Task 17: 书籍操作 / 编辑信息 / 信息 / 重新定位 弹窗

**Files:**
- Create: `src/desktop/system_open.rs`
- Create: `src/desktop/pages/library/dialogs.rs`
- Modify: `src/desktop/mod.rs`（`pub(crate) mod system_open;`）
- Modify: `src/desktop/pages/library/mod.rs`（`mod dialogs;`）
- Modify: `src/desktop/pages/library/card.rs`（「…」操作按钮）
- Modify: `src/desktop/pages/library/library_page.rs`（给卡片接上 `on_actions`）
- Modify: `src/desktop/model/library.rs`（追加 `restart_book`）

**Interfaces:**
- Consumes: `AppModel::{book_snapshot, relink_book, update_book_metadata, hide_book, delete_book_file, restart_book, push_error}`、`library::{write_cover, Book, BookFormat, ReadingProgress}`、`utils::formatting::{format_size, format_local_unix_secs}`。
- Produces:
  - `desktop::system_open::{reveal_in_file_manager, open_with_default_app}`（`pub(crate)`）
  - `LibraryPage::open_actions_dialog(&self, book_id, window, cx)`
  - `BookCard::with_actions(...)`

**弹窗实现约定（gpui-kit 的两个坑，先读再写）：**

1. `window.open_dialog(cx, build)` 的 `build` 是 **每帧重跑** 的 `Fn`：捕获的值必须先 clone
   再移进闭包，闭包里的 `on_click` 也只能捕获 clone 出来的局部变量。
2. 普通 `Dialog` 默认**不渲染任何按钮**（只有 `AlertDialog` 的 `.confirm()` 才有 OK/Cancel）。
   需要按钮就自己 `.footer(...)`，按完调 `window.close_dialog(cx)` 关闭。

- [ ] **Step 1: 系统打开 / 定位**

新建 `src/desktop/system_open.rs`：

```rust
//! 用系统默认程序打开文件、在文件管理器中定位文件。三平台各走命令行，不引新依赖。

use std::path::Path;
use std::process::Command;

/// 在文件管理器中显示并选中文件。
///
/// # Errors
///
/// 平台命令启动失败时返回错误文案（可直接进 toast）。
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let (program, args) = reveal_command(path);
    Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 用系统默认程序打开文件。
///
/// # Errors
///
/// 平台命令启动失败时返回错误文案。
pub fn open_with_default_app(path: &Path) -> Result<(), String> {
    let (program, args) = open_command(path);
    Command::new(program)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 「在文件管理器中显示」的平台命令 `(程序, 参数)`。
#[must_use]
pub fn reveal_command(path: &Path) -> (String, Vec<String>) {
    let target = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        // `explorer /select,<路径>` 会打开父目录并选中该文件。
        ("explorer".to_string(), vec![format!("/select,{target}")])
    } else if cfg!(target_os = "macos") {
        ("open".to_string(), vec!["-R".to_string(), target])
    } else {
        // Linux 没有统一的「选中」，退回打开父目录。
        let parent = path.parent().map_or(target, |dir| dir.to_string_lossy().into_owned());
        ("xdg-open".to_string(), vec![parent])
    }
}

/// 「用系统程序打开」的平台命令 `(程序, 参数)`。
#[must_use]
pub fn open_command(path: &Path) -> (String, Vec<String>) {
    let target = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        // 第一个空串是窗口标题占位：少了它，`start` 会把带引号的路径当成标题。
        (
            "cmd".to_string(),
            vec![
                "/C".to_string(),
                "start".to_string(),
                String::new(),
                target,
            ],
        )
    } else if cfg!(target_os = "macos") {
        ("open".to_string(), vec![target])
    } else {
        ("xdg-open".to_string(), vec![target])
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::Path;

    use super::{open_command, reveal_command};

    #[test]
    fn commands_carry_the_path() {
        let path = Path::new("/tmp/a b.txt");

        let (reveal_program, reveal_args) = reveal_command(path);
        assert!(!reveal_program.is_empty());
        assert!(reveal_args.iter().any(|arg| arg.contains("a b.txt")));

        let (open_program, open_args) = open_command(path);
        assert!(!open_program.is_empty());
        assert!(open_args.iter().any(|arg| arg.contains("a b.txt")));
    }
}
```

`src/desktop/mod.rs` 的模块列表按字母序插入：

```rust
pub(crate) mod system_open;
```

- [ ] **Step 2: 跑测试确认通过**

Run: `cargo test --lib desktop::system_open:: -- --nocapture`

Expected: PASS（1 个测试）。

- [ ] **Step 3: `AppModel::restart_book`**

`src/desktop/model/library.rs` 的 `impl AppModel` 内追加：

```rust
    /// 从头开始：清空阅读进度、保留阅读方式并立即落盘。
    pub fn restart_book(&mut self, id: &str) {
        let Some(book) = self.library.store.get(id) else {
            return;
        };
        let mode = book.progress.mode;
        self.library.store.record_progress(id, 0, 0.0, mode, false);
        self.library.bump();
        self.persist_library();
    }
```

- [ ] **Step 4: 卡片加「…」按钮**

`src/desktop/pages/library/card.rs`：

1) `BookCard` 增加字段与构造器：

```rust
/// 书架卡片。
#[derive(IntoElement)]
pub(super) struct BookCard {
    data: CardData,
    on_open: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
    on_favorite: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
    on_actions: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl BookCard {
    pub(super) fn new(
        data: CardData,
        on_open: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
        on_favorite: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
    ) -> Self {
        Self {
            data,
            on_open,
            on_favorite,
            on_actions: None,
        }
    }

    /// 挂上「…」操作按钮（Task 17 的书籍操作弹窗）。
    pub(super) fn with_actions(
        mut self,
        handler: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
    ) -> Self {
        self.on_actions = Some(handler);
        self
    }
}
```

2) render 内 destructure 补 `on_actions`，外层卡片加 `.group("shelf-card")`，并在封面之后追加「…」按钮：

```rust
        let Self {
            data,
            on_open,
            on_favorite,
            on_actions,
        } = self;
```

```rust
        // 右下「…」：默认不可见（`invisible` 同时挡住命中测试），悬停卡片时出现。
        if let Some(on_actions) = on_actions {
            cover = cover.child(
                div()
                    .id(ElementId::Name(format!("book-actions-{}", data.id).into()))
                    .absolute()
                    .bottom_1()
                    .right_1()
                    .w(px(22.))
                    .h(px(22.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgba(0x00000066))
                    .cursor_pointer()
                    .invisible()
                    .group_hover("shelf-card", |style| style.visible())
                    .child(Icon::new(IconName::Ellipsis).text_color(rgb(0xffffff)))
                    .on_click(move |event, window, cx| (on_actions)(event, window, cx)),
            );
        }
```

3) 外层卡片 div 加 `.group("shelf-card")`（放在 `.id(...)` 之后）：

```rust
        let mut card = div()
            .id(ElementId::Name(format!("book-card-{}", data.id).into()))
            .group("shelf-card")
            .w(px(COVER_W))
```

`IconName::Ellipsis` / `.invisible()` / `.group_hover(...)` / `.bottom_1()` / `.right_1()` 均已确认存在。

- [ ] **Step 5: 写弹窗**

新建 `src/desktop/pages/library/dialogs.rs`：

```rust
//! 书架弹窗：书籍操作、编辑信息、书籍信息、移出 / 删除确认、重新定位。
//!
//! 所有弹窗都从 `LibraryPage` 的方法打开；`window.open_dialog` 的 builder 每帧重跑，
//! 因此状态要么放进 entity（`EditState`），要么按帧从 `AppModel` 重读。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::dialog::{AlertDialog, Dialog};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, IntoElement, ObjectFit, ParentElement, Styled, Window,
    div, img, px,
};
use rust_i18n::t;

use super::library_page::LibraryPage;
use crate::desktop::components::format_size;
use crate::desktop::system_open;
use crate::library::{BookFormat, write_cover};
use crate::utils::formatting::format_local_unix_secs;

/// 编辑信息弹窗的临时状态：保存前不改动 store。
pub(super) struct EditState {
    /// 正在编辑的书。
    book_id: String,
    /// store 里当前的封面缓存文件名。
    cover_file: Option<String>,
    /// 新选的封面原图（保存时才写进缓存目录）。
    picked_cover: Option<PathBuf>,
    /// 用户点了「移除封面」。
    remove_cover: bool,
}

impl LibraryPage {
    /// 书籍操作弹窗（「…」按钮）。
    pub(super) fn open_actions_dialog(
        &self,
        book_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let page = cx.entity().downgrade();
        let id = book.id.clone();
        let title = book.title.clone();
        let path = PathBuf::from(&book.path);
        let readable = book.format.is_readable();
        let missing = book.missing;
        let finished = book.progress.finished;

        window.open_dialog(cx, move |dialog, _window, _cx| {
            let page = page.clone();
            let id = id.clone();
            let path = path.clone();

            // 「继续阅读」/「从头开始」/「重新定位」共用同一个入口。
            let continue_label = if missing {
                t!("Library.actions.relink")
            } else {
                t!("Library.actions.continue")
            }
            .to_string();
            let continue_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-continue")
                    .w_full()
                    .ghost()
                    .label(continue_label)
                    .disabled(!missing && !readable)
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| {
                            if missing {
                                page.pick_relink(&id, cx);
                            } else {
                                page.open_book(&id, window, cx);
                            }
                        });
                    })
            };

            let restart_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-restart")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.restart").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| {
                            page.model.update(cx, |model, _cx| model.restart_book(&id));
                            page.open_book(&id, window, cx);
                        });
                    })
            };

            let info_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-info")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.info").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| page.open_info_dialog(&id, window, cx));
                    })
            };

            let edit_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-edit")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.edit").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| page.open_edit_dialog(&id, window, cx));
                    })
            };

            let reveal_button = {
                let path = path.clone();
                Button::new("book-action-reveal")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.reveal").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        if let Err(error) = system_open::reveal_in_file_manager(&path) {
                            tracing::warn!("在文件管理器中显示失败: {error}");
                        }
                    })
            };

            let open_external_button = {
                let path = path.clone();
                Button::new("book-action-open-external")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.open_external").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        if let Err(error) = system_open::open_with_default_app(&path) {
                            tracing::warn!("用系统程序打开失败: {error}");
                        }
                    })
            };

            let hide_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-hide")
                    .w_full()
                    .ghost()
                    .label(t!("Library.actions.hide").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| page.prompt_hide(&id, window, cx));
                    })
            };

            let delete_button = {
                let page = page.clone();
                let id = id.clone();
                Button::new("book-action-delete")
                    .w_full()
                    .danger()
                    .label(t!("Library.actions.delete_file").to_string())
                    .on_click(move |_ev, window, cx| {
                        window.close_dialog(cx);
                        let _ = page.update(cx, |page, cx| {
                            page.prompt_delete_file(&id, window, cx);
                        });
                    })
            };

            dialog
                .title(t!("Library.actions.title", title = &title).to_string())
                .w(px(320.))
                .child(
                    v_flex()
                        .gap_1()
                        .child(continue_button)
                        .when(finished && !missing, |this| this.child(restart_button))
                        .child(info_button)
                        .child(edit_button)
                        .child(reveal_button)
                        .child(open_external_button)
                        .child(hide_button)
                        .child(delete_button),
                )
        });
    }

    /// 书籍信息（只读）。
    pub(super) fn open_info_dialog(
        &self,
        book_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let unsupported = !book.format.is_readable();
        window.open_dialog(cx, move |dialog, _window, cx| {
            let book = book.clone();
            let unknown = t!("Library.info.unknown").to_string();
            let chapters = book
                .chapter_count
                .map_or_else(|| unknown.clone(), |count| count.to_string());
            dialog
                .title(t!("Library.info.title").to_string())
                .w(px(560.))
                .child(
                    v_flex()
                        .gap_1()
                        .text_sm()
                        .child(info_row(
                            t!("Library.info.format").to_string(),
                            book.format.label().to_string(),
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.path").to_string(),
                            book.path.clone(),
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.size").to_string(),
                            format_size(book.size_bytes),
                            cx,
                        ))
                        .child(info_row(t!("Library.info.chapters").to_string(), chapters, cx))
                        .child(info_row(
                            t!("Library.info.added").to_string(),
                            format_local_unix_secs(
                                book.added_unix,
                                "Library.info.unknown",
                                "Library.info.unknown",
                                "Library.info.unknown",
                            ),
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.progress").to_string(),
                            format!("{} · {:.0}%", t!(book.status_key()), book.progress.percent),
                            cx,
                        ))
                        .child(info_row(
                            t!("Library.info.bookmarks").to_string(),
                            book.bookmarks.len().to_string(),
                            cx,
                        ))
                        .when(unsupported, |this| {
                            this.child(
                                div()
                                    .mt_2()
                                    .text_sm()
                                    .text_color(cx.theme().warning)
                                    .child(t!("Library.status.unsupported").to_string()),
                            )
                        }),
                )
        });
    }

    /// 编辑书名 / 作者 / 封面。
    pub(super) fn open_edit_dialog(
        &self,
        book_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let model = self.model.clone();
        let covers_dir = self.model.read(cx).paths.covers_dir.clone();

        let title_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("Library.edit.title_placeholder").to_string())
        });
        title_input.update(cx, |state, cx| state.set_value(book.title.clone(), window, cx));
        let author_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("Library.edit.author_placeholder").to_string())
        });
        author_input.update(cx, |state, cx| state.set_value(book.author.clone(), window, cx));

        let state = cx.new(|_cx| EditState {
            book_id: book.id.clone(),
            cover_file: book.cover_file.clone(),
            picked_cover: None,
            remove_cover: false,
        });

        window.open_dialog(cx, move |dialog, _window, cx| {
            // 每帧重跑：先取当前值，再 clone 进各按钮的闭包。
            let (picked, remove_cover, cover_file) = {
                let form = state.read(cx);
                (
                    form.picked_cover.clone(),
                    form.remove_cover,
                    form.cover_file.clone(),
                )
            };
            let preview = cover_preview(picked.as_deref(), remove_cover, cover_file.as_deref(), &covers_dir, cx);

            let pick_state = state.clone();
            let pick_button = Button::new("edit-cover-pick")
                .ghost()
                .label(t!("Library.edit.pick_cover").to_string())
                .on_click(move |_ev, _window, cx| {
                    let state = pick_state.clone();
                    cx.spawn(async move |async_cx| {
                        let file = rfd::AsyncFileDialog::new()
                            .add_filter(
                                t!("Library.edit.cover_filter").as_ref(),
                                &["png", "jpg", "jpeg", "webp", "gif"],
                            )
                            .set_title(t!("Library.edit.pick_cover").as_ref())
                            .pick_file()
                            .await;
                        let Some(file) = file else {
                            return;
                        };
                        let path = file.path().to_path_buf();
                        let _ = state.update(async_cx, |form, cx| {
                            form.picked_cover = Some(path);
                            form.remove_cover = false;
                            cx.notify();
                        });
                    })
                    .detach();
                });

            let remove_state = state.clone();
            let remove_button = Button::new("edit-cover-remove")
                .ghost()
                .label(t!("Library.edit.remove_cover").to_string())
                .on_click(move |_ev, _window, cx| {
                    remove_state.update(cx, |form, cx| {
                        form.picked_cover = None;
                        form.remove_cover = true;
                        cx.notify();
                    });
                });

            let save_model = model.clone();
            let save_covers_dir = covers_dir.clone();
            let save_state = state.clone();
            let save_title = title_input.clone();
            let save_author = author_input.clone();
            let save_button = Button::new("edit-save")
                .primary()
                .label(t!("Library.edit.save").to_string())
                .on_click(move |_ev, window, cx| {
                    let title = save_title.read(cx).value().to_string();
                    let author = save_author.read(cx).value().to_string();
                    let (id, picked, remove, old_cover) = {
                        let form = save_state.read(cx);
                        (
                            form.book_id.clone(),
                            form.picked_cover.clone(),
                            form.remove_cover,
                            form.cover_file.clone(),
                        )
                    };
                    let cover_file = if let Some(path) = picked {
                        std::fs::read(&path)
                            .ok()
                            .and_then(|bytes| write_cover(&save_covers_dir, &id, &bytes))
                            .or(old_cover)
                    } else if remove {
                        None
                    } else {
                        old_cover
                    };
                    save_model.update(cx, |model, _cx| {
                        model.update_book_metadata(&id, title.trim(), author.trim(), cover_file);
                    });
                    window.close_dialog(cx);
                });

            let cancel_button = Button::new("edit-cancel")
                .ghost()
                .label(t!("Library.edit.cancel").to_string())
                .on_click(|_ev, window, cx| window.close_dialog(cx));

            dialog
                .title(t!("Library.edit.title").to_string())
                .w(px(460.))
                .child(
                    h_flex()
                        .items_start()
                        .gap_4()
                        .child(v_flex().gap_2().child(preview).child(h_flex().gap_2().child(pick_button).child(remove_button)))
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_2()
                                .child(Input::new(&title_input))
                                .child(Input::new(&author_input)),
                        ),
                )
                .footer(h_flex().justify_end().gap_2().child(cancel_button).child(save_button))
        });
    }

    /// 移出书架（二次确认）。
    pub(super) fn prompt_hide(&self, book_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let model = self.model.clone();
        let id = book.id.clone();
        let title = book.title.clone();
        window.open_alert_dialog(cx, move |alert: AlertDialog, _window, _cx| {
            let model = model.clone();
            let id = id.clone();
            alert
                .title(t!("Library.hide_dialog.title"))
                .description(t!("Library.hide_dialog.message", title = &title))
                .ok_text(t!("Library.hide_dialog.confirm_button"))
                .cancel_text(t!("Library.dialog.cancel_button"))
                .confirm()
                .on_ok(move |_ev: &ClickEvent, _window, cx| {
                    model.update(cx, |model, _cx| model.hide_book(&id));
                    true
                })
        });
    }

    /// 删除磁盘文件（二次确认，危险操作）。
    pub(super) fn prompt_delete_file(
        &self,
        book_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let model = self.model.clone();
        let id = book.id.clone();
        let title = book.title.clone();
        window.open_alert_dialog(cx, move |alert: AlertDialog, _window, _cx| {
            let model = model.clone();
            let id = id.clone();
            alert
                .title(t!("Library.delete_file_dialog.title"))
                .description(t!("Library.delete_file_dialog.message", title = &title))
                .ok_text(t!("Library.delete_file_dialog.confirm_button"))
                .cancel_text(t!("Library.dialog.cancel_button"))
                .ok_variant(ButtonVariant::Danger)
                .confirm()
                .on_ok(move |_ev: &ClickEvent, _window, cx| {
                    model.update(cx, |model, _cx| model.delete_book_file(&id));
                    true
                })
        });
    }

    /// 重新定位：选一个新文件，格式必须与原书一致。
    pub(super) fn pick_relink(&self, book_id: &str, cx: &mut Context<Self>) {
        let Some(book) = self.model.read(cx).book_snapshot(book_id) else {
            return;
        };
        let format = book.format;
        let model = self.model.clone();
        let page = cx.entity().downgrade();
        let id = book.id.clone();
        cx.spawn(async move |_weak, async_cx| {
            let file = rfd::AsyncFileDialog::new()
                .add_filter(format.label(), &[extension_of(format)])
                .set_title(t!("Library.relink.dialog_title").as_ref())
                .pick_file()
                .await;
            let Some(file) = file else {
                return;
            };
            let path = file.path().to_path_buf();
            let _ = page.update(async_cx, |_page, cx| {
                model.update(cx, |model, _cx| {
                    if let Err(error) = model.relink_book(&id, &path) {
                        model.push_error(error.message());
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }
}

/// 编辑弹窗的封面预览：新选的图 > 「已移除」占位 > 现有缓存封面 > 无封面占位。
fn cover_preview(
    picked: Option<&std::path::Path>,
    remove_cover: bool,
    cover_file: Option<&str>,
    covers_dir: &std::path::Path,
    cx: &App,
) -> Div {
    let frame = div()
        .w(px(120.))
        .h(px(180.))
        .rounded_md()
        .overflow_hidden()
        .bg(cx.theme().muted);
    if let Some(path) = picked {
        return frame.child(img(path).size_full().object_fit(ObjectFit::Cover));
    }
    if !remove_cover
        && let Some(file) = cover_file
    {
        return frame.child(
            img(covers_dir.join(file))
                .size_full()
                .object_fit(ObjectFit::Cover),
        );
    }
    frame
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(t!("Library.edit.no_cover").to_string()),
        )
}

/// 一行「标签：值」。
fn info_row(label: String, value: String, cx: &App) -> Div {
    h_flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .w(px(88.))
                .flex_none()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().child(value))
}

/// 文件对话框过滤用的扩展名（无点、小写）。
fn extension_of(format: BookFormat) -> &'static str {
    match format {
        BookFormat::Txt => "txt",
        BookFormat::Epub => "epub",
        BookFormat::Markdown => "md",
        BookFormat::Html => "html",
        BookFormat::Zip => "zip",
        BookFormat::Pdf => "pdf",
    }
}
```

封面字节只在「保存」时读取并写进缓存目录，选择阶段只记路径；关闭弹窗（不保存）不会改动
store，也不会留下孤儿封面文件。

- [ ] **Step 6: 接上模块与卡片回调**

`src/desktop/pages/library/mod.rs`：

```rust
mod card;
mod dialogs;
mod library_page;

pub use library_page::{LibraryPage, OpenReaderFn};
```

`src/desktop/pages/library/library_page.rs` 的 `render_grid` 里，把卡片构造改成：

```rust
            let actions_id = data.id.clone();
            let on_actions: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> =
                Rc::new(cx.listener(move |this, _ev: &ClickEvent, window, cx| {
                    this.open_actions_dialog(&actions_id, window, cx);
                }));
            grid = grid.child(
                BookCard::new(data, on_open, on_favorite).with_actions(on_actions),
            );
```

（`on_favorite` 的构造保持不变。）

- [ ] **Step 7: 编译 + 测试**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿。

- [ ] **Step 8: 手工验收（GUI）**

Run: `cargo run`

1. 悬停卡片 → 右下浮出「…」；移开消失；点「…」弹出操作列表。
2. 「继续阅读」打开阅读器；已读完的书多出「从头开始」，点了从头读。
3. 「书籍信息」列出格式 / 路径 / 大小 / 章节数 / 添加时间 / 进度 / 书签数。
4. 「编辑信息」改名改作者 → 保存后卡片立刻更新；再扫描一次不会被覆盖（`metadata_edited`）。
5. 「选择图片」选一张 PNG → 预览变化 → 保存后卡片封面更新；「移除封面」→ 保存后回落生成封面。
6. 「在文件夹中显示」打开资源管理器并选中文件；「用系统程序打开」用默认程序打开。
7. 「移出书架」确认后卡片消失，重新扫描也不回来；「删除文件」确认后磁盘文件被删。
8. 把书文件改名 → 重新扫描后卡片显示「文件不存在」→ 操作弹窗出现「重新定位」→
   选新路径后恢复正常（选错格式的文件时弹错误通知）。

- [ ] **Step 9: 提交**

```bash
git add src/desktop/system_open.rs src/desktop/mod.rs src/desktop/pages/library src/desktop/model/library.rs
git commit -m "feat(desktop): 书籍操作、编辑信息与重新定位弹窗"
```

---

---

### Task 18: 滚动模式、目录抽屉、排版面板与进度落盘

**Files:**
- Create: `src/desktop/pages/reader/toc.rs`
- Create: `src/desktop/pages/reader/typography.rs`
- Modify: `src/desktop/pages/reader/mod.rs`
- Modify: `src/desktop/pages/reader/reader_page.rs`（整体重写 Task 15 的骨架实现）

**Interfaces:**
- Consumes: `reader::{ReaderBook, ReaderLayout, load_book}`、`config::{ReaderConfig, ReaderTheme, ReadingMode}`、`reader_colors`、`AppModel::{record_reading_progress, flush_library, persist_settings}`。
- Produces（Task 19 / 20 复用）：
  - `ReaderPage::reader_config(&self, cx: &App) -> ReaderConfig`（已 clamp）
  - `ReaderPage::update_reader_config(&mut self, update: impl FnOnce(&mut ReaderConfig), cx)`
  - `ReaderPage::{render_toc, render_typography, goto_chapter, scroll_relative, schedule_progress_commit}`
  - `ReaderPage` 的 `pub(super)` 字段：`book`、`content`、`load_state`、`mode`、`chapter`、`chapter_ratio`、`scroll`、`toc_titles`、`toc_filter`、`toc_open`、`typography_open`

**平台事实（先读再写，避免踩坑）：**

- gpui **没有** `mx_auto` 之类的 auto margin；居中一律用 `flex().flex_row().justify_center()` + 内层 `w_full().max_w(..)`。
- `ScrollHandle::{offset, max_offset, set_offset, bounds}`，`set_offset` 是取 `&self` 的普通方法。
  `max_offset()` 要等布局算完才有值，所以位置恢复必须放到 `cx.on_next_frame()`。
- 滚动条叠加用 `ScrollableElement::vertical_scrollbar(&handle)`（`gpui_kit::component::scroll`），
  它只是给同一个滚动元素加一层 `ScrollbarLayer`，不额外包 `Scrollable`。
- 文字样式：`.text_size(px(f32))`、`.line_height(px(f32))`、`.font_family(impl Into<SharedString>)`。
- `t!` 返回 `Cow<'static, str>`，进 `Button::label` / `.child` / `AlertDialog` 都能直接用；
  要拼字符串就先 `.to_string()`。

- [ ] **Step 1: 目录抽屉**

新建 `src/desktop/pages/reader/toc.rs`：

```rust
//! 目录抽屉：章节标题过滤 + 虚拟列表，点条目跳章。

use std::rc::Rc;

use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, Styled, div, px, uniform_list,
};
use rust_i18n::t;

use super::reader_page::ReaderPage;

impl ReaderPage {
    /// 目录抽屉。没有内容（加载中 / 失败）时给一句占位而不是空面板。
    pub(super) fn render_toc(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(titles) = self.toc_titles.clone() else {
            return div()
                .w(px(300.0))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t!("Reader.toc.empty").to_string())
                .into_any_element();
        };

        // 过滤每帧现算：几千条章节 O(n) 扫描比维护一份过滤索引简单，且只在抽屉打开时发生。
        let needle = self.toc_filter.read(cx).value().trim().to_lowercase();
        let indices: Rc<Vec<usize>> = Rc::new(
            (0..titles.len())
                .filter(|index| {
                    needle.is_empty() || titles[*index].to_lowercase().contains(&needle)
                })
                .collect(),
        );
        let filtered = indices.len();
        let total = titles.len();
        let current = self.chapter;
        let handle = cx.entity().downgrade();
        let list_titles = titles;
        let list_indices = indices;

        let list = uniform_list("reader-toc-list", filtered, move |range, _window, cx| {
            let active_bg = cx.theme().selection;
            let hover_bg = cx.theme().list_hover;
            range
                .map(|slot| {
                    let index = list_indices[slot];
                    let title = list_titles[index].clone();
                    let handle = handle.clone();
                    let is_current = index == current;
                    div()
                        .id(("reader-toc-item", index))
                        .w_full()
                        .h(px(32.0))
                        .px_3()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_sm()
                        .when(is_current, |this| {
                            this.font_weight(FontWeight::SEMIBOLD).bg(active_bg)
                        })
                        .hover(move |style| style.bg(hover_bg))
                        .child(div().truncate().child(SharedString::from(title)))
                        .on_click(move |_event, _window, cx| {
                            let _ = handle.update(cx, |page, cx| page.goto_chapter(index, cx));
                        })
                })
                .collect::<Vec<_>>()
        });

        div()
            .w(px(300.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Reader.toc.title").to_string()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        t!(
                                            "Reader.toc.chapter_count",
                                            count = &total.to_string()
                                        )
                                        .to_string(),
                                    ),
                            ),
                    )
                    .child(Input::new(&self.toc_filter).cleanable(true).small()),
            )
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }
}
```

- [ ] **Step 2: 排版面板**

新建 `src/desktop/pages/reader/typography.rs`：

```rust
//! 顶栏 `Aa` 面板：主题、字号、行高、段距、页宽、首行缩进。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{ClickEvent, Context, Div, FontWeight, ParentElement, Styled, div, px};
use rust_i18n::t;

use super::reader_page::ReaderPage;
use crate::config::{ReaderConfig, ReaderTheme};

/// 面板里五个主题按钮的顺序。
const THEMES: [ReaderTheme; 5] = [
    ReaderTheme::Paper,
    ReaderTheme::Sepia,
    ReaderTheme::Night,
    ReaderTheme::Green,
    ReaderTheme::App,
];

/// 可以用「− 值 +」调整的四项数值。
#[derive(Debug, Clone, Copy)]
enum Field {
    FontSize,
    LineHeight,
    ParagraphSpacing,
    PageWidth,
}

impl Field {
    const ALL: [Self; 4] = [
        Self::FontSize,
        Self::LineHeight,
        Self::ParagraphSpacing,
        Self::PageWidth,
    ];

    const fn slug(self) -> &'static str {
        match self {
            Self::FontSize => "font-size",
            Self::LineHeight => "line-height",
            Self::ParagraphSpacing => "paragraph-spacing",
            Self::PageWidth => "page-width",
        }
    }

    const fn label_key(self) -> &'static str {
        match self {
            Self::FontSize => "Reader.typography.font_size",
            Self::LineHeight => "Reader.typography.line_height",
            Self::ParagraphSpacing => "Reader.typography.paragraph_spacing",
            Self::PageWidth => "Reader.typography.page_width",
        }
    }

    const fn step(self) -> f32 {
        match self {
            Self::FontSize => 1.0,
            Self::LineHeight => 0.1,
            Self::ParagraphSpacing => 2.0,
            Self::PageWidth => 40.0,
        }
    }

    fn value(self, reader: &ReaderConfig) -> f32 {
        match self {
            Self::FontSize => reader.font_size,
            Self::LineHeight => reader.line_height,
            Self::ParagraphSpacing => reader.paragraph_spacing,
            Self::PageWidth => reader.page_width,
        }
    }

    fn set(self, reader: &mut ReaderConfig, value: f32) {
        match self {
            Self::FontSize => reader.font_size = value,
            Self::LineHeight => reader.line_height = value,
            Self::ParagraphSpacing => reader.paragraph_spacing = value,
            Self::PageWidth => reader.page_width = value,
        }
    }

    /// 步进值的小数位跟着 `step()` 走，避免 1.7000000000000002 这种尾巴。
    fn format(self, value: f32) -> String {
        match self {
            Self::LineHeight => format!("{value:.1}"),
            Self::FontSize | Self::ParagraphSpacing | Self::PageWidth => format!("{value:.0}"),
        }
    }
}

impl ReaderPage {
    /// 浮在正文右上角的排版面板；每次改动即时生效，关闭时才落盘。
    pub(super) fn render_typography(&self, cx: &mut Context<Self>) -> Div {
        let reader = self.reader_config(cx);
        let mut panel = div()
            .absolute()
            .top(px(52.0))
            .right(px(12.0))
            .w(px(300.0))
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_lg()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("Reader.typography.title").to_string()),
            )
            .child(self.render_theme_row(reader.theme, cx));

        for field in Field::ALL {
            panel = panel.child(self.render_field_row(field, &reader, cx));
        }

        panel.child(self.render_indent_row(reader.indent, cx))
    }

    fn render_theme_row(&self, current: ReaderTheme, cx: &mut Context<Self>) -> Div {
        let mut row = div().flex().flex_row().flex_wrap().gap_2();
        for theme in THEMES {
            let mut button =
                Button::new(format!("reader-theme-{}", theme.as_str())).label(t!(theme.label_key()));
            button = if theme == current {
                button.primary()
            } else {
                button.ghost()
            };
            row = row.child(button.on_click(cx.listener(
                move |this, _event: &ClickEvent, _window, cx| {
                    this.update_reader_config(move |reader| reader.theme = theme, cx);
                },
            )));
        }
        row
    }

    fn render_field_row(
        &self,
        field: Field,
        reader: &ReaderConfig,
        cx: &mut Context<Self>,
    ) -> Div {
        let value = field.value(reader);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!(field.label_key()).to_string()),
            )
            .child(
                Button::new(format!("reader-{}-minus", field.slug()))
                    .ghost()
                    .compact()
                    .label("−")
                    .on_click(cx.listener(
                        move |this, _event: &ClickEvent, _window, cx| {
                            this.update_reader_config(
                                move |reader| {
                                    let next = field.value(reader) - field.step();
                                    field.set(reader, next);
                                },
                                cx,
                            );
                        },
                    )),
            )
            .child(
                div()
                    .w(px(48.0))
                    .text_center()
                    .text_sm()
                    .child(field.format(value)),
            )
            .child(
                Button::new(format!("reader-{}-plus", field.slug()))
                    .ghost()
                    .compact()
                    .label("+")
                    .on_click(cx.listener(
                        move |this, _event: &ClickEvent, _window, cx| {
                            this.update_reader_config(
                                move |reader| {
                                    let next = field.value(reader) + field.step();
                                    field.set(reader, next);
                                },
                                cx,
                            );
                        },
                    )),
            )
    }

    fn render_indent_row(&self, indent: bool, cx: &mut Context<Self>) -> Div {
        let mut button = Button::new("reader-indent").label(t!("Reader.typography.indent"));
        button = if indent {
            button.primary()
        } else {
            button.ghost()
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("Reader.typography.indent").to_string()),
            )
            .child(button.on_click(cx.listener(
                |this, _event: &ClickEvent, _window, cx| {
                    this.update_reader_config(|reader| reader.indent = !reader.indent, cx);
                },
            )))
    }
}
```

- [ ] **Step 3: 注册子模块**

`src/desktop/pages/reader/mod.rs` 整体替换为：

```rust
//! 应用内阅读器：覆盖式视图，不作为第 7 个导航页。

mod toc;
mod typography;

pub mod reader_page;
pub mod theme;

pub use reader_page::{CloseReaderFn, LoadState, ReaderPage};
pub use theme::reader_colors;
```

- [ ] **Step 4: 重写 `reader_page.rs`**

`src/desktop/pages/reader/reader_page.rs` 整体替换为：

```rust
//! 阅读器页面：加载状态机 + 滚动模式正文 + 目录抽屉 / 排版面板。
//!
//! 翻页模式由 Task 19 接进 `render_body`；本文件把滚动模式的位置恢复、进度采样
//! 与自动落盘做完整。

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, FontWeight, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement, Render, ScrollEvent, ScrollHandle, SharedString,
    Styled, Window, div, point, px,
};
use rust_i18n::t;

use super::theme::reader_colors;
use crate::config::{ReaderConfig, ReadingMode};
use crate::desktop::model::AppModel;
use crate::library::{Book, ReadingProgress};
use crate::reader::{ReaderBook, ReaderError, ReaderLayout, load_book};

/// 关闭阅读器：由 `RootView` 注入（需要同时改 RootView 状态与全屏标记）。
pub type CloseReaderFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// 打开状态。
pub enum LoadState {
    Loading,
    Ready,
    Error(ReaderError),
}

/// 阅读器页面。
pub struct ReaderPage {
    model: Entity<AppModel>,
    book_id: String,
    /// 打开时的书籍快照；关闭时用于落盘。
    pub(super) book: Book,
    pub(super) content: Option<ReaderBook>,
    pub(super) load_state: LoadState,
    pub(super) mode: ReadingMode,
    pub(super) chapter: usize,
    pub(super) chapter_ratio: f32,
    /// 已到达书末并置位；一旦为 true 不再回落。
    finished: bool,
    /// 滚动模式的滚动位置。
    pub(super) scroll: ScrollHandle,
    /// 待恢复的滚动位置：`max_offset` 要等布局完成，所以恢复放到下一帧。
    restore_pending: bool,
    /// 每次滚动自增，只有最新一代的防抖任务会写进度。
    scroll_generation: u64,
    /// 章节标题快照；加载完成时建一次，抽屉每帧零拷贝共享。
    pub(super) toc_titles: Option<Rc<Vec<String>>>,
    /// 目录顶部的章节过滤框。
    pub(super) toc_filter: Entity<InputState>,
    pub(super) toc_open: bool,
    pub(super) typography_open: bool,
    immersive: bool,
    generation: u64,
    progress_dirty: bool,
    focus: FocusHandle,
    close_handler: Option<CloseReaderFn>,
}

impl ReaderPage {
    /// 打开一本书并启动后台加载。
    pub fn new(
        model: Entity<AppModel>,
        book: Book,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let toc_filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("Reader.toc.filter_placeholder").to_string())
        });
        let finished = book.progress.finished;
        let mut page = Self {
            model,
            book_id: book.id.clone(),
            mode: book.progress.mode,
            chapter: book.progress.chapter,
            chapter_ratio: book.progress.chapter_ratio,
            book,
            content: None,
            load_state: LoadState::Loading,
            finished,
            scroll: ScrollHandle::new(),
            restore_pending: true,
            scroll_generation: 0,
            toc_titles: None,
            toc_filter,
            toc_open: false,
            typography_open: false,
            immersive: false,
            generation: 1,
            progress_dirty: false,
            focus,
            close_handler: None,
        };
        // 过滤框内容变化要让抽屉重画；不清空的话列表会停在旧结果上。
        cx.subscribe(&page.toc_filter, |_this, _input, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        page.spawn_load(cx);
        page
    }

    /// 注入关闭回调（`RootView` 调用）。
    pub fn set_close_handler(&mut self, handler: CloseReaderFn) {
        self.close_handler = Some(handler);
    }

    /// 是否处于沉浸模式（`RootView` 据此隐藏 TitleBar）。
    #[must_use]
    pub const fn is_immersive(&self) -> bool {
        self.immersive
    }

    /// 落盘进度与阅读设置；随后由 `RootView` 丢弃本实体。
    pub fn close_and_flush(&mut self, cx: &mut Context<Self>) {
        self.record_progress(cx);
        self.progress_dirty = false;
        self.persist_settings(cx);
        self.model.update(cx, |model, _| model.flush_library());
    }

    /// 当前生效的排版参数（已 clamp）。
    pub(super) fn reader_config(&self, cx: &App) -> ReaderConfig {
        self.model.read(cx).config.reader.clamped()
    }

    /// 改一项排版参数：写回配置、保持当前阅读比例，并立刻重画。
    ///
    /// 不在这里落盘 —— 面板关闭 / 关闭阅读器时才写 config.toml，拖动步进不会刷盘。
    pub(super) fn update_reader_config(
        &mut self,
        update: impl FnOnce(&mut ReaderConfig),
        cx: &mut Context<Self>,
    ) {
        // 字号一变内容高度就变，先把「读到哪」按比例记下来，下一帧再按比例恢复。
        self.capture_scroll_ratio();
        self.restore_pending = true;
        self.model.update(cx, |model, _cx| {
            let mut reader = model.config.reader.clamped();
            update(&mut reader);
            model.config.reader = reader.clamped();
        });
        cx.notify();
    }

    fn persist_settings(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, _| model.persist_settings());
    }

    fn spawn_load(&mut self, cx: &mut Context<Self>) {
        let generation = self.generation;
        let path = PathBuf::from(&self.book.path);
        let format = self.book.format;
        let encoding = self.book.encoding.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_book(&path, format, encoding.as_deref()) })
                .await;
            let _ = this.update(cx, |page, cx| page.finish_load(generation, result, cx));
        })
        .detach();
    }

    fn finish_load(
        &mut self,
        generation: u64,
        result: Result<ReaderBook, ReaderError>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        match result {
            Ok(content) => {
                let chapter_count = content.chapters.len().max(1);
                self.chapter = self.chapter.min(chapter_count - 1);
                self.chapter_ratio = self.chapter_ratio.clamp(0.0, 1.0);
                let encoding = content.encoding.clone();
                self.toc_titles = Some(Rc::new(
                    content
                        .chapters
                        .iter()
                        .map(|chapter| chapter.title.clone())
                        .collect(),
                ));
                self.content = Some(content);
                self.load_state = LoadState::Ready;
                self.restore_pending = true;
                let book_id = self.book_id.clone();
                self.model.update(cx, |model, _| {
                    model.mark_book_opened(&book_id, chapter_count as u32, encoding);
                });
                self.record_progress(cx);
                self.spawn_autosave(cx);
            }
            Err(error) => self.load_state = LoadState::Error(error),
        }
        cx.notify();
    }

    /// 每 5 s 把脏掉的进度落盘一次；实体被丢弃时循环自然结束。
    fn spawn_autosave(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(5))
                    .await;
                let alive = this
                    .update(cx, |page, cx| page.autosave(cx))
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn autosave(&mut self, cx: &mut Context<Self>) {
        if !self.progress_dirty {
            return;
        }
        self.progress_dirty = false;
        self.model.update(cx, |model, _| model.flush_library());
    }

    /// 把当前滚动位置换算成章内比例（滚不动时保持原值）。
    fn capture_scroll_ratio(&mut self) {
        let max = f32::from(self.scroll.max_offset().y);
        if max > 1.0 {
            self.chapter_ratio = (f32::from(self.scroll.offset().y) / max).clamp(0.0, 1.0);
        }
    }

    fn record_progress(&mut self, cx: &mut Context<Self>) {
        let chapter_count = self
            .content
            .as_ref()
            .map_or(0, |content| content.chapters.len());
        // 滚动到最后一章的最后 1%（或翻页到最后一页）即视为读完。
        let at_end = chapter_count > 0
            && self.chapter + 1 >= chapter_count
            && self.chapter_ratio >= 0.99;
        self.finished |= at_end;
        let progress = ReadingProgress {
            chapter: self.chapter,
            chapter_ratio: self.chapter_ratio,
            // 百分比与时间戳由 store 统一计算，这里不做第二份实现。
            percent: 0.0,
            mode: self.mode,
            updated_unix: 0,
            finished: self.finished,
        };
        let book_id = self.book_id.clone();
        self.model.update(cx, |model, _| {
            model.record_reading_progress(&book_id, progress);
        });
        self.progress_dirty = true;
    }

    fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        self.capture_scroll_ratio();
        self.mode = match self.mode {
            ReadingMode::Scroll => ReadingMode::Page,
            ReadingMode::Page => ReadingMode::Scroll,
        };
        self.restore_pending = true;
        let book_id = self.book_id.clone();
        let mode = self.mode;
        self.model
            .update(cx, |model, _| model.set_reading_mode(&book_id, mode));
        self.record_progress(cx);
        cx.notify();
    }

    /// 跳到第 `index` 章（0-based），章内位置归零。
    pub(super) fn goto_chapter(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(content) = self.content.as_ref() else {
            return;
        };
        if index >= content.chapters.len() {
            return;
        }
        self.chapter = index;
        self.chapter_ratio = 0.0;
        self.restore_pending = true;
        self.record_progress(cx);
        cx.notify();
    }

    fn step_chapter(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(content) = self.content.as_ref() else {
            return;
        };
        let count = content.chapters.len();
        if count == 0 {
            return;
        }
        let next = self.chapter.saturating_add_signed(delta).min(count - 1);
        if next != self.chapter {
            self.goto_chapter(next, cx);
        }
    }

    /// 按屏幕高度滚动（`direction` 为 -1 / 1）。
    pub(super) fn scroll_by_page(&self, direction: f32) {
        let viewport = f32::from(self.scroll.bounds().size.height);
        self.scroll_relative(direction * (viewport * 0.9).max(1.0));
    }

    /// 小步滚动（方向键）。
    fn scroll_by_step(&self, direction: f32) {
        self.scroll_relative(direction * 40.0);
    }

    /// 滚动到章内某个比例处。
    fn scroll_to_ratio(&self, ratio: f32) {
        let max = f32::from(self.scroll.max_offset().y);
        self.scroll.set_offset(point(px(0.0), px(max * ratio.clamp(0.0, 1.0))));
    }

    /// 在当前位置上偏移；程序化滚动不会触发 `on_scroll`，所以同步排一次进度提交。
    pub(super) fn scroll_relative(&self, delta: f32) {
        let max = f32::from(self.scroll.max_offset().y);
        let target = (f32::from(self.scroll.offset().y) + delta).clamp(0.0, max.max(0.0));
        self.scroll.set_offset(point(px(0.0), px(target)));
    }

    /// 滚动防抖：200 ms 内不再滚动才写进度，避免每一帧都更新 store。
    pub(super) fn schedule_progress_commit(&mut self, cx: &mut Context<Self>) {
        self.scroll_generation = self.scroll_generation.wrapping_add(1);
        let generation = self.scroll_generation;
        let handle = self.scroll.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let _ = this.update(cx, |page, cx| {
                page.commit_scroll_progress(generation, &handle, cx);
            });
        })
        .detach();
    }

    fn commit_scroll_progress(
        &mut self,
        generation: u64,
        handle: &ScrollHandle,
        cx: &mut Context<Self>,
    ) {
        if generation != self.scroll_generation {
            return;
        }
        let max = f32::from(handle.max_offset().y);
        let offset = f32::from(handle.offset().y);
        self.chapter_ratio = if max <= 1.0 {
            0.0
        } else {
            (offset / max).clamp(0.0, 1.0)
        };
        self.record_progress(cx);
        cx.notify();
    }

    fn shortcuts_blocked(&self, window: &Window, cx: &App) -> bool {
        // 目录过滤框聚焦时按键归输入框，只让 Esc 例外（在 on_key_down 里处理）。
        self.toc_filter.read(cx).focus_handle().is_focused(window)
    }

    fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        if self.shortcuts_blocked(window, cx) {
            if key == "escape" {
                self.toc_open = false;
                window.focus(&self.focus, cx);
                cx.notify();
            }
            return;
        }

        match event.keystroke.key_char.as_deref() {
            Some("+") => {
                self.adjust_font_size(1.0, cx);
                return;
            }
            Some("-") => {
                self.adjust_font_size(-1.0, cx);
                return;
            }
            _ => {}
        }

        match key {
            "left" | "pageup" => self.scroll_by_page(-1.0),
            "right" | "pagedown" | "space" => self.scroll_by_page(1.0),
            "up" => self.scroll_by_step(-1.0),
            "down" => self.scroll_by_step(1.0),
            "home" => self.scroll_to_ratio(0.0),
            "end" => self.scroll_to_ratio(1.0),
            "t" => {
                self.toc_open = !self.toc_open;
                self.typography_open = false;
                cx.notify();
            }
            "m" => self.toggle_mode(cx),
            "escape" => {
                if self.typography_open {
                    self.typography_open = false;
                    self.persist_settings(cx);
                    cx.notify();
                } else if self.toc_open {
                    self.toc_open = false;
                    cx.notify();
                } else if let Some(handler) = self.close_handler.clone() {
                    handler(window, cx);
                }
            }
            _ => return,
        }

        // 方向键 / Home / End 只改了滚动位置，进度要在滚动之后补记。
        if matches!(
            key,
            "left" | "right" | "pageup" | "pagedown" | "space" | "up" | "down" | "home" | "end"
        ) {
            self.schedule_progress_commit(cx);
        }
    }

    fn adjust_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.update_reader_config(
            move |reader| reader.font_size += delta,
            cx,
        );
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let close_handler = self.close_handler.clone();
        let mode_label = match self.mode {
            ReadingMode::Scroll => t!("Reader.mode.page"),
            ReadingMode::Page => t!("Reader.mode.scroll"),
        };

        let mut toc_button = Button::new("reader-toc").label(t!("Reader.action.toc"));
        toc_button = if self.toc_open {
            toc_button.primary()
        } else {
            toc_button.ghost()
        };
        let mut typography_button = Button::new("reader-typography").label("Aa");
        typography_button = if self.typography_open {
            typography_button.primary()
        } else {
            typography_button.ghost()
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("reader-back")
                    .label(t!("Reader.action.back"))
                    .on_click(move |_event, window, cx| {
                        if let Some(handler) = &close_handler {
                            handler(window, cx);
                        }
                    }),
            )
            .child(toc_button.on_click(cx.listener(
                |this, _event, _window, cx| {
                    this.toc_open = !this.toc_open;
                    this.typography_open = false;
                    cx.notify();
                },
            )))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_center()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(SharedString::from(self.book.title.clone())),
            )
            .child(typography_button.on_click(cx.listener(
                |this, _event, _window, cx| {
                    this.typography_open = !this.typography_open;
                    this.toc_open = false;
                    if !this.typography_open {
                        this.persist_settings(cx);
                    }
                    cx.notify();
                },
            )))
            .child(
                Button::new("reader-mode")
                    .label(mode_label)
                    .on_click(cx.listener(|this, _event, _window, cx| this.toggle_mode(cx))),
            )
    }

    fn render_error(&self, error: &ReaderError) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .p_8()
            .child(div().text_lg().child(t!(error.message_key()).to_string()))
            .child(div().text_sm().child(error.detail()))
    }

    fn render_chapter_nav(&self, count: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let percent = if count == 0 {
            0.0
        } else {
            ((self.chapter as f32 + self.chapter_ratio.clamp(0.0, 1.0)) / count as f32 * 100.0)
                .clamp(0.0, 100.0)
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("reader-prev-chapter")
                    .label(t!("Reader.action.prev_chapter"))
                    .disabled(self.chapter == 0)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.step_chapter(-1, cx);
                    })),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} · {percent:.0}%",
                        t!(
                            "Reader.chapter_indicator",
                            current = self.chapter + 1,
                            total = count
                        )
                    )),
            )
            .child(
                Button::new("reader-next-chapter")
                    .label(t!("Reader.action.next_chapter"))
                    .disabled(self.chapter + 1 >= count)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.step_chapter(1, cx);
                    })),
            )
    }

    /// 正文区域。Task 19 会在这里加 `ReadingMode::Page` 分支。
    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        self.render_scroll_body(cx)
    }

    fn render_scroll_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(content) = self.content.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let Some(chapter) = content.chapters.get(self.chapter) else {
            return div().flex_1().into_any_element();
        };

        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, &*cx);
        let layout = ReaderLayout::new(
            reader.font_size,
            reader.line_height,
            reader.paragraph_spacing,
            reader.page_width,
            0.0,
            reader.indent,
            reader.font_family.clone(),
        );
        let line_height = layout.line_height_px();
        let font_size = reader.font_size;
        let paragraph_spacing = reader.paragraph_spacing;
        let page_width = reader.page_width;
        let font_family = reader.font_family.clone();
        let count = content.chapters.len();
        let chapter_title = chapter.title.clone();
        let paragraphs: Vec<String> = chapter
            .paragraphs
            .iter()
            .map(|paragraph| layout.display_text(paragraph).into_owned())
            .collect();

        let mut column = div()
            .w_full()
            .max_w(px(page_width))
            .flex()
            .flex_col()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::BOLD)
                    .mb_4()
                    .child(chapter_title),
            )
            .children(paragraphs.into_iter().map(move |text| {
                div()
                    .pb(px(paragraph_spacing))
                    .text_size(px(font_size))
                    .line_height(px(line_height))
                    .child(text)
            }));
        if !font_family.is_empty() {
            column = column.font_family(font_family);
        }

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(
                div()
                    .id("reader-scroll")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .vertical_scrollbar(&self.scroll)
                    .on_scroll(cx.listener(|this, _event: &ScrollEvent, _window, cx| {
                        this.schedule_progress_commit(cx);
                    }))
                    .child(
                        // 居中靠 flex：gpui 没有 auto margin 样式。
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .justify_center()
                            .px_6()
                            .py_6()
                            .child(column),
                    ),
            )
            .child(self.render_chapter_nav(count, cx))
            .into_any_element()
    }
}

impl Render for ReaderPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, &*cx);

        // 首帧布局算完才知道 max_offset，所以恢复位置一定发生在下一帧。
        if self.restore_pending && matches!(self.load_state, LoadState::Ready) {
            self.restore_pending = false;
            let handle = self.scroll.clone();
            let ratio = self.chapter_ratio.clamp(0.0, 1.0);
            cx.on_next_frame(window, move |_page, _window, _cx| {
                let max = f32::from(handle.max_offset().y);
                handle.set_offset(point(px(0.0), px(max * ratio)));
            });
        }

        let body: AnyElement = match &self.load_state {
            LoadState::Loading => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(t!("Reader.loading").to_string())
                .into_any_element(),
            LoadState::Error(error) => self.render_error(error).into_any_element(),
            LoadState::Ready => self.render_body(cx),
        };
        let toc = if self.toc_open {
            Some(self.render_toc(cx))
        } else {
            None
        };
        let typography = if self.typography_open {
            Some(self.render_typography(cx))
        } else {
            None
        };

        div()
            .relative()
            .track_focus(&self.focus)
            .key_context("Reader")
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .when_some(toc, |this, toc| this.child(toc))
                    .child(body),
            )
            .when_some(typography, |this, panel| this.child(panel))
    }
}
```

- [ ] **Step 5: 编译并跑测试**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿。若 `clippy::cast_precision_loss` 在 `chapter as f32` 上报错，
改成 `f64::from(u32::try_from(self.chapter).unwrap_or(u32::MAX)) as f32` 之类反而更丑 ——
本仓库其它地方已经在用 `as f32` 换算进度（见 `LibraryStore::record_progress`），
保持一致的写法即可；只有新引入的 warning 才需要处理。

- [ ] **Step 6: 手工验收（GUI）**

Run: `cargo run`

1. 书架点开一本 TXT：正文按 `[reader]` 默认排版显示，滚轮滚动，右侧出现滚动条。
2. 滚到中间 → 关闭阅读器 → 再打开：位置停在原处（同一章、相近比例）。
3. 点「目录」：左侧抽屉列出章节，当前章加粗高亮；点别的章 → 正文跳到该章章首。
4. 目录顶部输入片段：列表按标题过滤；按 `Esc` 只关抽屉并还焦点给正文。
5. 点 `Aa`：面板浮在右上角；点主题 → 背景 / 文字色立刻切换；点 `+`/`−` 改字号，
   正文即时重排且仍停在原来的相对位置。
6. 关掉 `Aa` → `~/.sonovel/config.toml` 的 `[reader]` 写入新值（`cargo run` 后再开仍是新排版）。
7. 键盘：`T` 开关目录、`M` 切换模式、`+`/`-` 改字号、`Space`/`PageDown` 下滚一屏、
   `Esc` 退出阅读器。
8. 滚到全书最后 1% → 关阅读器 → 书架卡片显示「已读完」。
9. 停留 10 s 不操作：`~/.sonovel/library.json` 的 `progress` 有更新（5 s 自动落盘）。

- [ ] **Step 7: 提交**

```bash
git add src/desktop/pages/reader
git commit -m "feat(reader): 滚动模式、目录抽屉与排版面板"
```

---

---

### Task 19: 翻页模式（整章连续排版 + 视口窗口）

**Files:**
- Create: `src/desktop/pages/reader/content_view.rs`
- Modify: `src/desktop/pages/reader/mod.rs`（`mod content_view;`）
- Modify: `src/desktop/pages/reader/reader_page.rs`

**Interfaces:**
- Consumes: `reader::layout::{ParagraphMetrics, ReaderLayout, paginate, page_for_ratio, ratio_for_page, content_height}`、`LayoutFingerprint`、`Window::{text_system, text_style, viewport_size, on_next_frame}`。
- Produces:
  - `content_view::{COLUMN_PADDING, chapter_column, measure_lines, measure_style, viewport_probe}`
  - `ReaderPage::{ensure_pagination, render_page_body, turn_page, goto_page, set_viewport_height}`
  - 结构体字段 `page_index` / `page_cache` / `viewport_height`（Task 20 的搜索跳转要用 `goto_chapter` 之外的 `goto_page`）

**平台事实（先读再写）：**

- `window.text_system().shape_text(text, px(font_size), &[run], Some(px(width)), None)`
  返回每源行一个 `WrappedLine`；`WrappedLine` `Deref` 到 `WrappedLineLayout`，
  `wrap_boundaries.len() + 1` 就是该段显示的折行数。
- `TextRun` 用 `TextStyle::to_run(len)` 造：`window.text_style()` 拿到当前样式，
  再改 `font_size`（`.into()` 转 `AbsoluteLength`）与 `font_family`。
- 容器真实高度只能靠 `canvas` 的 prepaint 拿 `bounds`；prepaint 里不能改 view 状态，
  要把变化用 `window.on_next_frame(...)` 排到下一帧。窗口级 `App::defer` 也是安全的
  「渲染后再动手」通道（`Context` deref 到 `App`，`cx.defer(|cx| ...)`）。
- 分页只在指纹变化时重算；正文列宽 = `min(page_width, 窗口宽 - 2*24)`，与渲染时的
  `px(24)` 内边距一致，否则测量和实际折行对不上。

- [ ] **Step 1: 正文测量与列渲染**

新建 `src/desktop/pages/reader/content_view.rs`：

```rust
//! 正文测量与列渲染：滚动模式与翻页模式共用同一份列布局。
//!
//! 测量只依赖 `shape_text` 的折行数，分页纯函数在 `crate::reader::layout` 里，
//! 两边共用「行数」这一约定，所以测量对不上时能用纯函数测出来。

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::{
    Div, FontWeight, IntoElement, ParentElement, SharedString, Styled, TextStyle, WeakEntity,
    Window, canvas, div, px,
};

use super::reader_page::ReaderPage;
use crate::config::ReaderConfig;

/// 正文列左右留白（每侧 px）。测量与渲染必须用同一个值。
pub(super) const COLUMN_PADDING: f32 = 24.0;

/// 把一章渲染成一整列（含章标题）。翻页模式的外层再套一个裁剪视口。
pub(super) fn chapter_column(
    title: &str,
    paragraphs: &[String],
    reader: &ReaderConfig,
    line_height_px: f32,
) -> Div {
    let font_size = reader.font_size;
    let spacing = reader.paragraph_spacing;
    let mut column = div()
        .w_full()
        .max_w(px(reader.page_width))
        .flex()
        .flex_col()
        .child(
            div()
                .pb(px(line_height_px * 0.75))
                .text_size(px(font_size))
                .line_height(px(line_height_px))
                .font_weight(FontWeight::BOLD)
                .child(SharedString::from(title.to_string())),
        )
        .children(paragraphs.iter().cloned().map(move |text| {
            div()
                .pb(px(spacing))
                .text_size(px(font_size))
                .line_height(px(line_height_px))
                .child(text)
        }));
    if !reader.font_family.is_empty() {
        column = column.font_family(reader.font_family.clone());
    }
    column
}

/// 测量时用的文本样式：字号跟随排版设置，字体族配了就用配的。
pub(super) fn measure_style(window: &Window, reader: &ReaderConfig) -> TextStyle {
    let mut style = window.text_style();
    style.font_size = px(reader.font_size).into();
    if !reader.font_family.is_empty() {
        style.font_family = SharedString::from(reader.font_family.clone());
    }
    style
}

/// 一段文字在 `wrap_width` 下折成几行。`shape_text` 出错时按字符宽度估算。
pub(super) fn measure_lines(
    window: &mut Window,
    text: &str,
    font_size: f32,
    wrap_width: f32,
    style: &TextStyle,
) -> usize {
    if text.is_empty() {
        return 1;
    }
    let run = style.to_run(text.len());
    let shaped = window.text_system().shape_text(
        SharedString::from(text.to_string()),
        px(font_size),
        std::slice::from_ref(&run),
        Some(px(wrap_width)),
        None,
    );
    match shaped {
        Ok(lines) if !lines.is_empty() => lines
            .iter()
            .map(|line| line.wrap_boundaries.len() + 1)
            .sum::<usize>()
            .max(1),
        _ => ((text.chars().count() as f32 * font_size / wrap_width.max(1.0)).ceil() as usize).max(1),
    }
}

/// 把正文视口的实测高度回写到页面。
///
/// `canvas` 的 prepaint 拿得到真实 `bounds` 但没有 view `Context`，所以先把值丢给
/// `window.on_next_frame`，由下一帧再更新 view 并触发一次重排。
pub(super) fn viewport_probe(
    page: WeakEntity<ReaderPage>,
    measured: Rc<Cell<Option<f32>>>,
) -> impl IntoElement {
    canvas(
        move |bounds, window, _cx| {
            let height = f32::from(bounds.size.height);
            if height <= 0.0 {
                return;
            }
            let changed = measured
                .get()
                .is_none_or(|previous| (previous - height).abs() > 0.5);
            if !changed {
                return;
            }
            measured.set(Some(height));
            let page = page.clone();
            window.on_next_frame(move |_window, cx| {
                let _ = page.update(cx, |page, cx| page.set_viewport_height(height, cx));
            });
        },
        |_bounds, (), _window, _cx| {},
    )
}
```

- [ ] **Step 2: 注册子模块**

`src/desktop/pages/reader/mod.rs` 顶部改成：

```rust
//! 应用内阅读器：覆盖式视图，不作为第 7 个导航页。

mod content_view;
mod toc;
mod typography;

pub mod reader_page;
pub mod theme;

pub use reader_page::{CloseReaderFn, LoadState, ReaderPage};
pub use theme::reader_colors;
```

- [ ] **Step 3: `reader_page.rs` 的 `use` 与结构体**

把 Task 18 的 `use` 段整体替换为：

```rust
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Entity, FocusHandle, FontWeight, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement, Render, ScrollEvent, ScrollHandle, SharedString,
    Styled, Window, div, point, px,
};
use rust_i18n::t;

use super::content_view::{self, COLUMN_PADDING};
use super::theme::reader_colors;
use crate::config::{ReaderConfig, ReadingMode};
use crate::desktop::model::AppModel;
use crate::library::{Book, ReadingProgress};
use crate::reader::layout::{page_for_ratio, ratio_for_page};
use crate::reader::{
    LayoutFingerprint, ParagraphMetrics, ReaderBook, ReaderError, ReaderLayout, load_book, paginate,
};
```

结构体与缓存结构整体替换为：

```rust
/// 阅读器页面。
pub struct ReaderPage {
    model: Entity<AppModel>,
    book_id: String,
    /// 打开时的书籍快照；关闭时用于落盘。
    pub(super) book: Book,
    pub(super) content: Option<ReaderBook>,
    pub(super) load_state: LoadState,
    pub(super) mode: ReadingMode,
    pub(super) chapter: usize,
    pub(super) chapter_ratio: f32,
    /// 已到达书末并置位；一旦为 true 不再回落。
    finished: bool,
    /// 滚动模式的滚动位置。
    pub(super) scroll: ScrollHandle,
    /// 待恢复的位置：`max_offset` 要等布局完成，所以恢复放到下一帧。
    restore_pending: bool,
    /// 每次滚动自增，只有最新一代的防抖任务会写进度。
    scroll_generation: u64,
    /// 翻页模式：当前页号。
    page_index: usize,
    /// 翻页模式：当前章的分页缓存。
    page_cache: Option<PageCache>,
    /// 越界向前翻页时置位：重算分页后落到新章最后一页。
    page_turn_to_last: bool,
    /// 正文视口实测高度（px）；`None` = 还没量到，先按窗口高度估。
    viewport_height: Option<f32>,
    /// 与 `viewport_probe` 共享的最近一次实测值，用来判断要不要通知。
    viewport_measured: Rc<Cell<Option<f32>>>,
    /// 测量异常提示过就不再刷屏。
    layout_warned: bool,
    /// 章节标题快照；加载完成时建一次，抽屉每帧零拷贝共享。
    pub(super) toc_titles: Option<Rc<Vec<String>>>,
    /// 目录顶部的章节过滤框。
    pub(super) toc_filter: Entity<InputState>,
    pub(super) toc_open: bool,
    pub(super) typography_open: bool,
    immersive: bool,
    generation: u64,
    progress_dirty: bool,
    focus: FocusHandle,
    close_handler: Option<CloseReaderFn>,
}

/// 一章的分页缓存：指纹变了才整章重算。
///
/// `offset` 是每页的起始纵向偏移（px，首项恒为 0），由 `paginate` 产出。
struct PageCache {
    fingerprint: LayoutFingerprint,
    offset: Vec<f32>,
}
```

`new()` 里结构体字面量在 `scroll_generation: 0,` 之后追加：

```rust
            page_index: 0,
            page_cache: None,
            page_turn_to_last: false,
            viewport_height: None,
            viewport_measured: Rc::new(Cell::new(None)),
            layout_warned: false,
```

- [ ] **Step 4: 分页与翻页方法**

在 `impl ReaderPage` 里、`fn render_body` 之前插入：

```rust
    /// `viewport_probe` 量到正文容器高度后回调进来；变化超过半像素才重排。
    pub(super) fn set_viewport_height(&mut self, height: f32, cx: &mut Context<Self>) {
        if self
            .viewport_height
            .is_some_and(|previous| (previous - height).abs() <= 0.5)
        {
            return;
        }
        self.viewport_height = Some(height);
        cx.notify();
    }

    /// 翻页模式：按当前指纹测一遍本章并分页；指纹没变就直接用缓存。
    fn ensure_pagination(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reader = self.reader_config(cx);
        let viewport_width = f32::from(window.viewport_size().width);
        let content_width = (viewport_width - 2.0 * COLUMN_PADDING)
            .min(reader.page_width)
            .max(160.0);
        let viewport_height = self
            .viewport_height
            .unwrap_or_else(|| (f32::from(window.viewport_size().height) - 160.0).max(120.0));
        let layout = ReaderLayout::new(
            reader.font_size,
            reader.line_height,
            reader.paragraph_spacing,
            content_width,
            viewport_height,
            reader.indent,
            reader.font_family.clone(),
        );
        let fingerprint = layout.fingerprint(self.chapter);
        if self
            .page_cache
            .as_ref()
            .is_some_and(|cache| cache.fingerprint == fingerprint)
        {
            return;
        }

        let line_height_px = layout.line_height_px();
        let Some(chapter) = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(self.chapter))
        else {
            return;
        };
        // 章标题也算一段（同字号、加粗），否则分页偏移和实际内容高度对不上。
        let texts: Vec<String> = std::iter::once(chapter.title.clone())
            .chain(
                chapter
                    .paragraphs
                    .iter()
                    .map(|paragraph| layout.display_text(paragraph).into_owned()),
            )
            .collect();
        let style = content_view::measure_style(window, &reader);
        let spacing = reader.paragraph_spacing;
        let metrics: Vec<ParagraphMetrics> = texts
            .iter()
            .enumerate()
            .map(|(index, text)| ParagraphMetrics {
                lines: content_view::measure_lines(
                    window,
                    text,
                    reader.font_size,
                    content_width,
                    &style,
                ),
                spacing_after: if index == 0 {
                    line_height_px * 0.75
                } else if index + 1 == texts.len() {
                    0.0
                } else {
                    spacing
                },
            })
            .collect();
        let page_offsets = paginate(&metrics, line_height_px, viewport_height);
        let content_height =
            crate::reader::layout::content_height(&metrics, line_height_px);
        let broken = page_offsets.is_empty()
            || page_offsets.last().copied().unwrap_or(0.0) > content_height;
        if broken {
            // 测量和分页对不上：退回滚动模式，只提示一次（在渲染后再动 model）。
            self.mode = ReadingMode::Scroll;
            self.page_cache = None;
            if !self.layout_warned {
                self.layout_warned = true;
                let model = self.model.clone();
                cx.defer(move |cx| {
                    model.update(cx, |model, _cx| {
                        model.push_warning(t!("Reader.error.layout_fallback").to_string());
                    });
                });
            }
            return;
        }

        let pages = page_offsets.len();
        self.page_index = if self.page_turn_to_last {
            self.page_turn_to_last = false;
            pages.saturating_sub(1)
        } else {
            page_for_ratio(self.chapter_ratio, pages)
        };
        self.chapter_ratio = ratio_for_page(self.page_index, pages);
        self.page_cache = Some(PageCache {
            fingerprint,
            offset: page_offsets,
        });
    }

    fn goto_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let pages = self
            .page_cache
            .as_ref()
            .map_or(0, |cache| cache.offset.len());
        if pages == 0 {
            return;
        }
        self.page_index = page.min(pages - 1);
        self.chapter_ratio = ratio_for_page(self.page_index, pages);
        self.record_progress(cx);
        cx.notify();
    }

    fn turn_page(&mut self, delta: isize, cx: &mut Context<Self>) {
        let pages = self
            .page_cache
            .as_ref()
            .map_or(0, |cache| cache.offset.len());
        if pages == 0 {
            return;
        }
        let next = self.page_index as isize + delta;
        if next < 0 {
            if self.chapter == 0 {
                return;
            }
            self.page_turn_to_last = true;
            let target = self.chapter - 1;
            self.goto_chapter(target, cx);
            return;
        }
        if next as usize >= pages {
            let count = self
                .content
                .as_ref()
                .map_or(0, |content| content.chapters.len());
            if self.chapter + 1 >= count {
                return;
            }
            let target = self.chapter + 1;
            self.goto_chapter(target, cx);
            return;
        }
        self.goto_page(next as usize, cx);
    }
```

- [ ] **Step 5: 渲染翻页正文**

把 Task 18 的 `fn render_body` 整体替换为下面这段（`render_scroll_body` 也一并替换，改成复用 `chapter_column`）：

```rust
    /// 正文区域：翻页模式用裁剪视口 + 纵向位移，滚动模式用滚动容器。
    fn render_body(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.mode {
            ReadingMode::Page => self.render_page_body(cx),
            ReadingMode::Scroll => self.render_scroll_body(cx),
        }
    }

    fn render_page_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some((start, pages)) = self.page_cache.as_ref().map(|cache| {
            let start = cache.offset.get(self.page_index).copied().unwrap_or(0.0);
            (start, cache.offset.len())
        }) else {
            // 还没分好页（首帧）：先按滚动模式画，下一帧缓存在了再切过来。
            return self.render_scroll_body(cx);
        };
        let Some(content) = self.content.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let Some(chapter) = content.chapters.get(self.chapter) else {
            return div().flex_1().into_any_element();
        };
        let count = content.chapters.len();
        let title = chapter.title.clone();
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, &*cx);
        let layout = ReaderLayout::new(
            reader.font_size,
            reader.line_height,
            reader.paragraph_spacing,
            reader.page_width,
            0.0,
            reader.indent,
            reader.font_family.clone(),
        );
        let line_height = layout.line_height_px();
        let paragraphs: Vec<String> = chapter
            .paragraphs
            .iter()
            .map(|paragraph| layout.display_text(paragraph).into_owned())
            .collect();
        let probe = content_view::viewport_probe(
            cx.entity().downgrade(),
            self.viewport_measured.clone(),
        );

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .px(px(COLUMN_PADDING))
                    .py(px(COLUMN_PADDING))
                    .child(probe)
                    .child(
                        div()
                            .relative()
                            .top(px(-start))
                            .w_full()
                            .flex()
                            .flex_row()
                            .justify_center()
                            .child(content_view::chapter_column(
                                &title,
                                &paragraphs,
                                &reader,
                                line_height,
                            )),
                    ),
            )
            .child(self.render_page_nav(pages, count, cx))
            .into_any_element()
    }

    fn render_scroll_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(content) = self.content.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let Some(chapter) = content.chapters.get(self.chapter) else {
            return div().flex_1().into_any_element();
        };
        let count = content.chapters.len();
        let title = chapter.title.clone();
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, &*cx);
        let layout = ReaderLayout::new(
            reader.font_size,
            reader.line_height,
            reader.paragraph_spacing,
            reader.page_width,
            0.0,
            reader.indent,
            reader.font_family.clone(),
        );
        let line_height = layout.line_height_px();
        let paragraphs: Vec<String> = chapter
            .paragraphs
            .iter()
            .map(|paragraph| layout.display_text(paragraph).into_owned())
            .collect();
        let column = content_view::chapter_column(&title, &paragraphs, &reader, line_height);

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(
                div()
                    .id("reader-scroll")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .vertical_scrollbar(&self.scroll)
                    .on_scroll(cx.listener(|this, _event: &ScrollEvent, _window, cx| {
                        this.schedule_progress_commit(cx);
                    }))
                    .child(
                        // 居中靠 flex：gpui 没有 auto margin 样式。
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .justify_center()
                            .px(px(COLUMN_PADDING))
                            .py(px(COLUMN_PADDING))
                            .child(column),
                    ),
            )
            .child(self.render_chapter_nav(count, cx))
            .into_any_element()
    }

    fn render_page_nav(&self, pages: usize, count: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let percent = if count == 0 {
            0.0
        } else {
            ((self.chapter as f32 + self.chapter_ratio.clamp(0.0, 1.0)) / count as f32 * 100.0)
                .clamp(0.0, 100.0)
        };
        let at_end = self.chapter + 1 >= count && self.page_index + 1 >= pages;
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("reader-prev-page")
                    .label(t!("Reader.action.prev_page"))
                    .disabled(self.chapter == 0 && self.page_index == 0)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.turn_page(-1, cx);
                    })),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} · {percent:.0}%",
                        t!(
                            "Reader.page_indicator",
                            page = self.page_index + 1,
                            pages = pages,
                            chapter = self.chapter + 1,
                            total = count
                        )
                    )),
            )
            .child(
                Button::new("reader-next-page")
                    .label(t!("Reader.action.next_page"))
                    .disabled(at_end)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.turn_page(1, cx);
                    })),
            )
    }
```

- [ ] **Step 6: `Render` 里触发分页**

`impl Render for ReaderPage` 的 `render()`，在「恢复位置」的 `if self.restore_pending ...` 之后、`let body` 之前插入：

```rust
        // 翻页模式：指纹变了才重算分页；分页结果供下面的正文与进度使用。
        if matches!(self.mode, ReadingMode::Page) && matches!(self.load_state, LoadState::Ready) {
            self.ensure_pagination(window, cx);
        }
```

- [ ] **Step 7: 按键在两种模式下分流**

`fn on_key_down` 的第二个 `match key { ... }`（`"left" | "pageup" => ...` 那个）整体替换为：

```rust
        let paging = matches!(self.mode, ReadingMode::Page);
        match key {
            "left" | "pageup" | "up" if paging => self.turn_page(-1, cx),
            "right" | "pagedown" | "space" | "down" if paging => self.turn_page(1, cx),
            "home" if paging => self.goto_page(0, cx),
            "end" if paging => {
                let pages = self
                    .page_cache
                    .as_ref()
                    .map_or(0, |cache| cache.offset.len());
                self.goto_page(pages.saturating_sub(1), cx);
            }
            "left" | "pageup" => self.scroll_by_page(-1.0),
            "right" | "pagedown" | "space" => self.scroll_by_page(1.0),
            "up" => self.scroll_by_step(-1.0),
            "down" => self.scroll_by_step(1.0),
            "home" => self.scroll_to_ratio(0.0),
            "end" => self.scroll_to_ratio(1.0),
            "t" => {
                self.toc_open = !self.toc_open;
                self.typography_open = false;
                cx.notify();
            }
            "m" => self.toggle_mode(cx),
            "escape" => {
                if self.typography_open {
                    self.typography_open = false;
                    self.persist_settings(cx);
                    cx.notify();
                } else if self.toc_open {
                    self.toc_open = false;
                    cx.notify();
                } else if let Some(handler) = self.close_handler.clone() {
                    handler(window, cx);
                }
            }
            _ => return,
        }

        // 滚动模式的按键只改了滚动位置，进度要在滚动之后补记；
        // 翻页模式的 turn_page / goto_page 已经自己记过进度了。
        if !paging
            && matches!(
                key,
                "left" | "right" | "pageup" | "pagedown" | "space" | "up" | "down" | "home" | "end"
            )
        {
            self.schedule_progress_commit(cx);
        }
```

- [ ] **Step 8: 编译并跑测试**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿。

- [ ] **Step 9: 手工验收（GUI）**

Run: `cargo run`

1. 打开一本书 → 顶栏点「翻页」（或按 `M`）：正文只显示当前页，页边界不切半行。
2. `→` / `Space` / `PageDown` 下一页，`←` / `PageUp` 上一页；页脚「第 x / y 页 · 第 c / n 章」随之变化。
3. 本章最后一页再往后 → 进入下一章第 1 页；上一页越过章首 → 回到上一章最后一页。
4. `Home` / `End` 到本章首页 / 末页。
5. 改字号（`Aa` 面板或 `+`/`-`）→ 重新分页，且仍停在原来的相对位置附近。
6. 拖动窗口改变大小 / 最大化 → 分页随视口高度重算，不出现半页空白截断。
7. 翻页模式关闭再打开同一本书 → 回到上次那一页（`chapter_ratio` 保留）。
8. 切回滚动模式 → 滚动位置对应刚才的阅读比例。

- [ ] **Step 10: 提交**

```bash
git add src/desktop/pages/reader
git commit -m "feat(reader): 整章连续排版的翻页模式"
```

---

---

### Task 20: 书内搜索面板与命中高亮

**Files:**
- Create: `src/desktop/pages/reader/search_panel.rs`
- Modify: `src/desktop/pages/reader/mod.rs`（`mod search_panel;`）
- Modify: `src/desktop/pages/reader/content_view.rs`（`chapter_column` 支持高亮）
- Modify: `src/desktop/pages/reader/reader_page.rs`

**Interfaces:**
- Consumes: `reader::{SearchGroup, SearchHit, HITS_TOTAL}`、`reader::search::search_chapter`、`HighlightStyle` / `StyledText`。
- Produces:
  - `search_panel::SearchPanel`
  - `ReaderPage::{start_search, goto_hit, chapter_highlights, render_search}`

**平台事实：**

- 搜索在**原始文本**上按字节匹配，偏移是段落内字节偏移；首行缩进是渲染时加的
  6 字节前缀（`ReaderLayout::INDENT`），高亮区间要整体加这个偏移，否则标错位置。
- 段落高亮用 `StyledText::new(text).with_highlights([(range, HighlightStyle { background_color: Some(color), ..Default::default() })])`：
  它按 `TextRun` 分段，折行仍然由排版引擎处理；不要自己拼 `span` —— 行内分段会丢掉跨段折行。
- 整书扫描放在 `cx.spawn` 里**逐章**做，每章间 `timer(8ms)` 让出 UI 线程；
  代次 `generation` + `AtomicBool` 双保险，新搜索 / 关面板都能让旧任务立刻退出。

- [ ] **Step 1: 搜索状态与扫描**

新建 `src/desktop/pages/reader/search_panel.rs`：

```rust
//! 书内搜索抽屉：逐章扫描、结果分组、点击跳转与命中高亮。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, FontWeight, HighlightStyle, IntoElement, ParentElement, Styled,
    StyledText, div, px,
};
use rust_i18n::t;

use super::reader_page::ReaderPage;
use super::theme::reader_colors;
use crate::config::ReaderConfig;
use crate::reader::search::search_chapter;
use crate::reader::{HITS_TOTAL, ReaderLayout, SearchHit, SearchGroup};

/// 搜索抽屉的全部状态。
pub(super) struct SearchPanel {
    /// 当前生效的关键词（已 trim）。
    query: String,
    groups: Vec<SearchGroup>,
    scanning: bool,
    /// 已扫描章数。
    scanned: usize,
    /// 总章数。
    total: usize,
    /// 累计命中数。
    hits: usize,
    /// 每次新搜索自增；过期任务据此退出。
    generation: u64,
    /// 当前定位的命中 `(章, 段, 字节偏移)`。
    active: Option<(usize, usize, usize)>,
}

impl SearchPanel {
    pub(super) fn new() -> Self {
        Self {
            query: String::new(),
            groups: Vec::new(),
            scanning: false,
            scanned: 0,
            total: 0,
            hits: 0,
            generation: 0,
            active: None,
        }
    }
}

impl ReaderPage {
    /// 开始一次新搜索：清旧结果、起后台逐章扫描。
    pub(super) fn start_search(&mut self, cx: &mut Context<Self>) {
        self.cancel_search();
        let needle = self.search_input.read(cx).value().trim().to_string();
        self.search.query = needle.clone();
        self.search.groups.clear();
        self.search.scanned = 0;
        self.search.hits = 0;
        self.search.active = None;
        self.search.total = self
            .content
            .as_ref()
            .map_or(0, |content| content.chapters.len());
        if needle.is_empty() || self.search.total == 0 {
            self.search.scanning = false;
            cx.notify();
            return;
        }

        self.search.generation = self.search.generation.wrapping_add(1);
        let generation = self.search.generation;
        self.search.scanning = true;
        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = Some(cancel.clone());
        cx.spawn(async move |this, cx| {
            loop {
                let more = this
                    .update(cx, |page, cx| {
                        page.search_step(generation, &needle, &cancel, cx)
                    })
                    .unwrap_or(false);
                if !more {
                    break;
                }
                // 每章让出一次调度：整书扫描不会把输入框和滚动卡死。
                cx.background_executor()
                    .timer(Duration::from_millis(8))
                    .await;
            }
        })
        .detach();
        cx.notify();
    }

    /// 停掉在跑的扫描任务。
    pub(super) fn cancel_search(&mut self) {
        if let Some(cancel) = self.search_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.search.scanning = false;
    }

    /// 扫一章；返回是否还有下一章要扫。
    fn search_step(
        &mut self,
        generation: u64,
        needle: &str,
        cancel: &AtomicBool,
        cx: &mut Context<Self>,
    ) -> bool {
        if generation != self.search.generation || cancel.load(Ordering::Relaxed) {
            return false;
        }
        let index = self.search.scanned;
        let Some(chapter) = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(index))
        else {
            self.search.scanning = false;
            self.search_cancel = None;
            cx.notify();
            return false;
        };
        let budget = HITS_TOTAL.saturating_sub(self.search.hits);
        let group = if budget == 0 {
            None
        } else {
            search_chapter(index, &chapter.title, &chapter.paragraphs, needle, budget)
        };
        if let Some(group) = group {
            self.search.hits += group.hits.len();
            self.search.groups.push(group);
        }
        self.search.scanned += 1;
        self.search.scanning =
            self.search.scanned < self.search.total && self.search.hits < HITS_TOTAL;
        let more = self.search.scanning;
        if !more {
            self.search_cancel = None;
        }
        cx.notify();
        more
    }

    /// 跳到某条命中：章按段落比例定位，并把它设为「当前命中」。
    pub(super) fn goto_hit(
        &mut self,
        chapter: usize,
        paragraph: usize,
        byte_offset: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(chapter))
        else {
            return;
        };
        let paragraphs = target.paragraphs.len();
        let ratio = if paragraphs == 0 {
            0.0
        } else {
            (paragraph as f32 / paragraphs as f32).clamp(0.0, 1.0)
        };
        self.chapter = chapter;
        self.chapter_ratio = ratio;
        self.restore_pending = true;
        self.search.active = Some((chapter, paragraph, byte_offset));
        self.record_progress(cx);
        cx.notify();
    }

    /// 当前章的高亮区间（按段落索引）；没有命中的段落是空表。
    pub(super) fn chapter_highlights(
        &self,
        reader: &ReaderConfig,
        cx: &App,
    ) -> Option<Vec<Vec<(std::ops::Range<usize>, HighlightStyle)>>> {
        let group = self
            .search
            .groups
            .iter()
            .find(|group| group.chapter == self.chapter)?;
        let count = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(self.chapter))
            .map_or(0, |chapter| chapter.paragraphs.len());
        if count == 0 {
            return None;
        }
        let colors = reader_colors(reader.theme, cx);
        // 渲染时段落前面会加两个全角空格（6 字节），搜索结果没有这段前缀。
        let indent = if reader.indent {
            ReaderLayout::INDENT.len()
        } else {
            0
        };
        let mut rows: Vec<Vec<(std::ops::Range<usize>, HighlightStyle)>> = vec![Vec::new(); count];
        for hit in &group.hits {
            let active = self.search.active == Some((self.chapter, hit.paragraph, hit.byte_offset));
            let style = HighlightStyle {
                background_color: Some(if active {
                    colors.active_highlight
                } else {
                    colors.highlight
                }),
                ..HighlightStyle::default()
            };
            let start = hit.byte_offset + indent;
            if let Some(row) = rows.get_mut(hit.paragraph) {
                row.push((start..start + hit.byte_len, style));
            }
        }
        Some(rows)
    }

    /// 右侧搜索抽屉。
    pub(super) fn render_search(&self, cx: &mut Context<Self>) -> AnyElement {
        let status = if self.search.query.is_empty() {
            t!("Reader.search.hint").to_string()
        } else if self.search.scanning {
            t!(
                "Reader.search.scanning",
                done = &self.search.scanned.to_string(),
                total = &self.search.total.to_string()
            )
            .to_string()
        } else {
            t!(
                "Reader.search.results",
                count = &self.search.hits.to_string()
            )
            .to_string()
        };

        let mut results = div()
            .id("reader-search-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        for group in &self.search.groups {
            results = results.child(
                div()
                    .px_2()
                    .pt_2()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!("{} · {}", group.title, group.hits.len())),
            );
            for hit in &group.hits {
                results = results.child(self.render_search_hit(group.chapter, hit, cx));
            }
        }

        div()
            .w(px(320.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Reader.search.title").to_string()),
                            )
                            .child(
                                Button::new("reader-search-close")
                                    .ghost()
                                    .compact()
                                    .label(t!("Reader.action.close"))
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.search_open = false;
                                        this.cancel_search();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(Input::new(&self.search_input).cleanable(true))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(status),
                    ),
            )
            .child(results)
            .into_any_element()
    }

    fn render_search_hit(
        &self,
        chapter: usize,
        hit: &SearchHit,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let paragraph = hit.paragraph;
        let byte_offset = hit.byte_offset;
        let active = self.search.active == Some((chapter, paragraph, byte_offset));
        let colors = reader_colors(self.reader_config(cx).theme, &*cx);
        let highlight = if active {
            colors.active_highlight
        } else {
            colors.highlight
        };
        let hover_bg = cx.theme().list_hover;
        let active_bg = cx.theme().selection;
        let snippet = StyledText::new(hit.snippet.clone()).with_highlights([(
            hit.snippet_match.clone(),
            HighlightStyle {
                background_color: Some(highlight),
                ..HighlightStyle::default()
            },
        )]);

        div()
            .id(format!("reader-search-hit-{chapter}-{paragraph}-{byte_offset}"))
            .px_2()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_sm()
            .when(active, |this| this.bg(active_bg))
            .hover(move |style| style.bg(hover_bg))
            .child(snippet)
            .on_click(cx.listener(
                move |this, _event, _window, cx| {
                    this.goto_hit(chapter, paragraph, byte_offset, cx);
                },
            ))
    }
}
```

- [ ] **Step 2: 段落高亮接进列渲染**

`src/desktop/pages/reader/content_view.rs` 的 `use` 段加上 `HighlightStyle`、`StyledText`：

```rust
use gpui_kit::{
    Div, FontWeight, HighlightStyle, IntoElement, ParentElement, SharedString, Styled, StyledText,
    TextStyle, WeakEntity, Window, canvas, div, px,
};
```

`chapter_column` 整体替换为（多一个 `highlights` 参数）：

```rust
/// 把一章渲染成一整列（含章标题）。`highlights` 按段落索引给出高亮区间。
pub(super) fn chapter_column(
    title: &str,
    paragraphs: &[String],
    reader: &ReaderConfig,
    line_height_px: f32,
    highlights: Option<&[Vec<(std::ops::Range<usize>, HighlightStyle)>]>,
) -> Div {
    let font_size = reader.font_size;
    let spacing = reader.paragraph_spacing;
    // 闭包要 'static，所以高亮区间得拷贝成 owned；
    // 只复制几千个 (Range, HighlightStyle) 对，代价比拆成 span 小得多。
    let highlight_rows = highlights.map(|rows| rows.to_vec());
    let mut column = div()
        .w_full()
        .max_w(px(reader.page_width))
        .flex()
        .flex_col()
        .child(
            div()
                .pb(px(line_height_px * 0.75))
                .text_size(px(font_size))
                .line_height(px(line_height_px))
                .font_weight(FontWeight::BOLD)
                .child(SharedString::from(title.to_string())),
        )
        .children(paragraphs.iter().cloned().enumerate().map(move |(index, text)| {
            let mut row = div()
                .pb(px(spacing))
                .text_size(px(font_size))
                .line_height(px(line_height_px));
            row = match highlight_rows
                .as_ref()
                .and_then(|rows| rows.get(index))
                .filter(|ranges| !ranges.is_empty())
            {
                Some(ranges) => row.child(
                    StyledText::new(text).with_highlights(ranges.clone()),
                ),
                None => row.child(text),
            };
            row
        }));
    if !reader.font_family.is_empty() {
        column = column.font_family(reader.font_family.clone());
    }
    column
}
```

`reader_page.rs` 里两处 `content_view::chapter_column(...)` 调用各加一个参数：

```rust
                            .child(content_view::chapter_column(
                                &title,
                                &paragraphs,
                                &reader,
                                line_height,
                                highlights.as_deref(),
                            )),
```

滚动模式那处：

```rust
        let highlights = self.chapter_highlights(&reader, &*cx);
        let column = content_view::chapter_column(
            &title,
            &paragraphs,
            &reader,
            line_height,
            highlights.as_deref(),
        );
```

（翻页模式在 `let paragraphs = ...;` 之后同样加一行
`let highlights = self.chapter_highlights(&reader, &*cx);`。）

- [ ] **Step 3: 页面状态与模块**

`src/desktop/pages/reader/mod.rs` 顶部改成：

```rust
//! 应用内阅读器：覆盖式视图，不作为第 7 个导航页。

mod content_view;
mod search_panel;
mod toc;
mod typography;

pub mod reader_page;
pub mod theme;

pub use reader_page::{CloseReaderFn, LoadState, ReaderPage};
pub use theme::reader_colors;
```

`reader_page.rs`：

1) `use` 段加：

```rust
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::search_panel::SearchPanel;
```

2) 结构体在 `pub(super) toc_open: bool,` 之前插入：

```rust
    /// 搜索抽屉状态。
    pub(super) search: SearchPanel,
    pub(super) search_input: Entity<InputState>,
    pub(super) search_open: bool,
    /// 在跑的扫描任务的取消旗标。
    search_cancel: Option<Arc<AtomicBool>>,
```

3) `new()` 里创建输入框并订阅回车 —— 在 `let toc_filter = cx.new(...)` 之后加：

```rust
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("Reader.search.placeholder").to_string())
        });
```

结构体字面量里加：

```rust
            search: SearchPanel::new(),
            search_input,
            search_open: false,
            search_cancel: None,
```

在已有的 `cx.subscribe(&page.toc_filter, ...)` 之后再订阅回车：

```rust
        // 回车才开始扫描：边输入边搜会让整书扫描反复重启。
        cx.subscribe(&page.search_input, |this, _input, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.start_search(cx);
            }
        })
        .detach();
```

4) `close_and_flush` 开头加一行 `self.cancel_search();`。

5) `shortcuts_blocked` 整体替换为：

```rust
    fn shortcuts_blocked(&self, window: &Window, cx: &App) -> bool {
        // 抽屉里的输入框聚焦时按键归输入框；只有 Esc 例外（在 on_key_down 里处理）。
        self.toc_filter.read(cx).focus_handle().is_focused(window)
            || self.search_input.read(cx).focus_handle().is_focused(window)
    }
```

6) `on_key_down` 的 `if self.shortcuts_blocked(window, cx) { ... }` 块整体替换为：

```rust
        if self.shortcuts_blocked(window, cx) {
            if key == "escape" {
                self.toc_open = false;
                self.search_open = false;
                self.cancel_search();
                window.focus(&self.focus, cx);
                cx.notify();
            }
            return;
        }
```

7) `on_key_down` 的 `match key` 里，在 `"t" => { ... }` 之后加：

```rust
            "s" => {
                self.search_open = !self.search_open;
                self.toc_open = false;
                self.typography_open = false;
                if !self.search_open {
                    self.cancel_search();
                }
                cx.notify();
            }
```

8) `on_key_down` 的 `"escape"` 分支里，在判断 `toc_open` 之前加一层：

```rust
                if self.search_open {
                    self.search_open = false;
                    self.cancel_search();
                    cx.notify();
                } else if self.toc_open {
```

（原来的 `} else if self.toc_open {` 与后面的分支保持不变。）

- [ ] **Step 4: 工具栏与布局**

`render_toolbar` 里，在 `toc_button.on_click(...)` 那个 `.child(...)` 之后插入搜索按钮：

```rust
            .child(
                {
                    let mut search_button =
                        Button::new("reader-search").label(t!("Reader.action.search"));
                    search_button = if self.search_open {
                        search_button.primary()
                    } else {
                        search_button.ghost()
                    };
                    search_button.on_click(cx.listener(|this, _event, window, cx| {
                        this.search_open = !this.search_open;
                        this.toc_open = false;
                        this.typography_open = false;
                        if this.search_open {
                            let handle = this.search_input.read(cx).focus_handle().clone();
                            window.focus(&handle, cx);
                        } else {
                            this.cancel_search();
                        }
                        cx.notify();
                    }))
                },
            )
```

`Render::render` 里，在 `let typography = ...` 之前加：

```rust
        let search = if self.search_open {
            Some(self.render_search(cx))
        } else {
            None
        };
```

正文行改成：

```rust
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .when_some(toc, |this, toc| this.child(toc))
                    .child(body)
                    .when_some(search, |this, search| this.child(search)),
            )
```

- [ ] **Step 5: 编译并跑测试**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿。

- [ ] **Step 6: 手工验收（GUI）**

Run: `cargo run`

1. 阅读器里点「搜索」（或按 `S`）→ 右侧抽屉出现，输入框自动聚焦。
2. 输入一个正文里有的词 + 回车 → 状态先在「正在扫描 x/y 章」后变成「N 个结果」，
   结果按章分组，每条显示上下文且命中片段有高亮底色。
3. 点一条结果 → 正文跳到对应章并滚到大致位置，命中处出现更亮的底色；
   再点另一条 → 高亮跟着换（同一章内只亮当前条）。
4. 输入不含的词 + 回车 → 「0 个结果」，正文高亮清空。
5. 搜索框聚焦时按 `T` / `M` / `+` 不生效（按键归输入框）；按 `Esc` 关抽屉并回到正文。
6. 扫描大书（几千章）时界面仍可滚动、可关掉抽屉。
7. 目录抽屉 + 搜索抽屉同时只能开一个（互相顶掉）。

- [ ] **Step 7: 提交**

```bash
git add src/desktop/pages/reader
git commit -m "feat(reader): 书内搜索与命中高亮"
```

---

### Task 21: 书签面板

**Files:**
- Create: `src/desktop/pages/reader/bookmarks_panel.rs`
- Modify: `src/desktop/pages/reader/mod.rs`（`mod bookmarks_panel;`）
- Modify: `src/desktop/pages/reader/reader_page.rs`

**Interfaces:**
- Consumes: `library::Bookmark::new(chapter, chapter_ratio, excerpt)`、`AppModel::{book_snapshot, add_reading_bookmark, remove_reading_bookmark, push_warning}`、`utils::formatting::{format_local_unix_secs, truncate}`。
- Produces: `ReaderPage::{bookmarks_snapshot, add_bookmark, goto_position, render_bookmarks}`、字段 `bookmarks_open`。

- [ ] **Step 1: 书签抽屉**

新建 `src/desktop/pages/reader/bookmarks_panel.rs`：

```rust
//! 书签抽屉：添加当前书签、按章节升序展示、点击跳转、删除。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    Styled, div, px,
};
use rust_i18n::t;

use super::reader_page::ReaderPage;
use crate::library::Bookmark;
use crate::utils::formatting::{format_local_unix_secs, truncate};

impl ReaderPage {
    /// 书签列表快照：store 是唯一真相，`self.book` 只是打开时的副本。
    pub(super) fn bookmarks_snapshot(&self, cx: &App) -> Vec<Bookmark> {
        self.model
            .read(cx)
            .book_snapshot(&self.book_id)
            .map_or_else(Vec::new, |book| book.bookmarks)
    }

    /// 把当前位置加成书签；同章同位置重复时提示「已存在」。
    pub(super) fn add_bookmark(&mut self, cx: &mut Context<Self>) {
        let bookmark = Bookmark::new(
            self.chapter,
            self.chapter_ratio.clamp(0.0, 1.0),
            self.current_excerpt(),
        );
        let book_id = self.book_id.clone();
        let added = self
            .model
            .update(cx, |model, _cx| model.add_reading_bookmark(&book_id, bookmark));
        if !added {
            self.model.update(cx, |model, _cx| {
                model.push_warning(t!("Reader.bookmarks.duplicate").to_string());
            });
        }
        cx.notify();
    }

    /// 当前位置附近一段的前 40 字，作为书签摘录。
    fn current_excerpt(&self) -> String {
        let Some(chapter) = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(self.chapter))
        else {
            return String::new();
        };
        if chapter.paragraphs.is_empty() {
            return String::new();
        }
        let last = chapter.paragraphs.len() - 1;
        let index =
            (self.chapter_ratio.clamp(0.0, 1.0) * chapter.paragraphs.len() as f32) as usize;
        truncate(&chapter.paragraphs[index.min(last)], 40)
    }

    /// 跳到 `(章, 章内比例)`。书签跳转用它；搜索 / 目录各自有更精确的入口。
    pub(super) fn goto_position(&mut self, chapter: usize, ratio: f32, cx: &mut Context<Self>) {
        let Some(count) = self
            .content
            .as_ref()
            .map(|content| content.chapters.len())
        else {
            return;
        };
        if count == 0 {
            return;
        }
        self.chapter = chapter.min(count - 1);
        self.chapter_ratio = ratio.clamp(0.0, 1.0);
        self.restore_pending = true;
        self.record_progress(cx);
        cx.notify();
    }

    /// 右侧书签抽屉。
    pub(super) fn render_bookmarks(&self, cx: &mut Context<Self>) -> AnyElement {
        let bookmarks = self.bookmarks_snapshot(cx);
        let titles = self.toc_titles.clone();
        let mut list = div()
            .id("reader-bookmarks")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        if bookmarks.is_empty() {
            list = list.child(
                div()
                    .p_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("Reader.bookmarks.empty").to_string()),
            );
        }

        for bookmark in &bookmarks {
            let chapter = bookmark.chapter;
            let ratio = bookmark.chapter_ratio;
            let bookmark_id = bookmark.id.clone();
            let title = titles
                .as_ref()
                .and_then(|titles| titles.get(chapter))
                .cloned()
                .unwrap_or_else(|| {
                    t!("Reader.bookmarks.chapter", index = chapter + 1).to_string()
                });
            let time = format_local_unix_secs(
                bookmark.created_unix,
                "Reader.bookmarks.unknown_time",
                "Reader.bookmarks.invalid_time",
                "Reader.bookmarks.time_failed",
            );
            let excerpt = bookmark.excerpt.clone();
            let hover_bg = cx.theme().list_hover;
            let delete_id = bookmark_id.clone();
            list = list.child(
                div()
                    .id(format!("reader-bookmark-{bookmark_id}"))
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_bg))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.goto_position(chapter, ratio, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                Button::new(format!("reader-bookmark-delete-{delete_id}"))
                                    .ghost()
                                    .compact()
                                    .label(t!("Reader.bookmarks.delete"))
                                    .on_click(cx.listener(
                                        move |this, _event, _window, cx| {
                                            // 删完不要顺带触发整行的「跳转」。
                                            cx.stop_propagation();
                                            let book_id = this.book_id.clone();
                                            let id = delete_id.clone();
                                            this.model.update(cx, |model, _cx| {
                                                model.remove_reading_bookmark(&book_id, &id);
                                            });
                                            cx.notify();
                                        },
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(excerpt),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(time),
                    ),
            );
        }

        div()
            .w(px(320.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Reader.bookmarks.title").to_string()),
                    )
                    .child(
                        Button::new("reader-bookmark-add")
                            .primary()
                            .compact()
                            .label(t!("Reader.bookmarks.add"))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.add_bookmark(cx);
                            })),
                    ),
            )
            .child(list)
            .into_any_element()
    }
}
```

- [ ] **Step 2: 注册模块与状态**

`src/desktop/pages/reader/mod.rs` 顶部改成：

```rust
//! 应用内阅读器：覆盖式视图，不作为第 7 个导航页。

mod bookmarks_panel;
mod content_view;
mod search_panel;
mod toc;
mod typography;

pub mod reader_page;
pub mod theme;

pub use reader_page::{CloseReaderFn, LoadState, ReaderPage};
pub use theme::reader_colors;
```

`reader_page.rs`：

1) 结构体在 `pub(super) search_open: bool,` 之后插入：

```rust
    /// 书签抽屉。
    pub(super) bookmarks_open: bool,
```

2) `new()` 的结构体字面量里，`search_open: false,` 之后加：

```rust
            bookmarks_open: false,
```

- [ ] **Step 3: 工具栏、快捷键与布局**

`render_toolbar` 里，在搜索按钮那个 `.child(...)` 之后插入：

```rust
            .child(
                {
                    let mut bookmark_button =
                        Button::new("reader-bookmarks-toggle").label(t!("Reader.action.bookmarks"));
                    bookmark_button = if self.bookmarks_open {
                        bookmark_button.primary()
                    } else {
                        bookmark_button.ghost()
                    };
                    bookmark_button.on_click(cx.listener(|this, _event, _window, cx| {
                        this.bookmarks_open = !this.bookmarks_open;
                        this.toc_open = false;
                        this.search_open = false;
                        this.typography_open = false;
                        this.cancel_search();
                        cx.notify();
                    }))
                },
            )
```

`on_key_down` 的 `match key` 里，在 `"s" => { ... }` 之后加：

```rust
            "b" => {
                self.bookmarks_open = !self.bookmarks_open;
                self.toc_open = false;
                self.search_open = false;
                self.typography_open = false;
                self.cancel_search();
                cx.notify();
            }
```

`"escape"` 分支改成按面板从里到外依次关闭：

```rust
            "escape" => {
                if self.typography_open {
                    self.typography_open = false;
                    self.persist_settings(cx);
                    cx.notify();
                } else if self.search_open {
                    self.search_open = false;
                    self.cancel_search();
                    cx.notify();
                } else if self.bookmarks_open {
                    self.bookmarks_open = false;
                    cx.notify();
                } else if self.toc_open {
                    self.toc_open = false;
                    cx.notify();
                } else if let Some(handler) = self.close_handler.clone() {
                    handler(window, cx);
                }
            }
```

（同时把 `"s"` 分支里的 `self.search_open = !self.search_open;` 后面补上
`self.bookmarks_open = false;`，让三个抽屉互斥。`"t"` 分支同样补 `self.bookmarks_open = false;`。）

`Render::render` 里，在 `let search = ...` 之后加：

```rust
        let bookmarks = if self.bookmarks_open {
            Some(self.render_bookmarks(cx))
        } else {
            None
        };
```

正文行改成：

```rust
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .when_some(toc, |this, toc| this.child(toc))
                    .child(body)
                    .when_some(search, |this, search| this.child(search))
                    .when_some(bookmarks, |this, bookmarks| this.child(bookmarks)),
            )
```

- [ ] **Step 4: 编译并跑测试**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿。

- [ ] **Step 5: 手工验收（GUI）**

Run: `cargo run`

1. 阅读器里点「书签」（或按 `B`）→ 右侧抽屉出现，空列表显示提示。
2. 点「添加当前书签」→ 列表出现一条：章节名 + 摘录 + 时间。
3. 在同一位置再点一次 → 弹「已存在」警告，列表条数不变。
4. 滚到别处再加一条 → 两条按章节 / 位置排在前后。
5. 点一条书签 → 正文跳到对应位置（滚动模式按比例、翻页模式落到最近一页）。
6. 点条目右侧「删除」→ 该书签消失，且**不会**触发跳转。
7. 关掉阅读器再打开同一本书 → 书签仍在（已落盘）。
8. 书架卡片 / 详情里书签数跟着变（`bookmarks.len()`）。

- [ ] **Step 6: 提交**

```bash
git add src/desktop/pages/reader
git commit -m "feat(reader): 书签面板"
```

---

### Task 22: 沉浸模式与「更多」信息抽屉

**Files:**
- Create: `src/desktop/pages/reader/more_panel.rs`
- Modify: `src/desktop/pages/reader/mod.rs`
- Modify: `src/desktop/pages/reader/reader_page.rs`
- Modify: `src/desktop/root.rs`（`open_reader` 里注入沉浸回调）

**Interfaces:**
- Consumes: `desktop::system_open::{reveal_in_file_manager, open_with_default_app}`、`components::format_size`、`BookFormat::label()`、`window.toggle_fullscreen()`、`ReaderPage::is_immersive`（RootView 既有的标题栏开关）。
- Produces: `ReaderPage::{toggle_immersive, set_immersive_handler, render_more, render_immersive_overlay}`；`pub type ImmersiveFn = Rc<dyn Fn(&mut App)>`；字段 `toolbar_visible` / `toolbar_hide_gen` / `more_open` / `immersive_handler`。

> 说明：`immersive` 字段与 `is_immersive()` 早在 Task 2 的基础结构里就有，
> `RootView::close_reader` 也已经会在关闭时恢复窗口装饰。本任务只补「进入 / 退出」
> 与「RootView 重画」。

- [ ] **Step 1: 「更多」抽屉**

新建 `src/desktop/pages/reader/more_panel.rs`：

```rust
//! 「更多」抽屉：书籍信息 + 在文件夹中显示 / 用默认程序打开。

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, h_flex};
use gpui_kit::{
    AnyElement, App, Context, Div, FontWeight, IntoElement, ParentElement, Styled, div, px,
};
use rust_i18n::t;

use super::reader_page::ReaderPage;
use crate::desktop::components::format_size;
use crate::desktop::system_open;
use crate::library::Book;
use crate::utils::formatting::format_local_unix_secs;

impl ReaderPage {
    /// 书在 store 里的最新快照；拿不到就退回打开时的副本。
    fn current_book(&self, cx: &App) -> Book {
        self.model
            .read(cx)
            .book_snapshot(&self.book_id)
            .unwrap_or_else(|| self.book.clone())
    }

    /// 「在文件夹中显示」/「用默认程序打开」共用的执行体。
    fn run_file_action(&self, path: &str, open_with_app: bool, cx: &mut Context<Self>) {
        let target = PathBuf::from(path);
        let result = if open_with_app {
            system_open::open_with_default_app(&target)
        } else {
            system_open::reveal_in_file_manager(&target)
        };
        if let Err(message) = result {
            self.model
                .update(cx, |model, _cx| model.push_error(message));
        }
    }

    /// 右侧「更多」抽屉。
    pub(super) fn render_more(&self, cx: &mut Context<Self>) -> AnyElement {
        let book = self.current_book(cx);
        let unknown = t!("Reader.info.unknown").to_string();
        // 时间戳缺失 / 非法 / 格式化失败都显示同一个占位。
        let time_unknown = "Reader.info.unknown";

        let mut rows: Vec<Div> = Vec::new();
        rows.push(more_row(
            t!("Reader.info.author").to_string(),
            filled(&book.author, &unknown),
            cx,
        ));
        let intro = book
            .intro
            .as_deref()
            .map_or_else(|| unknown.clone(), str::to_string);
        rows.push(more_row(t!("Reader.info.intro").to_string(), intro, cx));
        rows.push(more_row(
            t!("Reader.info.format").to_string(),
            book.format.label().to_string(),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.size").to_string(),
            format_size(book.size_bytes),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.chapters").to_string(),
            book.chapter_count
                .map_or_else(|| unknown.clone(), |count| count.to_string()),
            cx,
        ));
        let percent = book.progress.percent.clamp(0.0, 100.0);
        rows.push(more_row(
            t!("Reader.info.progress").to_string(),
            format!("{percent:.1}%"),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.bookmarks").to_string(),
            book.bookmarks.len().to_string(),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.added").to_string(),
            format_local_unix_secs(book.added_unix, time_unknown, time_unknown, time_unknown),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.last_opened").to_string(),
            book.last_opened_unix.map_or_else(
                || unknown.clone(),
                |secs| format_local_unix_secs(secs, time_unknown, time_unknown, time_unknown),
            ),
            cx,
        ));
        rows.push(more_row(
            t!("Reader.info.path").to_string(),
            book.path.clone(),
            cx,
        ));

        let title = book.title.clone();
        let reveal_path = book.path.clone();
        let open_path = book.path.clone();

        div()
            .w(px(340.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Reader.info.title").to_string()),
                    )
                    .child(
                        Button::new("reader-more-close")
                            .ghost()
                            .compact()
                            .label(t!("Reader.info.close"))
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.more_open = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("reader-more-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .children(rows)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("reader-file-reveal")
                                    .ghost()
                                    .compact()
                                    .label(t!("Reader.info.reveal"))
                                    .on_click(cx.listener(
                                        move |this, _event, _window, cx| {
                                            this.run_file_action(&reveal_path, false, cx);
                                        },
                                    )),
                            )
                            .child(
                                Button::new("reader-file-open")
                                    .ghost()
                                    .compact()
                                    .label(t!("Reader.info.open"))
                                    .on_click(cx.listener(
                                        move |this, _event, _window, cx| {
                                            this.run_file_action(&open_path, true, cx);
                                        },
                                    )),
                            ),
                    ),
            )
            .into_any_element()
    }
}

/// 一行「标签：值」。
fn more_row(label: String, value: String, cx: &App) -> Div {
    h_flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .w(px(72.))
                .flex_none()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(div().flex_1().min_w_0().text_xs().child(value))
}

/// 空字符串换成占位文字。
fn filled(value: &str, fallback: &str) -> String {
    if value.trim().is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}
```

- [ ] **Step 2: 注册模块**

`src/desktop/pages/reader/mod.rs` 顶部再加一行模块声明，并导出新类型：

```rust
//! 应用内阅读器：覆盖式视图，不作为第 7 个导航页。

mod bookmarks_panel;
mod content_view;
mod more_panel;
mod search_panel;
mod toc;
mod typography;

pub mod reader_page;
pub mod theme;

pub use reader_page::{CloseReaderFn, ImmersiveFn, LoadState, ReaderPage};
pub use theme::reader_colors;
```

- [ ] **Step 3: 状态与回调**

`src/desktop/pages/reader/reader_page.rs`：

1) `CloseReaderFn` 类型别名之后加：

```rust
/// 沉浸模式变化通知：`RootView` 用它重画自己的标题栏。
pub type ImmersiveFn = Rc<dyn Fn(&mut App)>;
```

2) 结构体里 `close_handler` 之前加一个字段：

```rust
    immersive_handler: Option<ImmersiveFn>,
```

并在 `pub(super) bookmarks_open: bool,` 之后加：

```rust
    /// 沉浸模式的悬浮工具栏是否可见。
    toolbar_visible: bool,
    /// 收起工具栏的延时任务代次：鼠标重新进入就作废旧的延时。
    toolbar_hide_gen: u64,
    /// 「更多」抽屉。
    pub(super) more_open: bool,
```

3) `new()` 的结构体字面量里补四个初值（和上面字段写在一起）：

```rust
            toolbar_visible: false,
            toolbar_hide_gen: 0,
            more_open: false,
            immersive_handler: None,
```

4) `set_close_handler` 之后加：

```rust
    /// 注入「沉浸模式变了」的回调（`RootView` 调用）。
    pub fn set_immersive_handler(&mut self, handler: ImmersiveFn) {
        self.immersive_handler = Some(handler);
    }
```

5) `close_and_flush` 之后加两个方法：

```rust
    /// 关掉所有抽屉 —— 抽屉之间互斥，开新的之前先调它。
    fn close_drawers(&mut self, cx: &mut Context<Self>) {
        if self.typography_open {
            self.persist_settings(cx);
        }
        self.toc_open = false;
        self.search_open = false;
        self.bookmarks_open = false;
        self.typography_open = false;
        self.more_open = false;
        self.cancel_search();
    }

    /// 进出沉浸模式：进 = 隐藏应用内工具栏 + 窗口全屏；出 = 反过来。
    pub(super) fn toggle_immersive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.immersive = !self.immersive;
        self.toolbar_visible = false;
        if self.immersive {
            self.close_drawers(cx);
        }
        window.toggle_fullscreen();
        if let Some(handler) = self.immersive_handler.clone() {
            handler(cx);
        }
        cx.notify();
    }
```

- [ ] **Step 4: 沉浸悬浮工具栏**

`reader_page.rs` 里 `render_error` 之后加：

```rust
    /// 沉浸模式：顶部 8px 热区唤出悬浮工具栏，鼠标移出 1.5 s 后收起。
    fn render_immersive_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.toolbar_visible {
            return div()
                .id("reader-immersive-hotzone")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(8.0))
                .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                    if *hovered {
                        this.toolbar_visible = true;
                        cx.notify();
                    }
                }))
                .into_any_element();
        }

        let background = reader_colors(self.reader_config(cx).theme, &*cx).background;
        div()
            .id("reader-immersive-toolbar")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                this.toolbar_hide_gen = this.toolbar_hide_gen.wrapping_add(1);
                let generation = this.toolbar_hide_gen;
                if *hovered {
                    this.toolbar_visible = true;
                    cx.notify();
                } else if this.immersive {
                    // 移出后 1.5 s 再收起：从工具栏往正文划时不会闪一下。
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(1500))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            if this.toolbar_hide_gen == generation && this.immersive {
                                this.toolbar_visible = false;
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                }
            }))
            .child(div().bg(background).child(self.render_toolbar(cx)))
            .into_any_element()
    }
```

- [ ] **Step 5: 整体替换 `render_toolbar`**

把 `reader_page.rs` 里整个 `fn render_toolbar`（从 `fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {`
到它与下一个 `fn` 之间的 `}`）替换为：

```rust
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let close_handler = self.close_handler.clone();
        let mode_label = match self.mode {
            ReadingMode::Scroll => t!("Reader.mode.page"),
            ReadingMode::Page => t!("Reader.mode.scroll"),
        };
        let immersive_label = if self.immersive {
            t!("Reader.action.exit_immersive")
        } else {
            t!("Reader.action.immersive")
        };

        let mut toc_button = Button::new("reader-toc").label(t!("Reader.action.toc"));
        toc_button = if self.toc_open {
            toc_button.primary()
        } else {
            toc_button.ghost()
        };
        let mut more_button = Button::new("reader-more").label(t!("Reader.action.more"));
        more_button = if self.more_open {
            more_button.primary()
        } else {
            more_button.ghost()
        };
        let mut typography_button = Button::new("reader-typography").label("Aa");
        typography_button = if self.typography_open {
            typography_button.primary()
        } else {
            typography_button.ghost()
        };
        let mut search_button = Button::new("reader-search").label(t!("Reader.action.search"));
        search_button = if self.search_open {
            search_button.primary()
        } else {
            search_button.ghost()
        };
        let mut bookmarks_button =
            Button::new("reader-bookmarks-toggle").label(t!("Reader.action.bookmarks"));
        bookmarks_button = if self.bookmarks_open {
            bookmarks_button.primary()
        } else {
            bookmarks_button.ghost()
        };
        let mut immersive_button =
            Button::new("reader-immersive-toggle").label(immersive_label);
        immersive_button = if self.immersive {
            immersive_button.primary()
        } else {
            immersive_button.ghost()
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("reader-back")
                    .label(t!("Reader.action.back"))
                    .on_click(move |_event, window, cx| {
                        if let Some(handler) = &close_handler {
                            handler(window, cx);
                        }
                    }),
            )
            .child(toc_button.on_click(cx.listener(
                |this, _event, _window, cx| {
                    let open = !this.toc_open;
                    this.close_drawers(cx);
                    this.toc_open = open;
                    cx.notify();
                },
            )))
            .child(more_button.on_click(cx.listener(
                |this, _event, _window, cx| {
                    let open = !this.more_open;
                    this.close_drawers(cx);
                    this.more_open = open;
                    cx.notify();
                },
            )))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_center()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(SharedString::from(self.book.title.clone())),
            )
            .child(typography_button.on_click(cx.listener(
                |this, _event, _window, cx| {
                    let open = !this.typography_open;
                    this.close_drawers(cx);
                    this.typography_open = open;
                    cx.notify();
                },
            )))
            .child(search_button.on_click(cx.listener(
                |this, _event, window, cx| {
                    let open = !this.search_open;
                    this.close_drawers(cx);
                    this.search_open = open;
                    if this.search_open {
                        let handle = this.search_input.read(cx).focus_handle().clone();
                        window.focus(&handle, cx);
                    }
                    cx.notify();
                },
            )))
            .child(bookmarks_button.on_click(cx.listener(
                |this, _event, _window, cx| {
                    let open = !this.bookmarks_open;
                    this.close_drawers(cx);
                    this.bookmarks_open = open;
                    cx.notify();
                },
            )))
            .child(
                Button::new("reader-mode")
                    .label(mode_label)
                    .on_click(cx.listener(|this, _event, _window, cx| this.toggle_mode(cx))),
            )
            .child(immersive_button.on_click(cx.listener(
                |this, _event, window, cx| {
                    this.toggle_immersive(window, cx);
                },
            )))
    }
```

- [ ] **Step 6: 按键**

`on_key_down` 里：

1) `if self.shortcuts_blocked(window, cx) { ... }` 整块替换为：

```rust
        if self.shortcuts_blocked(window, cx) {
            if key == "escape" {
                self.close_drawers(cx);
                window.focus(&self.focus, cx);
                cx.notify();
            }
            return;
        }
```

2) `"t"` / `"s"` / `"b"` 三个分支整体替换成下面三段：

```rust
            "t" => {
                let open = !self.toc_open;
                self.close_drawers(cx);
                self.toc_open = open;
                cx.notify();
            }
            "s" => {
                let open = !self.search_open;
                self.close_drawers(cx);
                self.search_open = open;
                cx.notify();
            }
            "b" => {
                let open = !self.bookmarks_open;
                self.close_drawers(cx);
                self.bookmarks_open = open;
                cx.notify();
            }
```

3) `"m" => self.toggle_mode(cx),` 之后加：

```rust
            "f11" => self.toggle_immersive(window, cx),
```

4) `"escape"` 分支整体替换为：

```rust
            "escape" => {
                if self.toc_open
                    || self.search_open
                    || self.bookmarks_open
                    || self.typography_open
                    || self.more_open
                {
                    self.close_drawers(cx);
                    cx.notify();
                } else if self.immersive {
                    self.toggle_immersive(window, cx);
                } else if let Some(handler) = self.close_handler.clone() {
                    handler(window, cx);
                }
            }
```

- [ ] **Step 7: `Render` 布局**

`impl Render for ReaderPage` 的 `render()` 里：

1) 在 Task 21 加的那段 `let bookmarks = ...` 之后再加：

```rust
        let more = if self.more_open {
            Some(self.render_more(cx))
        } else {
            None
        };
        let toolbar = self.render_toolbar(cx);
        let overlay = if self.immersive {
            Some(self.render_immersive_overlay(cx))
        } else {
            None
        };
```

2) 根 `div()` 从 `.child(self.render_toolbar(cx))` 到末尾 `.when_some(typography, ...)` 整段替换为：

```rust
            .when(!self.immersive, |this| this.child(toolbar))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .when_some(toc, |this, toc| this.child(toc))
                    .child(body)
                    .when_some(search, |this, search| this.child(search))
                    .when_some(bookmarks, |this, bookmarks| this.child(bookmarks))
                    .when_some(more, |this, more| this.child(more)),
            )
            .when_some(typography, |this, panel| this.child(panel))
            .when_some(overlay, |this, overlay| this.child(overlay))
```

- [ ] **Step 8: `RootView` 注入回调**

`src/desktop/root.rs` 的 `open_reader` 里，在 `page.set_close_handler(...)` 那个 `reader.update(...)` 之后再加一次：

```rust
        let immersive_handle = cx.entity().downgrade();
        reader.update(cx, |page, _cx| {
            page.set_immersive_handler(Rc::new(move |cx| {
                let _ = immersive_handle.update(cx, |_root, cx| cx.notify());
            }));
        });
```

（`RootView::render` 已经按 `is_immersive()` 决定是否画标题栏，这里只要让它重画一次。）

- [ ] **Step 9: 编译并跑测试**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿。

- [ ] **Step 10: 手工验收（GUI）**

Run: `cargo run`

1. 工具栏多出「更多」「沉浸」两个按钮；点「更多」→ 右侧抽屉显示作者 / 格式 / 大小 / 章节 / 进度 / 书签数 / 加入时间 / 最近阅读 / 路径与两个文件按钮。
2. 「在文件夹中显示」→ 资源管理器定位到该文件；「用默认程序打开」→ 系统默认程序打开它。删掉文件再点 → 顶部弹出错误提示，不 panic。
3. 点「沉浸」或按 `F11` → 窗口全屏、应用标题栏与工具栏一起消失；鼠标移到窗口最顶端 8px → 悬浮工具栏落下，移开 → 收起。
4. 沉浸模式下点「目录」/「搜索」/「书签」/「更多」仍能打开抽屉；再点一次或按 `Esc` 收起。
5. `Esc` 顺序：先关抽屉 → 再退出沉浸（窗口恢复窗口化、标题栏回来）→ 再退出阅读器。
6. 沉浸模式下按 `M` 切换翻页 / 滚动、方向键翻页都照常。
7. 沉浸模式下点「返回」或切到别的导航页 → 窗口一定要恢复窗口化，不能卡在全屏。

- [ ] **Step 11: 提交**

```bash
git add src/desktop/pages/reader src/desktop/root.rs
git commit -m "feat(reader): 沉浸模式与书籍信息抽屉"
```

---

### Task 23: 三语文案、README 与端到端验收

**Files:**
- Modify: `locales/app.yml`
- Modify: `src/i18n.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: 前 22 个任务里所有 `t!("Library.*")` / `t!("Reader.*")` 调用点，以及 `format_local_unix_secs` 的三个 fallback key。
- Produces: 三语齐备的 `Library.*` / `Reader.*` 文案、`LIBRARY_KEYS` / `READER_KEYS` 覆盖率测试、README 的功能 / 结构 / 快捷键条目。

- [ ] **Step 1: 重写 `Library:` 段并新增 `Reader:` 段**

`locales/app.yml` 里，把从 `Library:` 这一行开始、到 `# 书源管理页面` 注释之前的**全部内容**
整体替换为下面的 `Library:` 段 + 新增的 `Reader:` 段（旧的 `filter_option_all` /
`download_path_label` / `delete_dialog.*` / `fallback_unknown_filename` 等已随
Task 12–17 重写页面而失效，一并删掉；`time.*` 保留，`format_local_unix_secs` 还在用）：

```yaml
Library:
  page_title:
    en: Library
    zh-CN: 本地书库
    zh-TW: 本地書庫
  # PageHeader 副标题：%{count} 本书 + 扫描目录 %{dir}。
  subtitle:
    en: "%{count} books · %{dir}"
    zh-CN: "共 %{count} 本 · %{dir}"
    zh-TW: "共 %{count} 本 · %{dir}"
  search_placeholder:
    en: "Search title or author…"
    zh-CN: 搜索书名或作者…
    zh-TW: 搜尋書名或作者…
  action_import:
    en: Import
    zh-CN: 导入
    zh-TW: 匯入
  action_rescan:
    en: Rescan
    zh-CN: 重新扫描
    zh-TW: 重新掃描
  action_load_more:
    en: "Load more"
    zh-CN: 加载更多
    zh-TW: 載入更多
  action_clear_filters:
    en: "Clear filters"
    zh-CN: 清除筛选
    zh-TW: 清除篩選
  filter:
    all:
      en: All
      zh-CN: 全部
      zh-TW: 全部
    reading:
      en: Reading
      zh-CN: 在读
      zh-TW: 在讀
    unread:
      en: Unread
      zh-CN: 未读
      zh-TW: 未讀
    finished:
      en: Finished
      zh-CN: 已读
      zh-TW: 已讀
    favorites:
      en: Favorites
      zh-CN: 收藏
      zh-TW: 收藏
  # 排序按钮显示的是「当前排序」，点一下切下一种。
  sort:
    title:
      en: "Sort: title"
      zh-CN: 排序：书名
      zh-TW: 排序：書名
    last_opened:
      en: "Sort: recent"
      zh-CN: 排序：最近阅读
      zh-TW: 排序：最近閱讀
    added_at:
      en: "Sort: added"
      zh-CN: 排序：加入时间
      zh-TW: 排序：加入時間
    size:
      en: "Sort: size"
      zh-CN: 排序：文件大小
      zh-TW: 排序：檔案大小
  status:
    unread:
      en: Unread
      zh-CN: 未读
      zh-TW: 未讀
    reading:
      en: Reading
      zh-CN: 在读
      zh-TW: 在讀
    finished:
      en: Finished
      zh-CN: 已读
      zh-TW: 已讀
    # 非 txt / epub 的书点了「继续阅读」时的提示。
    unsupported:
      en: "This format cannot be read in the app yet"
      zh-CN: 该格式暂不支持应用内阅读
      zh-TW: 該格式暫不支援應用程式內閱讀
  card:
    missing:
      en: "File missing"
      zh-CN: 文件已不存在
      zh-TW: 檔案已不存在
  empty_title:
    en: "Library is empty"
    zh-CN: 本地书库为空
    zh-TW: 本地書庫為空
  empty_subtitle:
    en: "Scanner finds no txt / epub in the download directory."
    zh-CN: 下载目录里没有找到 txt / epub 书籍。
    zh-TW: 下載目錄裡沒有找到 txt / epub 書籍。
  empty_filtered_title:
    en: "Nothing matches"
    zh-CN: 没有符合条件的书
    zh-TW: 沒有符合條件的書
  empty_filtered_subtitle:
    en: "Try another keyword, or clear the filters."
    zh-CN: 换个关键字，或清除筛选条件。
    zh-TW: 換個關鍵字，或清除篩選條件。
  scan_failed:
    en: "Scan failed"
    zh-CN: "扫描失败"
    zh-TW: "掃描失敗"
  import:
    dialog_title:
      en: "Import books"
      zh-CN: 导入书籍
      zh-TW: 匯入書籍
    filter_books:
      en: "Books (txt / epub)"
      zh-CN: 书籍（txt / epub）
      zh-TW: 書籍（txt / epub）
    filter_all:
      en: "All files"
      zh-CN: 所有文件
      zh-TW: 所有檔案
    # %{added} 新增本数，%{existing} 已在书架的本数。
    done:
      en: "Imported %{added}, already in library %{existing}"
      zh-CN: 已导入 %{added} 本，已在书架 %{existing} 本
      zh-TW: 已匯入 %{added} 本，已在書架 %{existing} 本
    # %{file} 文件名，%{err} 失败原因。
    failed_item:
      en: "%{file}: %{err}"
      zh-CN: "%{file}：%{err}"
      zh-TW: "%{file}：%{err}"
  # 用户可见错误摘要（数据层直接 `t!` 后进 toast / 导入失败列表）。
  # IO 失败不透传 key，直接拼系统原文。
  error:
    not_a_file:
      en: "Not a file"
      zh-CN: 不是文件
      zh-TW: 不是檔案
    unsupported_format:
      en: "Unsupported file format"
      zh-CN: 不支持的文件格式
      zh-TW: 不支援的檔案格式
    book_not_found:
      en: "That book is no longer on the shelf"
      zh-CN: 书籍已不在书架中
      zh-TW: 書籍已不在書架中
    format_mismatch:
      en: "The chosen file has a different format"
      zh-CN: 选择的文件格式与书籍不一致
      zh-TW: 選擇的檔案格式與書籍不一致
    already_on_shelf:
      en: "That file is already on the shelf"
      zh-CN: 该文件已在书架中
      zh-TW: 該檔案已在書架中
    # %{detail} 系统原始错误。
    load_failed:
      en: "Could not read library.json: %{detail}"
      zh-CN: "读取 library.json 失败：%{detail}"
      zh-TW: "讀取 library.json 失敗：%{detail}"
    # %{path} 备份文件名，%{detail} 解析错误。
    corrupt_backup:
      en: "library.json could not be parsed and was backed up to %{path}: %{detail}"
      zh-CN: "library.json 解析失败，已备份为 %{path}：%{detail}"
      zh-TW: "library.json 解析失敗，已備份為 %{path}：%{detail}"
    # %{detail} 备份失败原因；接在 corrupt_backup 之后。
    backup_failed:
      en: " (backup failed: %{detail})"
      zh-CN: "（备份失败：%{detail}）"
      zh-TW: "（備份失敗：%{detail}）"
    # %{version} 文件版本，%{supported} 当前支持的版本。
    future_version:
      en: "library.json is version %{version}, newer than the supported %{supported}; this run is read-only and will not overwrite it"
      zh-CN: "library.json 版本 %{version} 高于当前支持的 %{supported}，本次只读，不会覆盖"
      zh-TW: "library.json 版本 %{version} 高於目前支援的 %{supported}，本次唯讀，不會覆寫"
    save_refused:
      en: "library.json comes from a newer version; refusing to overwrite. Upgrade the app to save."
      zh-CN: "library.json 来自更高版本，拒绝覆盖；请升级应用后再保存"
      zh-TW: "library.json 來自更高版本，拒絕覆寫；請先升級應用程式再儲存"
    # 上次读取失败 / 备份失败：磁盘上的文件没被保住，写回就等于抹掉它。
    save_refused_after_load_failure:
      en: "The shelf file could not be read or backed up; refusing to overwrite it. Move or delete it, then restart the app."
      zh-CN: "书架文件读取或备份失败，拒绝覆盖；请移走或删除该文件后重启应用"
      zh-TW: "書架檔案讀取或備份失敗，拒絕覆寫；請移走或刪除該檔案後重新啟動應用程式"
    create_dir_failed:
      en: "Could not create the shelf folder"
      zh-CN: 创建书架目录失败
      zh-TW: 建立書架目錄失敗
    write_failed:
      en: "Could not write library.json"
      zh-CN: 写入 library.json 失败
      zh-TW: 寫入 library.json 失敗
  info:
    title:
      en: "Book info"
      zh-CN: 书籍信息
      zh-TW: 書籍資訊
    unknown:
      en: "(unknown)"
      zh-CN: "(未知)"
      zh-TW: "(未知)"
    format:
      en: Format
      zh-CN: 格式
      zh-TW: 格式
    path:
      en: Path
      zh-CN: 路径
      zh-TW: 路徑
    size:
      en: Size
      zh-CN: 大小
      zh-TW: 大小
    chapters:
      en: Chapters
      zh-CN: 章节
      zh-TW: 章節
    added:
      en: Added
      zh-CN: 加入时间
      zh-TW: 加入時間
    progress:
      en: Progress
      zh-CN: 进度
      zh-TW: 進度
    bookmarks:
      en: Bookmarks
      zh-CN: 书签
      zh-TW: 書籤
  edit:
    title:
      en: "Edit info"
      zh-CN: 编辑信息
      zh-TW: 編輯資訊
    title_placeholder:
      en: Title
      zh-CN: 书名
      zh-TW: 書名
    author_placeholder:
      en: Author
      zh-CN: 作者
      zh-TW: 作者
    pick_cover:
      en: "Choose cover"
      zh-CN: 选择封面
      zh-TW: 選擇封面
    remove_cover:
      en: "Remove cover"
      zh-CN: 移除封面
      zh-TW: 移除封面
    no_cover:
      en: "No cover"
      zh-CN: 没有封面
      zh-TW: 沒有封面
    cover_filter:
      en: Images
      zh-CN: 图片
      zh-TW: 圖片
    save:
      en: Save
      zh-CN: 保存
      zh-TW: 儲存
    cancel:
      en: Cancel
      zh-CN: 取消
      zh-TW: 取消
  # 卡片右上角「⋯」菜单。
  actions:
    title:
      en: "Actions · %{title}"
      zh-CN: 操作 · %{title}
      zh-TW: 操作 · %{title}
    continue:
      en: "Continue reading"
      zh-CN: 继续阅读
      zh-TW: 繼續閱讀
    info:
      en: "Book info"
      zh-CN: 书籍信息
      zh-TW: 書籍資訊
    edit:
      en: "Edit info"
      zh-CN: 编辑信息
      zh-TW: 編輯資訊
    relink:
      en: "Relink file"
      zh-CN: 重新定位文件
      zh-TW: 重新定位檔案
    reveal:
      en: "Show in folder"
      zh-CN: 在文件夹中显示
      zh-TW: 在資料夾中顯示
    open_external:
      en: "Open with system app"
      zh-CN: 用系统程序打开
      zh-TW: 用系統程式開啟
    hide:
      en: "Remove from library"
      zh-CN: 从书架移除
      zh-TW: 從書架移除
    delete_file:
      en: "Delete file"
      zh-CN: 删除文件
      zh-TW: 刪除檔案
    restart:
      en: "Start over"
      zh-CN: 从头开始
      zh-TW: 從頭開始
  hide_dialog:
    title:
      en: "Remove from library"
      zh-CN: 从书架移除
      zh-TW: 從書架移除
    message:
      en: 'Remove "%{title}" from the library? The file stays on disk.'
      zh-CN: '把「%{title}」从书架移除？磁盘上的文件不会被删除。'
      zh-TW: '把「%{title}」從書架移除？磁碟上的檔案不會被刪除。'
    confirm_button:
      en: Remove
      zh-CN: 移除
      zh-TW: 移除
  delete_file_dialog:
    title:
      en: "Delete file"
      zh-CN: 删除文件
      zh-TW: 刪除檔案
    message:
      en: 'Delete the file of "%{title}"? This cannot be undone.'
      zh-CN: '确定删除「%{title}」的文件吗？此操作无法撤销。'
      zh-TW: '確定刪除「%{title}」的檔案嗎？此操作無法撤銷。'
    confirm_button:
      en: Delete
      zh-CN: 删除
      zh-TW: 刪除
  dialog:
    cancel_button:
      en: Cancel
      zh-CN: 取消
      zh-TW: 取消
  relink:
    dialog_title:
      en: "Pick the book file again"
      zh-CN: 重新选择书籍文件
      zh-TW: 重新選擇書籍檔案
  delete:
    # %{err} 失败原因。
    failed:
      en: "Delete failed: %{err}"
      zh-CN: "删除失败：%{err}"
      zh-TW: "刪除失敗：%{err}"
  # 时间戳的三个 fallback，`utils::formatting::format_local_unix_secs` 与书架卡片共用。
  time:
    unknown:
      en: "(unknown)"
      zh-CN: "(未知)"
      zh-TW: "(未知)"
    invalid:
      en: "(invalid time)"
      zh-CN: "(无效时间)"
      zh-TW: "(無效時間)"
    format_failed:
      en: "(format failed)"
      zh-CN: "(格式化失败)"
      zh-TW: "(格式化失敗)"

# 应用内阅读器（`src/desktop/pages/reader/`）的所有文案。
# 技术名词（TXT / EPUB / Aa）不翻译；`mode.*` 是工具栏上那个「切到另一种模式」的按钮文案。
Reader:
  action:
    back:
      en: Back
      zh-CN: 返回
      zh-TW: 返回
    toc:
      en: Contents
      zh-CN: 目录
      zh-TW: 目錄
    search:
      en: Search
      zh-CN: 搜索
      zh-TW: 搜尋
    bookmarks:
      en: Bookmarks
      zh-CN: 书签
      zh-TW: 書籤
    more:
      en: More
      zh-CN: 更多
      zh-TW: 更多
    immersive:
      en: "Full screen"
      zh-CN: 沉浸
      zh-TW: 沉浸
    exit_immersive:
      en: "Exit full screen"
      zh-CN: 退出沉浸
      zh-TW: 退出沉浸
    close:
      en: Close
      zh-CN: 关闭
      zh-TW: 關閉
    prev_chapter:
      en: "Prev chapter"
      zh-CN: 上一章
      zh-TW: 上一章
    next_chapter:
      en: "Next chapter"
      zh-CN: 下一章
      zh-TW: 下一章
    prev_page:
      en: "Prev page"
      zh-CN: 上一页
      zh-TW: 上一頁
    next_page:
      en: "Next page"
      zh-CN: 下一页
      zh-TW: 下一頁
  mode:
    scroll:
      en: Scroll
      zh-CN: 滚动
      zh-TW: 滑動
    page:
      en: Pages
      zh-CN: 翻页
      zh-TW: 翻頁
  # %{current} / %{total} 都是 1-based。
  chapter_indicator:
    en: "Chapter %{current} / %{total}"
    zh-CN: 第 %{current} / %{total} 章
    zh-TW: 第 %{current} / %{total} 章
  page_indicator:
    en: "Chapter %{chapter} / %{total} · Page %{page} / %{pages}"
    zh-CN: 第 %{chapter} / %{total} 章 · 第 %{page} / %{pages} 页
    zh-TW: 第 %{chapter} / %{total} 章 · 第 %{page} / %{pages} 頁
  loading:
    en: "Loading…"
    zh-CN: 正在载入…
    zh-TW: 正在載入…
  error:
    io:
      en: "Failed to read the file"
      zh-CN: 读取文件失败
      zh-TW: 讀取檔案失敗
    missing:
      en: "File not found — relink it from the shelf"
      zh-CN: 文件不存在，请回书架重新定位
      zh-TW: 檔案不存在，請回書架重新定位
    unsupported:
      en: "This format cannot be read in the app yet"
      zh-CN: 该格式暂不支持应用内阅读
      zh-TW: 該格式暫不支援應用程式內閱讀
    decode:
      en: "Failed to decode the text"
      zh-CN: 文本解码失败
      zh-TW: 文字解碼失敗
    encoding:
      en: "Unknown or unsupported encoding"
      zh-CN: 未知或不支持的编码
      zh-TW: 未知或不支援的編碼
    epub:
      en: "Broken EPUB"
      zh-CN: EPUB 结构损坏
      zh-TW: EPUB 結構損壞
    no_content:
      en: "No readable content"
      zh-CN: 没有可显示的正文
      zh-TW: 沒有可顯示的正文
    layout_fallback:
      en: "Window or font too small — switched back to scroll mode"
      zh-CN: 窗口或字号太小，已回退到滚动模式
      zh-TW: 視窗或字級太小，已回退到滑動模式
    # 错误面板上的按钮。
    action:
      relink:
        en: "Relink file"
        zh-CN: 重新定位
        zh-TW: 重新定位
      open_external:
        en: "Open with system app"
        zh-CN: 用系统程序打开
        zh-TW: 用系統程式開啟
      back:
        en: "Back to shelf"
        zh-CN: 返回书架
        zh-TW: 返回書架
  toc:
    title:
      en: "Contents"
      zh-CN: 目录
      zh-TW: 目錄
    empty:
      en: "No chapters"
      zh-CN: 没有章节
      zh-TW: 沒有章節
    filter_placeholder:
      en: "Filter chapters…"
      zh-CN: 过滤章节…
      zh-TW: 過濾章節…
    chapter_count:
      en: "%{count} chapters"
      zh-CN: "%{count} 章"
      zh-TW: "%{count} 章"
  typography:
    title:
      en: "Type"
      zh-CN: 排版
      zh-TW: 排版
    font_size:
      en: "Font size"
      zh-CN: 字号
      zh-TW: 字級
    line_height:
      en: "Line height"
      zh-CN: 行高
      zh-TW: 行高
    paragraph_spacing:
      en: "Paragraph gap"
      zh-CN: 段距
      zh-TW: 段距
    page_width:
      en: "Page width"
      zh-CN: 页宽
      zh-TW: 頁寬
    indent:
      en: "Indent"
      zh-CN: 首行缩进
      zh-TW: 首行縮排
    theme:
      app:
        en: "App theme"
        zh-CN: 跟随应用
        zh-TW: 跟隨應用程式
      paper:
        en: Paper
        zh-CN: 纸白
        zh-TW: 紙白
      sepia:
        en: Sepia
        zh-CN: 米黄
        zh-TW: 米黃
      green:
        en: "Eye care"
        zh-CN: 护眼绿
        zh-TW: 護眼綠
      night:
        en: Night
        zh-CN: 夜间
        zh-TW: 夜間
  search:
    title:
      en: Search
      zh-CN: 搜索
      zh-TW: 搜尋
    placeholder:
      en: "Type a keyword, press Enter"
      zh-CN: 输入关键字，回车搜索
      zh-TW: 輸入關鍵字，按 Enter 搜尋
    hint:
      en: "Enter to search · Esc to close"
      zh-CN: 回车搜索 · Esc 关闭
      zh-TW: 按 Enter 搜尋 · Esc 關閉
    # %{done} / %{total} 已扫描章数。
    scanning:
      en: "Scanning %{done} / %{total} chapters"
      zh-CN: 正在扫描 %{done} / %{total} 章
      zh-TW: 正在掃描 %{done} / %{total} 章
    results:
      en: "%{count} results"
      zh-CN: "%{count} 个结果"
      zh-TW: "%{count} 個結果"
  bookmarks:
    title:
      en: Bookmarks
      zh-CN: 书签
      zh-TW: 書籤
    add:
      en: "Add bookmark"
      zh-CN: 添加当前书签
      zh-TW: 新增目前書籤
    # 章节标题缺失时的兜底，%{index} 是 1-based 章号。
    chapter:
      en: "Chapter %{index}"
      zh-CN: 第 %{index} 章
      zh-TW: 第 %{index} 章
    delete:
      en: Delete
      zh-CN: 删除
      zh-TW: 刪除
    duplicate:
      en: "A bookmark already exists here"
      zh-CN: 当前位置已经有书签
      zh-TW: 目前位置已經有書籤
    empty:
      en: "No bookmarks yet"
      zh-CN: 还没有书签
      zh-TW: 還沒有書籤
    unknown_time:
      en: "(unknown)"
      zh-CN: "(未知)"
      zh-TW: "(未知)"
    invalid_time:
      en: "(invalid time)"
      zh-CN: "(无效时间)"
      zh-TW: "(無效時間)"
    time_failed:
      en: "(format failed)"
      zh-CN: "(格式化失败)"
      zh-TW: "(格式化失敗)"
  info:
    title:
      en: "Book info"
      zh-CN: 书籍信息
      zh-TW: 書籍資訊
    close:
      en: Close
      zh-CN: 关闭
      zh-TW: 關閉
    author:
      en: Author
      zh-CN: 作者
      zh-TW: 作者
    intro:
      en: Intro
      zh-CN: 简介
      zh-TW: 簡介
    format:
      en: Format
      zh-CN: 格式
      zh-TW: 格式
    size:
      en: Size
      zh-CN: 大小
      zh-TW: 大小
    chapters:
      en: Chapters
      zh-CN: 章节
      zh-TW: 章節
    progress:
      en: Progress
      zh-CN: 进度
      zh-TW: 進度
    bookmarks:
      en: Bookmarks
      zh-CN: 书签
      zh-TW: 書籤
    added:
      en: Added
      zh-CN: 加入时间
      zh-TW: 加入時間
    last_opened:
      en: "Last read"
      zh-CN: 最近阅读
      zh-TW: 最近閱讀
    path:
      en: Path
      zh-CN: 路径
      zh-TW: 路徑
    reveal:
      en: "Show in folder"
      zh-CN: 在文件夹中显示
      zh-TW: 在資料夾中顯示
    open:
      en: "Open with system app"
      zh-CN: 用默认程序打开
      zh-TW: 用預設程式開啟
    unknown:
      en: "(unknown)"
      zh-CN: "(未知)"
      zh-TW: "(未知)"
```

- [ ] **Step 2: key 覆盖率测试**

`src/i18n.rs` 的 `mod tests` 里，在 `TOOLS_KEYS` 常量与它的测试之后再加两个常量 + 两个测试：

```rust
    const LIBRARY_KEYS: &[&str] = &[
        "Library.page_title",
        "Library.subtitle",
        "Library.search_placeholder",
        "Library.action_import",
        "Library.action_rescan",
        "Library.action_load_more",
        "Library.action_clear_filters",
        "Library.filter.all",
        "Library.filter.reading",
        "Library.filter.unread",
        "Library.filter.finished",
        "Library.filter.favorites",
        "Library.sort.title",
        "Library.sort.last_opened",
        "Library.sort.added_at",
        "Library.sort.size",
        "Library.status.unread",
        "Library.status.reading",
        "Library.status.finished",
        "Library.status.unsupported",
        "Library.card.missing",
        "Library.empty_title",
        "Library.empty_subtitle",
        "Library.empty_filtered_title",
        "Library.empty_filtered_subtitle",
        "Library.scan_failed",
        "Library.import.dialog_title",
        "Library.import.filter_books",
        "Library.import.filter_all",
        "Library.import.done",
        "Library.import.failed_item",
        "Library.error.not_a_file",
        "Library.error.unsupported_format",
        "Library.error.book_not_found",
        "Library.error.format_mismatch",
        "Library.error.already_on_shelf",
        "Library.error.load_failed",
        "Library.error.corrupt_backup",
        "Library.error.backup_failed",
        "Library.error.future_version",
        "Library.error.save_refused",
        "Library.error.save_refused_after_load_failure",
        "Library.error.create_dir_failed",
        "Library.error.write_failed",
        "Library.info.title",
        "Library.info.unknown",
        "Library.info.format",
        "Library.info.path",
        "Library.info.size",
        "Library.info.chapters",
        "Library.info.added",
        "Library.info.progress",
        "Library.info.bookmarks",
        "Library.edit.title",
        "Library.edit.title_placeholder",
        "Library.edit.author_placeholder",
        "Library.edit.pick_cover",
        "Library.edit.remove_cover",
        "Library.edit.no_cover",
        "Library.edit.cover_filter",
        "Library.edit.save",
        "Library.edit.cancel",
        "Library.actions.title",
        "Library.actions.continue",
        "Library.actions.info",
        "Library.actions.edit",
        "Library.actions.relink",
        "Library.actions.reveal",
        "Library.actions.open_external",
        "Library.actions.hide",
        "Library.actions.delete_file",
        "Library.actions.restart",
        "Library.hide_dialog.title",
        "Library.hide_dialog.message",
        "Library.hide_dialog.confirm_button",
        "Library.delete_file_dialog.title",
        "Library.delete_file_dialog.message",
        "Library.delete_file_dialog.confirm_button",
        "Library.dialog.cancel_button",
        "Library.relink.dialog_title",
        "Library.delete.failed",
        "Library.time.unknown",
        "Library.time.invalid",
        "Library.time.format_failed",
    ];

    const READER_KEYS: &[&str] = &[
        "Reader.action.back",
        "Reader.action.toc",
        "Reader.action.search",
        "Reader.action.bookmarks",
        "Reader.action.more",
        "Reader.action.immersive",
        "Reader.action.exit_immersive",
        "Reader.action.close",
        "Reader.action.prev_chapter",
        "Reader.action.next_chapter",
        "Reader.action.prev_page",
        "Reader.action.next_page",
        "Reader.mode.scroll",
        "Reader.mode.page",
        "Reader.chapter_indicator",
        "Reader.page_indicator",
        "Reader.loading",
        "Reader.error.io",
        "Reader.error.missing",
        "Reader.error.unsupported",
        "Reader.error.decode",
        "Reader.error.encoding",
        "Reader.error.epub",
        "Reader.error.no_content",
        "Reader.error.layout_fallback",
        "Reader.error.action.relink",
        "Reader.error.action.open_external",
        "Reader.error.action.back",
        "Reader.toc.title",
        "Reader.toc.empty",
        "Reader.toc.filter_placeholder",
        "Reader.toc.chapter_count",
        "Reader.typography.title",
        "Reader.typography.font_size",
        "Reader.typography.line_height",
        "Reader.typography.paragraph_spacing",
        "Reader.typography.page_width",
        "Reader.typography.indent",
        "Reader.typography.theme.app",
        "Reader.typography.theme.paper",
        "Reader.typography.theme.sepia",
        "Reader.typography.theme.green",
        "Reader.typography.theme.night",
        "Reader.search.title",
        "Reader.search.placeholder",
        "Reader.search.hint",
        "Reader.search.scanning",
        "Reader.search.results",
        "Reader.bookmarks.title",
        "Reader.bookmarks.add",
        "Reader.bookmarks.chapter",
        "Reader.bookmarks.delete",
        "Reader.bookmarks.duplicate",
        "Reader.bookmarks.empty",
        "Reader.bookmarks.unknown_time",
        "Reader.bookmarks.invalid_time",
        "Reader.bookmarks.time_failed",
        "Reader.info.title",
        "Reader.info.close",
        "Reader.info.author",
        "Reader.info.intro",
        "Reader.info.format",
        "Reader.info.size",
        "Reader.info.chapters",
        "Reader.info.progress",
        "Reader.info.bookmarks",
        "Reader.info.added",
        "Reader.info.last_opened",
        "Reader.info.path",
        "Reader.info.reveal",
        "Reader.info.open",
        "Reader.info.unknown",
    ];

    #[test]
    fn library_translated_in_all_three_locales() {
        for &key in LIBRARY_KEYS {
            for locale in ["en", "zh-CN", "zh-TW"] {
                let v = rust_i18n::t!(key, locale = locale);
                assert!(!v.is_empty(), "{key} 在 locale={locale} 翻译为空字符串");
                assert_ne!(
                    v.as_ref(),
                    key,
                    "{key} 在 locale={locale} 缺失（返回 key 本身）"
                );
            }
        }
    }

    #[test]
    fn reader_translated_in_all_three_locales() {
        for &key in READER_KEYS {
            for locale in ["en", "zh-CN", "zh-TW"] {
                let v = rust_i18n::t!(key, locale = locale);
                assert!(!v.is_empty(), "{key} 在 locale={locale} 翻译为空字符串");
                assert_ne!(
                    v.as_ref(),
                    key,
                    "{key} 在 locale={locale} 缺失（返回 key 本身）"
                );
            }
        }
    }
```

- [ ] **Step 3: 核对没有漏掉任何 key**

Run:

```bash
grep -rhoE 't!\("(Library|Reader)\.[A-Za-z0-9_.]+"' src | sed 's/.*t!("//; s/"$//' | sort -u
```

Expected: 打印出的每一行都必须能在上一步的 `LIBRARY_KEYS` / `READER_KEYS` 里找到
（`Library.time.*` 由 `utils/formatting` 的测试引用，也算在内）。清单之外若还有新 key，
回到 Step 1、Step 2 补上 —— 否则运行时界面上会直接显示原始 key。

- [ ] **Step 4: README**

`README.md`：

1) 「✨ 功能」表里，把「📚 本地书库」那一行替换为三行：

```markdown
| 📚 **本地书库** | 扫描下载目录 + 手动导入，真实封面 / 元数据自动提取、搜索筛选排序、阅读进度、收藏、重新定位、移出与删文件 |
| 📖 **内置阅读器** | TXT / EPUB 应用内阅读，滚动 + 翻页双模式、目录抽屉、字号 / 行高 / 段距 / 页宽 / 主题即时调整、书内搜索、书签、沉浸全屏 |
| 🗂 **书籍管理** | 卡片书架（封面 / 作者 / 进度 / 状态徽标）、手动编辑书名作者封面、缺失文件标记 |
```

2) 「📂 项目结构」的 `src/` 树里，在 `desktop/` 那行之后加两行：

```
    ├── library/           # 本地书库数据层（扫描 / 元数据 / 封面 / 进度 / 书签，与 GUI 解耦）
    ├── reader/            # 阅读数据层（txt / epub → 章节段落、分页算法，与 GUI 解耦）
```

并把 `desktop/` 那行改成：

```
    ├── desktop/           # GPUI 桌面 GUI（components / model / pages / themes/；pages/reader 是覆盖式阅读器）
```

3) 「⌨️ 快捷键」表里，在 `Escape` 那一行之后加：

```markdown
| `T` / `S` / `B` | 阅读器内开关目录 / 搜索 / 书签抽屉 |
| `M` | 阅读器内切换滚动 / 翻页模式 |
| `←` `→` `PgUp` `PgDn` / `Space` | 阅读器翻页 |
| `F11` | 阅读器沉浸全屏（鼠标移到窗口顶部唤出工具栏） |
```

- [ ] **Step 5: 全量验证**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿，尤其新增的两个 locale 覆盖率测试通过。

- [ ] **Step 6: 端到端验收（GUI）**

Run: `cargo run`

1. 搜索一本书 → 下载 → 切到「本地书库」→ 自动出现卡片（封面、作者、下载时写入的进度 0%）。
2. 书库页：搜索框按书名 / 作者过滤、五种筛选 chip、排序按钮循环切换、方向键浏览列表、`Enter` 打开阅读器。
3. 阅读器：目录跳章、翻页模式从当前章连续排到下一章、字号 / 行高 / 段距 / 页宽调整后重启仍保留、主题五种都能正确上色。
4. 书内搜索找一个常用词 → 命中高亮 → 点结果跳转 → 再点别处高亮跟随。
5. 书签：添加 → 关闭阅读器 → 重开同一本 → 书签仍在、点击能跳回原位置；删除不触发跳转。
6. 沉浸：`F11` 全屏、顶部 8px 唤出工具栏、`Esc` 逐层退出，最后回到窗口化且标题栏恢复。
7. 元数据：手动改书名 / 作者 / 封面 → 重新扫描后不被覆盖；「从头开始」把进度清零。
8. 把书文件改个名 → 重新扫描 → 卡片标「文件已不存在」；「重新定位」指回文件后恢复正常。
9. 切换到 English / 繁體中文 → 书库与阅读器文案全部翻译，界面里**不应**出现 `Library.` / `Reader.` 开头的原始 key。

- [ ] **Step 7: 提交**

```bash
git add locales/app.yml src/i18n.rs README.md
git commit -m "docs(i18n): 本地书库与阅读器三语文案、README 与验收清单"
```

---

### Task 24: 阅读失败时的恢复操作

对应 spec §4.10：打开失败不能只甩一行字 —— 文件丢了要能重新定位，解码失败要能换编码重试，
其余错误至少能「用系统程序打开」或「返回书架」。

**Files:**
- Modify: `src/library/store.rs`（`set_encoding`）
- Modify: `src/desktop/model/library.rs`（`set_book_encoding`）
- Modify: `src/desktop/pages/reader/reader_page.rs`

**Interfaces:**
- Consumes: `LibraryStore::relink`、`AppModel::{relink_book, push_error, book_snapshot}`、`ReaderPage::{spawn_load, cancel_search}`、`rfd::AsyncFileDialog`、`system_open::open_with_default_app`。
- Produces: `LibraryStore::set_encoding(&mut self, id: &str, encoding: Option<String>)`、`AppModel::set_book_encoding(&mut self, id: &str, encoding: Option<String>)`、`ReaderPage::{pick_relink, apply_relink, retry_with_encoding, reload_from_disk}`。i18n key `Reader.error.action.*` 已在 Task 23 写进 `app.yml` 与 `READER_KEYS`。

- [ ] **Step 1: store 与 model**

在 `src/library/store.rs` 的 `impl LibraryStore` 内、`set_cover_file` 之后追加：

```rust
    /// 覆盖 TXT 编码（阅读器里「换编码重试」写回的结果）。
    pub fn set_encoding(&mut self, id: &str, encoding: Option<String>) {
        if let Some(book) = self.books.iter_mut().find(|book| book.id == id) {
            book.encoding = encoding;
            self.dirty = true;
        }
    }
```

在 `src/desktop/model/library.rs` 的 `impl AppModel` 内、`update_book_metadata` 之后追加：

```rust
    /// 记录用户在阅读器里手动指定的编码，下次打开不再重新探测。
    pub fn set_book_encoding(&mut self, id: &str, encoding: Option<String>) {
        self.library.store.set_encoding(id, encoding);
        self.library.bump();
        self.persist_library();
    }
```

- [ ] **Step 2: 阅读器的重载与恢复**

`src/desktop/pages/reader/reader_page.rs`：

1) `use` 段补三处：

```rust
use std::path::Path;
```
```rust
use crate::desktop::system_open;
```
```rust
use crate::library::BookFormat;
```

（`use crate::library::{Book, ReadingMode};` 若已存在，合并成
`use crate::library::{Book, BookFormat, ReadingMode};`。）

2) 模块级常量放在 `COLUMN_PADDING` 旁边：

```rust
/// 解码失败时的候选编码（`encoding_rs` 认识这些标签）。
const ENCODING_CHOICES: &[&str] =
    &["UTF-8", "GB18030", "GBK", "Big5", "UTF-16LE", "UTF-16BE"];
```

3) `impl ReaderPage` 里、`spawn_load` 之后追加四个方法：

```rust
    /// 清掉旧的正文 / 分页 / 搜索状态，从磁盘重新加载。
    fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        self.cancel_search();
        self.search = SearchPanel::new();
        self.content = None;
        self.page_cache = None;
        self.toc_titles = None;
        self.chapter = 0;
        self.chapter_ratio = 0.0;
        self.page_index = 0;
        self.page_turn_to_last = false;
        self.restore_pending = true;
        self.load_state = LoadState::Loading;
        self.generation = self.generation.wrapping_add(1);
        self.spawn_load(cx);
        cx.notify();
    }

    /// 换一种编码重新解码：写回 store 后再走一遍加载。
    fn retry_with_encoding(&mut self, encoding: &'static str, cx: &mut Context<Self>) {
        let book_id = self.book_id.clone();
        let value = encoding.to_string();
        self.model.update(cx, |model, _cx| {
            model.set_book_encoding(&book_id, Some(value));
        });
        self.book.encoding = Some(encoding.to_string());
        self.reload_from_disk(cx);
    }

    /// 文件丢了：选一个新路径，选完交给 `apply_relink`。
    pub(super) fn pick_relink(&self, cx: &mut Context<Self>) {
        let format = self.book.format;
        let book_id = self.book_id.clone();
        let page = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            let file = rfd::AsyncFileDialog::new()
                .add_filter(format.label(), &[extension_of(format)])
                .set_title(t!("Reader.error.action.relink").as_ref())
                .pick_file()
                .await;
            let Some(file) = file else {
                return;
            };
            let path = file.path().to_path_buf();
            let _ = page.update(async_cx, |page, cx| {
                page.apply_relink(&book_id, &path, cx);
            });
        })
        .detach();
    }

    /// 重新定位：换 ID / 路径后按新书记录重载。
    fn apply_relink(&mut self, book_id: &str, path: &Path, cx: &mut Context<Self>) {
        let result = self
            .model
            .update(cx, |model, _cx| model.relink_book(book_id, path));
        match result {
            Ok(new_id) => {
                let snapshot = self.model.read(cx).book_snapshot(&new_id);
                if let Some(book) = snapshot {
                    self.book_id = new_id;
                    self.book = book;
                }
                self.reload_from_disk(cx);
            }
            Err(error) => {
                let message = error.message();
                self.model
                    .update(cx, |model, _cx| model.push_error(message));
                cx.notify();
            }
        }
    }
```

4) 文件末尾（`extension_of` 之外的其它自由函数旁边）加：

```rust
/// rfd 过滤器用的扩展名（无点、小写）。
fn extension_of(format: BookFormat) -> &'static str {
    match format {
        BookFormat::Txt => "txt",
        BookFormat::Epub => "epub",
        BookFormat::Markdown => "md",
        BookFormat::Html => "html",
        BookFormat::Zip => "zip",
        BookFormat::Pdf => "pdf",
    }
}
```

- [ ] **Step 3: 错误面板加按钮**

把 `render_error` 整体替换为：

```rust
    /// 出错时的正文区：原因 + 该错误支持的操作按钮。
    fn render_error(&self, error: &ReaderError, cx: &mut Context<Self>) -> impl IntoElement {
        let close_handler = self.close_handler.clone();
        let mut actions: Vec<AnyElement> = Vec::new();

        if matches!(error, ReaderError::MissingFile) {
            actions.push(
                Button::new("reader-error-relink")
                    .primary()
                    .compact()
                    .label(t!("Reader.error.action.relink"))
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.pick_relink(cx);
                    }))
                    .into_any_element(),
            );
        }

        if matches!(
            error,
            ReaderError::Decode(..) | ReaderError::InvalidEncoding(_)
        ) {
            for label in ENCODING_CHOICES {
                actions.push(
                    Button::new(format!("reader-encoding-{label}"))
                        .ghost()
                        .compact()
                        .label(*label)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.retry_with_encoding(label, cx);
                        }))
                        .into_any_element(),
                );
            }
        }

        if matches!(
            error,
            ReaderError::Epub(_)
                | ReaderError::UnsupportedFormat(_)
                | ReaderError::NoContent
                | ReaderError::Io(_)
        ) {
            let path = self.book.path.clone();
            actions.push(
                Button::new("reader-error-external")
                    .ghost()
                    .compact()
                    .label(t!("Reader.error.action.open_external"))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        if let Err(message) =
                            system_open::open_with_default_app(&PathBuf::from(&path))
                        {
                            this.model.update(cx, |model, _cx| model.push_error(message));
                        }
                    }))
                    .into_any_element(),
            );
        }

        actions.push(
            Button::new("reader-error-back")
                .ghost()
                .compact()
                .label(t!("Reader.error.action.back"))
                .on_click(move |_event, window, cx| {
                    if let Some(handler) = &close_handler {
                        handler(window, cx);
                    }
                })
                .into_any_element(),
        );

        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .p_8()
            .child(div().text_lg().child(t!(error.message_key()).to_string()))
            .child(div().text_sm().child(error.detail()))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .children(actions),
            )
    }
```

调用点（`render()` 里的 `LoadState::Error(error) => ...`）改成：

```rust
            LoadState::Error(error) => self.render_error(error, cx).into_any_element(),
```

- [ ] **Step 4: 编译并跑测试**

Run:

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: 全绿。

- [ ] **Step 5: 手工验收（GUI）**

先准备素材：用记事本另存一本 GBK 编码的 `.txt`（内容随便，带几章），放进下载目录。

Run: `cargo run`

1. 打开一本正常的书 → 关闭 → 把文件拖到别处 → 回到书架刷新 → 卡片标「文件已不存在」→
   点「继续阅读」→ 阅读器错误面板只有「重新定位」「返回书架」两个按钮。
2. 点「重新定位」→ 选一个同格式文件 → 不要重开阅读器，正文直接加载出来，进度已落在新书上。
3. 把 GBK 文件改名成 `.txt` 但内容是真的 GBK → 打开 → 若自动探测失败，错误面板出现
   `UTF-8 / GB18030 / GBK / Big5 / UTF-16LE / UTF-16BE` 一排按钮；点 `GBK` → 正文立刻正常显示。
4. 关闭再打开这本书 → 不再报解码错（编码已写进 `library.json` 的 `encoding` 字段）。
5. 打开一个 PDF（先放进下载目录）→ 错误面板显示「该格式暂不支持应用内阅读」+「用系统程序打开」「返回书架」；
   点「用系统程序打开」→ 系统默认阅读器打开它。
6. 面板上的「返回书架」能正常关闭阅读器，窗口装饰 / 沉浸全屏状态都恢复。

- [ ] **Step 6: 提交**

```bash
git add src/library/store.rs src/desktop/model/library.rs src/desktop/pages/reader
git commit -m "feat(reader): 打开失败时的重新定位与换编码重试"
```

---

