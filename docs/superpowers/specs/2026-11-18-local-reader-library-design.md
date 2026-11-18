# 本地阅读器与书架改造 — 设计文档

- 日期：2026-11-18
- 状态：已确认（架构 A、首版只读 TXT/EPUB、真书架、扫描 + 引用导入、自动 + 手动元数据、滚动 + 翻页双模式、书内搜索 / 书签 / 进度 / 沉浸模式均获用户批准）

## 背景与目标

当前「本地书库」只是下载目录的文件列表：扫描 `download_path` 的直接子文件，按白名单扩展名过滤，
支持文件名过滤、格式过滤、分页、打开 / 定位 / 删除。它没有书籍元数据、没有阅读入口、没有阅读进度，
用户想看一本书必须交给系统默认程序。

本次改造：

1. 把「书库」升级为**书架**：以书为单位展示封面、书名、作者、格式、阅读进度、收藏状态，支持导入、
   编辑元数据、重新定位失效文件。
2. 新增**应用内阅读器**：TXT / EPUB 可读，具备目录、滚动与翻页两种模式、排版与阅读主题、进度记忆、
   书内搜索、书签、沉浸模式。
3. 新增两个 UI 中立的子系统：`src/library/`（书籍与书架数据）与 `src/reader/`（内容、分页、搜索），
   为后续支持 Markdown / HTML / ZIP / PDF 阅读预留扩展点。

### 非目标（YAGNI）

- 首版不解析 Markdown / HTML / ZIP / PDF 的正文；这些格式仍在书架展示，可打开信息、定位、用系统程序打开，
  卡片提示「暂不支持在应用内阅读」。`BookFormat::is_readable()` 与 `reader::content::load_book` 的
  `match` 是扩展点。
- 不做 TTS、云同步、阅读时长统计、笔记 / 批注、书内书签名分组、标签 / 分组 / 书单。
- 不做文件系统实时 watcher；仍由「进入书架 / 下载目录变化 / 手动刷新」触发扫描。
- 不做 EPUB 内嵌样式 / 图片 / 脚注的还原；正文统一投影为纯文本段落。
- 不做批量导入后的批量编辑；不做拖拽导入。
- 不引入额外字体文件；字体族仅在系统已安装的常见中文字体中挑选，缺失回落系统默认。

## 已确认决策

| 决策点 | 结论 |
|---|---|
| 书库形态 | 真书架：卡片 / 封面网格、元数据、阅读进度、最近阅读、收藏 |
| 书籍来源 | 自动扫描下载目录 + 引用式导入（只记录路径，不复制文件） |
| 元数据 | 自动提取 / 推断 + 允许手动编辑；手动编辑后重新扫描不覆盖 |
| 首版可读格式 | TXT、EPUB；其余格式保留接口 |
| 阅读模式 | 纵向滚动 + 左右翻页，两种都要 |
| 进阶功能 | 书内搜索、书签 + 进度、沉浸 / 全屏；不做实时简繁转换 |
| 架构 | A：新增 `src/library` + `src/reader`，阅读器为覆盖式视图 |

## 一、总体架构

```
┌───────────────────────────── desktop ─────────────────────────────┐
│ RootView                                                           │
│   ├── Shelf 模式：Sidebar + TitleBar + LibraryPage（书架）          │
│   └── Reader 模式：ReaderPage（覆盖主区域）+ 可选 TitleBar          │
│                                                                    │
│ LibraryPage ──► AppModel.library(LibraryState) ──► src/library     │
│ ReaderPage  ──► AppModel（进度/书签落盘）───────► src/library      │
│              └─ 本地状态：内容、分页缓存、搜索、面板 ──► src/reader  │
└────────────────────────────────────────────────────────────────────┘
```

依赖方向严格单向：

- `src/library`：不依赖 GUI，依赖 `config`（只读路径）、`db::atomic_write`、`tools::load::epub`
  （仅复用 EPUB 元数据读取）。
- `src/reader`：不依赖 GUI，依赖 `tools::load`（TXT/EPUB 正文解析）、`encoding_rs`、`regex`。
- `desktop`：只调用上述两层的公开 API；后台任务通过 channel 回送结果。

### 1.1 新增模块

```
src/library/
  mod.rs        # 公开 API 与 re-export
  book.rs       # Book / BookFormat / BookOrigin / ReadingProgress / Bookmark / status
  store.rs      # LibraryStore：加载、保存、合并扫描结果、导入、编辑、移除、失效
  scan.rs       # 目录扫描 → Vec<ScannedFile>（无 UI、无状态）
  meta.rs       # 元数据提取（EPUB OPF / TXT 编码）、封面字节
  cover.rs      # 封面缓存路径、扩展名嗅探、生成封面配色
src/reader/
  mod.rs        # 公开 API
  content.rs    # ReaderBook / ReaderChapter；load_book（TXT/EPUB）+ 格式扩展点
  layout.rs     # ReaderLayout（排版参数）、paginate 纯函数、进度换算
  search.rs     # 书内搜索索引、命中、取消
```

### 1.2 修改模块

| 文件 | 改动 |
|---|---|
| `src/config/types.rs` / `defaults.rs` / `toml_io.rs` | 新增 `[reader]` 配置节与校验 |
| `src/config/paths.rs` | 新增 `library_file`、`covers_dir` |
| `src/lib.rs` | 注册 `library`、`reader` 模块 |
| `src/core/library.rs` | 保留 `SUPPORTED_LIBRARY_EXTS`；移除 `LibraryEntry`（由 `Book` 取代） |
| `src/tools/load/epub.rs` | 拆出 `read_metadata()`（只读 OPF + 封面）供书架复用 |
| `src/desktop/model/library_state.rs` | 重写：`LibraryStore` + 扫描 / 元数据通道 + 筛选排序状态 |
| `src/desktop/model/library.rs` | 重写：扫描、导入、编辑、移除、进度 / 书签、刷新等 `AppModel` 方法 |
| `src/desktop/model/events.rs` / `drain_loop.rs` | 排空书架元数据通道；按时间戳节流进度落盘 |
| `src/desktop/pages/library/*` | 整页重写为书架（卡片网格、工具栏、编辑/信息弹窗） |
| `src/desktop/pages/reader/*` | 新增阅读器页面、正文视图、目录/搜索/书签/排版面板 |
| `src/desktop/root.rs` | 挂载 ReaderPage、隐藏侧栏、TitleBar 随沉浸态显隐、导航旁路 |
| `src/desktop/nav.rs` | 导航文案 key 改为书架；阅读中导航先退出阅读器 |
| `locales/app.yml` | 新增 `Library.*` / `Reader.*` 三语文案 |
| `src/i18n.rs` | 更新 key 完整性测试清单 |
| `README.md` | 功能表与项目结构补阅读器 / 书架 |

## 二、数据模型与持久化

### 2.1 书籍模型（`src/library/book.rs`）

```rust
/// 库内书籍标识与元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Book {
    /// 稳定 ID：`path_key` 的 FNV-1a 64 位哈希（16 hex），用于封面文件名与 UI 键。
    pub id: String,
    /// 规范化查找键：绝对路径，Windows 下转小写；不展示给用户。
    pub path_key: String,
    /// 展示用绝对路径（保留用户原始大小写）。
    pub path: String,
    pub format: BookFormat,
    pub origin: BookOrigin,
    pub title: String,
    pub author: String,
    pub intro: Option<String>,
    /// 封面缓存文件名（相对 `covers_dir`）；`None` = 使用生成封面。
    pub cover_file: Option<String>,
    pub size_bytes: u64,
    pub file_mtime: i64,
    pub added_unix: i64,
    pub last_opened_unix: Option<i64>,
    /// 用户手动改过元数据 → 扫描时不再自动覆盖 title / author / cover。
    pub metadata_edited: bool,
    /// 从书架移除（仅对扫描来源生效，扫描不会再加回）。
    pub hidden: bool,
    /// 文件当前不存在。运行时由扫描更新，持久化便于离线展示。
    pub missing: bool,
    /// TXT 编码覆盖；`None` = 自动探测。
    pub encoding: Option<String>,
    /// 首次成功打开后写入。
    pub chapter_count: Option<u32>,
    pub favorite: bool,
    pub progress: ReadingProgress,
    pub bookmarks: Vec<Bookmark>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookFormat {
    Txt,
    Epub,
    Markdown,
    Html,
    Zip,
    Pdf,
}

impl BookFormat {
    /// `txt` / `epub` / `md|markdown` / `html|htm` / `zip` / `pdf`；未知后缀返回 `None`。
    pub fn from_extension(ext: &str) -> Option<Self>;

    pub const fn is_readable(self) -> bool {
        matches!(self, Self::Txt | Self::Epub)
    }

    /// 用于徽标与文件对话框过滤。
    pub const fn label(self) -> &'static str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BookOrigin {
    /// 下载目录扫描到的文件。
    Downloaded,
    /// 用户从任意位置导入（只记录路径）。
    Imported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadingProgress {
    /// 0-based 章节下标。
    pub chapter: usize,
    /// 章内相对进度 0.0..=1.0：
    /// 滚动模式 = `offset / max_offset`；翻页模式 = `page_index / (page_count - 1)`。
    /// 两种模式都以 1.0 表示本章末尾，切换模式时按此比例定位。
    pub chapter_ratio: f32,
    /// 全书百分比 0..=100；`chapter_count` 未知时为 0。
    pub percent: f32,
    /// 上次阅读模式，重开时优先恢复。
    pub mode: ReadingMode,
    pub updated_unix: i64,
    pub finished: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReadingMode {
    #[default]
    Scroll,
    Page,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    /// `<chapter>-<created_unix_millis>`；同一章同一相对位置（误差 < 0.5%）视为重复。
    pub id: String,
    pub chapter: usize,
    pub chapter_ratio: f32,
    /// 添加时当前段落前 40 个字符。
    pub excerpt: String,
    pub created_unix: i64,
}
```

派生状态（不持久化）：

```rust
pub enum ShelfStatus { Unread, Reading, Finished }
```

`Unread`：`updated_unix == 0`；`Finished`：`progress.finished`；其余 `Reading`。

### 2.2 持久化文件

- 位置：`~/.sonovel/library.json`
- 结构：

```json
{
  "version": 1,
  "books": [ /* Book */ ]
}
```

- 写入：复用 `crate::db::atomic_write`（临时文件 + rename）。
- 读取失败（JSON 损坏 / 权限错误）：把原文件重命名为 `library.json.corrupt-<unix>`，
  以空库启动并推一条错误通知；之后首次保存写新文件，损坏文件保留可人工恢复。
- 保存策略：
  - 结构化变更（导入、移除、删除、编辑、收藏、书签）立即保存；
  - 阅读进度只改内存并标脏，`drain_loop` 每 100 ms 检查，距上次保存 ≥ 5 s 时写盘；
  - 关闭阅读器、退出应用前强制保存。
- 版本字段预留给后续迁移；v1 遇到更高 `version` 时只读加载并在首次保存前提示（不做静默降级）。
- `Book` / `ReadingProgress` / `Bookmark` 的字段一律带 `#[serde(default)]`（含容器级默认），
  后续新增字段时旧 `library.json` 可直接加载。

### 2.3 标识与去重

- `path_key` = `std::fs::canonicalize(path)` 的字符串；失败时退回 `current_dir().join(path)`；
  Windows 下 `.to_lowercase()`。反斜杠保留，仅用于比较。
- `id` = `path_key` 字节的 FNV-1a 64 位哈希，16 位小写 hex。若加载时发现 ID 冲突（极低概率），
  给后者追加 `-1`、`-2`，`path_key` 仍是唯一事实来源。
- 重新定位（换文件）时重新计算 `id` / `path_key`，并把 `covers_dir/<旧 id>.*` 重命名为新 `id`；
  进度与书签原样保留。

### 2.4 阅读设置 `[reader]`

```toml
[reader]
font_size = 19.0          # 16.0..=32.0
line_height = 1.8         # 1.3..=2.4，相对字号倍数
page_width = 720          # 480..=1200，正文列最大宽度 px
paragraph_spacing = 12.0  # 0.0..=32.0 px
indent = true             # 首行缩进两个全角空格
theme = "paper"           # paper | sepia | night | green | app
default_mode = "scroll"   # scroll | page
font_family = ""          # 空 = 系统默认
```

- `config::types::AppConfig` 增加 `reader: ReaderConfig`；
- `defaults.rs` 模板补该节；`toml_io.rs` 读取时缺节用默认值，超范围夹到边界；
- `validate()` 校验同上范围，非法值返回 `ConfigError`；
- 字号等改动由阅读器排版面板即时写入内存配置，面板关闭 / 阅读器退出时保存 `config.toml`。

### 2.5 辅助类型

```rust
/// 书架状态筛选（与 `ShelfStatus` 区分：`All` 不是一种书籍状态）。
pub enum ShelfStatusFilter { All, Unread, Reading, Finished }

/// 书架排序。
pub enum ShelfSort { LastOpened, AddedAt, Title, Size }

/// 后台扫描结果。
pub type ScanEvent = Result<Vec<ScannedFile>, AppError>;

/// 单本书的元数据提取结果。
pub struct MetaEvent {
    pub id: String,
    pub outcome: Result<ExtractedMeta, String>,
}

pub struct ExtractedMeta {
    pub title: Option<String>,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub cover: Option<Vec<u8>>,
    pub encoding: Option<String>,
}

pub struct ScannedFile {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime: i64,
}

pub struct ImportOutcome {
    pub added: usize,
    pub existing: usize,
    /// 失败项及错误文案（打开失败 / 不支持的后缀）。
    pub failed: Vec<(PathBuf, String)>,
}
```

### 2.6 `LibraryStore` 公开 API

```rust
impl LibraryStore {
    pub fn load(path: &Path) -> (Self, Option<String>);      // 失败时返回空库 + 错误文案
    pub fn save(&self, path: &Path) -> Result<(), AppError>;
    pub fn books(&self) -> &[Book];
    pub fn get(&self, id: &str) -> Option<&Book>;
    pub fn merge_scan(&mut self, files: &[ScannedFile]) -> Vec<String>; // 返回需提取元数据的 id
    pub fn import_files(&mut self, paths: &[PathBuf]) -> ImportOutcome; // 新增 / 已存在 / 失败计数
    pub fn apply_meta(&mut self, id: &str, meta: ExtractedMeta, cover_file: Option<String>);
    pub fn mark_missing(&mut self, id: &str, missing: bool);
    pub fn edit_metadata(&mut self, id: &str, title: &str, author: &str, cover_file: Option<String>);
    pub fn set_hidden(&mut self, id: &str, hidden: bool);
    pub fn remove(&mut self, id: &str) -> Option<Book>;
    pub fn relink(&mut self, id: &str, new_path: &Path) -> Result<String, AppError>; // 返回新 id
    pub fn set_favorite(&mut self, id: &str, favorite: bool);
    pub fn record_progress(&mut self, id: &str, chapter: usize, ratio: f32, mode: ReadingMode, finished: bool);
    pub fn add_bookmark(&mut self, id: &str, bookmark: Bookmark) -> bool; // false = 已存在
    pub fn remove_bookmark(&mut self, id: &str, bookmark_id: &str);
    pub fn take_dirty(&mut self) -> bool;
}
```

`LibraryStore` 不启动线程、不持有 channel；扫描与元数据提取由 `desktop/model` 调度。

## 三、书架数据层行为

### 3.1 扫描

输入：下载目录、当前 `LibraryStore`。实现：`scan::scan_download_dir(dir)` 只读直接子文件，
扩展名白名单复用 `core::library::SUPPORTED_LIBRARY_EXTS`，返回 `Vec<ScannedFile { path, size, mtime }>`。
`merge_scan` 同时对 `origin=Imported` 的记录做 `Path::exists()` 检查，用于更新 `missing`；
导入文件不因不在下载目录而被判失效。

合并规则（`LibraryStore::merge_scan`）：

| 情况 | 处理 |
|---|---|
| 新文件 | 新增 `Book`，`origin=Downloaded`，入元数据提取队列 |
| 已存在且 `size` / `mtime` 未变 | 不动，进度与书签保留 |
| 已存在但文件变化 | 更新 `size_bytes` / `file_mtime` / `missing=false`；未手动编辑过则重新提取元数据；保留进度书签 |
| 已存在且 `hidden` | 跳过，不取消隐藏 |
| `origin=Downloaded` 但文件不在扫描结果中 | `missing=true`（记录保留，文件放回后恢复） |
| `origin=Imported` 且文件不存在 | `missing=true`；导入文件不参与下载目录移除 |
| 同一 `path_key` 已存在 | 只刷新，不重复建书，`origin` 保持不变 |

扫描在 `spawn_blocking` 中执行，结果经 smol channel 回送；`scan_in_flight` 防止重复触发。
扫描完成后只对「新增 / 变化」的书触发元数据提取。

### 3.2 元数据提取

单工作线程串行处理（`LibraryState.meta_queue` + `meta_in_flight`），每本书发送一条
`MetaEvent { id, outcome }`，由 `drain_loop` 回写：

- **EPUB**：新增 `tools::load::epub::read_metadata(path) -> Result<EpubMetadata, ToolError>`：
  - 只读 `META-INF/container.xml`、OPF、封面条目；**不解析 spine 正文**；
  - 返回 `title` / `author` / `intro` / `language` / `cover: Option<Vec<u8>>`；
  - 失败不影响书进入书架，保留文件名推断的元数据并记录一次警告。
- **TXT**：`encoding::detect_file_encoding(path)` 探测编码；标题 = 文件名去扩展名；作者留空。
- **其他格式**：标题 = 文件名去扩展名，无封面。
- 写回规则：`metadata_edited == false` 时才覆盖 `title` / `author` / `intro` / `cover_file`；
  `chapter_count` 只由阅读器首次成功打开后写入，扫描不计算。
- 封面字节写入 `~/.sonovel/covers/<id>.<ext>`；扩展名按魔数嗅探（PNG / JPEG / GIF / WebP），
  不认识则丢弃；旧封面文件在替换 / 移除 / 删除时清理。
- 生成封面不落盘：UI 用 `id` 哈希得到色相，`linear_gradient` 两段色 + 书名首字渲染。

### 3.3 导入

- 文件对话框（`rfd::AsyncFileDialog`）多选，过滤 `SUPPORTED_LIBRARY_EXTS`；
- 每个文件计算 `path_key`：已存在 → 取消隐藏、刷新文件信息并保留进度；不存在 → 新增
  `Book { origin: Imported }` 并入元数据队列；
- 完成后 toast：`已导入 N 本，M 本已在书架`；选择多个时逐个处理，失败项汇总提示。

### 3.4 移除 / 删除 / 重新定位

- **移出书架**：`Downloaded` → `hidden=true`；`Imported` → 删除记录。均二次确认，不碰磁盘文件。
- **删除文件**：确认后删除磁盘文件，再删除记录与封面缓存；删除失败保留记录并提示。
- **重新定位**：仅对 `missing` 的书开放；选同格式文件后更新路径 / ID / 封面文件名，
  清 `missing`，保留进度与书签。

## 四、阅读器

### 4.1 内容加载

`src/reader/content.rs`：

```rust
pub struct ReaderBook {
    pub title: String,
    pub author: String,
    pub chapters: Vec<ReaderChapter>,
    pub encoding: Option<String>,
}

pub struct ReaderChapter {
    pub title: String,
    pub paragraphs: Vec<String>,
}

pub fn load_book(path: &Path, format: BookFormat, encoding: Option<&str>) -> Result<ReaderBook, ReaderError>;
```

```rust
pub enum ReaderError {
    UnsupportedFormat(BookFormat),
    MissingFile,
    Io(std::io::Error),
    Decode(BookFormat, String),
    Epub(String),
    NoContent,
    InvalidEncoding(String),
}
```

- 统一走 `tools::load::load_path`（它已按扩展名分派 txt / md / html / zip / epub）；
  入口先按 `BookFormat::is_readable()` 拦截，只放行 TXT / EPUB；
- `encoding` 仅对 TXT 生效：`encoding_rs::Encoding::for_label` 解析，未知标签返回
  `ReaderError::InvalidEncoding`；
- 章节为空 / 无正文 → `ReaderError::NoContent`；
- 加载在 `spawn_blocking` 中执行，完成后回送 UI；`ReaderPage` 用递增的代次 token 丢弃过期结果。
- 打开成功后回写 `chapter_count`、`encoding`（自动探测成功且无手动覆盖时）、`last_opened_unix`。

### 4.2 `ReaderPage` 状态

```rust
pub struct ReaderPage {
    model: Entity<AppModel>,
    book_id: String,
    book: Book,                  // 打开时的快照，关闭时用于落盘
    content: Option<ReaderBook>,
    load_state: LoadState,       // Loading | Ready | Error(ReaderError)
    mode: ReadingMode,
    chapter: usize,
    chapter_ratio: f32,
    // 滚动模式
    scroll_handle: ScrollHandle,
    // 翻页模式
    page_offsets: Vec<Pixels>,
    page_index: usize,
    layout_fingerprint: LayoutFingerprint,
    // 面板
    toc_open: bool,
    search_open: bool,
    bookmarks_open: bool,
    typography_open: bool,
    immersive: bool,
    // 搜索
    search_query: String,
    search_state: InBookSearchState,
    // 待落盘
    progress_dirty: bool,
}

pub enum LoadState {
    Loading,
    Ready,
    Error(ReaderError),
}

pub struct InBookSearchState {
    pub query: String,
    pub results: Vec<SearchGroup>,   // reader::search 的输出，按章分组
    pub scanning: bool,
    pub scanned_chapters: usize,
    pub generation: u64,
}
```

`LayoutFingerprint` 包含：内容宽度、视口高度、字号、行高、段距、缩进、字体族、章节下标。
变化时重算分页。

对外方法：

```rust
impl ReaderPage {
    pub fn is_immersive(&self) -> bool;
    /// 落盘进度、保存阅读设置，然后由 `RootView` 丢弃本实体。
    pub fn close_and_flush(&mut self, cx: &mut Context<Self>);
}
```

### 4.3 版面

- 正文列：`width = min(page_width, 可用宽度 - 2 * 24px)`，居中；上下留白 24–48px。
- 段落渲染：`StyledText`，`line_height = font_size * line_height_factor`，段间距
  `padding_bottom(paragraph_spacing)`（最后一段除外）。
- 首行缩进：渲染前给段落文本前置 `"　　"`；测量使用同一文本；搜索命中区间按前缀的
  UTF-8 字节长度整体偏移（两个全角空格 = 6 字节）。
- 阅读主题由 `[reader].theme` 决定：`paper` / `sepia` / `night` / `green` / `app`。
  `src/reader` 只定义枚举 `ReadingThemeKind`（UI 中立）；具体颜色在
  `desktop/pages/reader/theme.rs` 映射为 gpui `Rgba`，作用于正文、阅读器顶栏与抽屉。
  `app` 表示背景 / 前景沿用应用主题，高亮色取主题的 selection / accent。
- 生成封面的配色同样由 UI 层根据 `library::cover::cover_palette(id)` 返回的
  `(hue, [u8; 3], [u8; 3])` 组装；`src/library` 不依赖 gpui。

### 4.4 滚动模式

- 正文放入自建 `ScrollHandle` 的容器：`div().track_scroll(&handle).overflow_y_scroll()`，
  配 `Scrollbar::vertical(&handle)`；不用 `Scrollable` 包装（需要程序化恢复偏移）。
- 章首 / 章末各一组「上一章 / 下一章」按钮；滚到底不自动跳章。
- `chapter_ratio = offset_y / max(max_offset_y, 1)`；内容布局完成后的下一帧，
  用 `handle.set_offset(point(px(0.), px(ratio * handle.max_offset().y)))` 恢复位置
  （`max_offset()` 在首帧后才有值，所以必须等一帧）。
- 滚动停止（`on_scroll` 后 200 ms 防抖）更新进度。

### 4.5 翻页模式：整章连续排版 + 视口窗口

不切碎正文，只计算「每页从整章的哪个纵向偏移开始」：

1. **测量**（UI 层，`desktop/pages/reader/content_view.rs`）：对每段用
   `Window::text_system().shape_text(text, font_size, &runs, Some(content_width), None)`；
   返回的每个 `WrappedLine` 对应一个源段落，显示行数 = `wrap_boundaries().len() + 1`；
   段落高度 = `lines * line_height + paragraph_spacing`。
   - 搜索高亮只改颜色，不影响行宽，测量时用默认 `TextRun` 即可；
   - 测量结果转成 `ParagraphMetrics`，尺寸统一用 `f32` 像素，UI 层再转 `gpui::Pixels`。
2. **分页**（`src/reader/layout.rs`，纯函数，不依赖 gpui）：

```rust
pub struct ParagraphMetrics {
    pub lines: usize,
    /// 段落之后的间距（最后一段为 0）。
    pub spacing_after: f32,
}

/// 返回每页起始的纵向偏移（首项恒为 0）。
pub fn paginate(
    paragraphs: &[ParagraphMetrics],
    line_height: f32,
    viewport_height: f32,
) -> Vec<f32>;
```

   - 顺序累加 `lines * line_height + spacing_after`；放不下时在**行边界**断开：
     当前页剩余空间可容纳 `k = floor(剩余 / 行高)` 行；
   - `k > 0`：放入 `k` 行，页偏移推进 `k * 行高`，剩余部分从新页开头继续；
     `k == 0`：当前页结束，该段从新页开头开始；
   - 页边界不切断半行；跨页段落的段距只在段落真正结束时计入，页边界处不额外留白；
   - 视口高度不足一行时按 1 行处理，避免死循环。
3. **渲染**：整章段落按顺序布局在内容层，外层 `h(viewport_height).overflow_hidden()`，
   内容层 `relative().top(px(-page_start))`；视觉上只显示当前页。
4. **翻页**：`page_index ± 1`；本章最后一页再向后 → `chapter + 1` 的第 0 页；向前同理。
5. **缓存**：`(chapter, fingerprint) -> page_offsets` 保存当前章一份（滚动模式不需要）；
   窗口 resize / 排版变化 / 切章时失效重算。
6. **进度**：`chapter_ratio = page_index / max(page_count - 1, 1)`，最后一页为 1.0；
   与滚动模式的 `offset / max_offset` 语义一致，模式切换时按比例定位。
7. **兜底**：页偏移越界（≥ 预测内容高度）或分页结果为空时，自动切滚动模式并提示一次
   「排版测量异常，已切换为滚动模式」。debug 构建下首帧渲染后额外对比内容层实际高度与
   预测高度，偏差 > 2% 时 `warn!` 记录（不影响用户，便于实机排查）。

### 4.6 导航与快捷键

阅读器根节点 `key_context("Reader")`，面板输入框聚焦时输入事件归输入框。

| 键 | 翻页模式 | 滚动模式 |
|---|---|---|
| `←` / `PageUp` | 上一页 | 上滚一屏 |
| `→` / `Space` / `PageDown` | 下一页 | 下滚一屏 |
| `↑` / `↓` | 上一页 / 下一页 | 小步滚动 |
| `Home` / `End` | 章首 / 章末 | 章首 / 章末 |
| `T` / `S` / `B` / `M` | 目录 / 搜索 / 书签 / 切换模式 | 同左 |
| `+` / `-` | 字号 +1 / -1 | 同左 |
| `Esc` | 退沉浸 → 再按退出阅读器 | 同左 |

模式切换：`chapter_ratio` 原样保留，滚动模式按新比例设置滚动偏移；翻页模式取最接近该比例的页。

### 4.7 面板

- **目录**：左侧抽屉（约 300px）。`List` 虚拟滚动，当前章高亮，顶部按章节标题过滤，
  点击跳章；顶部显示格式与「共 N 章」。
- **搜索**：右侧抽屉。
  - 输入关键词 → 后台扫描全部章节；每章最多 20 条，总计 1000 条；可取消，展示扫描进度；
  - 匹配为 ASCII 大小写不敏感（`eq_ignore_ascii_case`），中文按字节精确匹配，保留原始字节偏移；
  - 结果按章分组：章标题、命中数、若干条「前后文 + 高亮」；
    点击按 `chapter_ratio = paragraph_index / max(paragraph_count, 1)` 定位到命中段；
  - 当前章所有命中用 `StyledText::with_highlights` 标黄，当前命中用更强的高亮色；
  - 搜索代次 token 丢弃过期结果。

```rust
pub struct SearchGroup {
    pub chapter: usize,
    pub title: String,
    pub hits: Vec<SearchHit>,
}

pub struct SearchHit {
    pub paragraph: usize,
    /// 段落内字节偏移与长度（搜索在原始文本上匹配，不做大小写折叠后的偏移换算）。
    pub byte_offset: usize,
    pub byte_len: usize,
    /// 展示用上下文与其中命中的字节区间。
    pub snippet: String,
    pub snippet_match: std::ops::Range<usize>,
}
```
- **书签**：右侧抽屉。顶部「添加当前书签」；列表按章节升序，显示章节名、摘录、时间；
  点击跳转，删除按钮。同一章同一相对位置（误差 < 0.5%）视为重复，提示「已存在」。
- **排版**：顶栏 `Aa` 弹出面板：主题五选一、字号滑杆、行高滑杆、段距滑杆、页宽滑杆、
  缩进开关、字体族下拉。即时生效，面板关闭时持久化。
- **书籍信息**：顶栏「更多」弹出：书名、作者、简介、格式、大小、路径、添加 / 最近阅读时间；
  按钮「编辑信息」「在文件夹中显示」「用系统程序打开」。

### 4.8 沉浸模式

- 进入：隐藏阅读器顶栏，`window.toggle_fullscreen()`；`RootView` 读取
  `reader.read(cx).is_immersive()` 隐藏 `TitleBar`（侧栏在阅读器模式下本就隐藏）。
- 顶部保留 8px 触发区，鼠标移入浮出顶栏，移出 1.5 s 后收起。
- 退出：`Esc` 或 `F11` → 再次 `toggle_fullscreen()` 恢复装饰；窗口尺寸与当前页不变。
- 沉浸期间快捷键全部保留；退出应用仍走正常确认流程。

### 4.9 进度与完成态

- 位置变化（翻页 / 切章 / 滚动停止）→ 更新内存 `ReadingProgress`，标脏；
- `percent = (chapter + chapter_ratio) / chapter_count * 100`，`finished` 时直接 100；
  `chapter_count` 未知时按 0 处理（首次打开成功后会写入）；
- 每 5 s、切章、关闭阅读器、退出应用时落盘；
- 到达最后一章最后一页（翻页模式）或滚动比例 ≥ 0.99（滚动模式）→ `finished = true`；
- 书架卡片显示「已读完」；「继续阅读」仍停在书末，另有「从头开始」菜单项。

### 4.10 错误处理

| 场景 | 展示 |
|---|---|
| 文件不存在（打开前 / 打开中） | 错误面板：「文件已被移动或删除」，按钮「重新定位」「返回书架」 |
| 解码失败 | 错误面板：「无法按 <编码> 解码」，按钮「指定编码重试」（UTF-8 / GB18030 / GBK / Big5 / UTF-16LE / UTF-16BE）「返回」 |
| EPUB 损坏 / DRM / 无正文 | 错误面板：具体原因 + 「用系统程序打开」「返回」 |
| 扫描失败 | 书架顶部错误条 + 重试按钮 |
| 元数据提取失败 | 不阻塞书架展示；书保持文件名元数据，日志 `warn!` |
| 封面缓存写入失败 | 回落生成封面；日志 `warn!` |
| `library.json` 损坏 | 备份为 `.corrupt-<unix>`，空库启动 + 错误 toast |
| 删除文件失败 | toast 错误；记录保留 |

## 五、书架页面

### 5.1 布局

```
PageHeader「书架」                [导入] [重新扫描]
────────────────────────────────────────────────
[搜索框]  [全部|在读|未读|已读|收藏]      [排序 ▾]
────────────────────────────────────────────────
┌卡片┐ ┌卡片┐ ┌卡片┐ ┌卡片┐ ┌卡片┐
└────┘ └────┘ └────┘ └────┘ └────┘
...
                [加载更多]
```

- `PageHeader` 副标题：`N 本书 · <下载目录>`；扫描中「重新扫描」显示 loading 且禁用。
- 搜索匹配 `title` / `author` / 文件名，实时过滤（大小写不敏感）。
- 状态筛选：全部 / 在读 / 未读 / 已读 / 收藏；与搜索、排序组合。
  「收藏」是独立开关（`filter_favorite=true`，同时状态回到「全部」），其余四个互斥
  写入 `filter_status`。
- 排序：最近阅读（`last_opened_unix` 降序，未打开排后）/ 最近添加 / 书名 / 大小。
- 卡片网格：卡片宽 140px、封面 2:3（140×210px）、间距 20px、`flex_wrap`，
  容器外层纵向滚动（`ScrollHandle`）。
- 每批渲染 60 张，底部「加载更多」；筛选 / 排序变化重置批次；书架为空时不显示。

### 5.2 卡片

- 封面区：
  - 有 `cover_file` → `img(covers_dir/<file>).object_fit(Cover)`，圆角；
  - 无封面 → `linear_gradient` 两段色（`id` 哈希决定色相）+ 书名首字（1–2 字符），
    颜色保证与白色 / 深色文字对比度；
  - 左上格式徽标（TXT / EPUB / …），右上收藏星标（点击切换，`stop_propagation`）；
  - `missing` → 半透明遮罩 + 「文件不存在」。
- 文本区：书名最多 2 行、作者 1 行，均省略号；
- 底部：进度条 + 状态文字（未读 / 32% / 已读完 / 文件不存在）。
- 悬停：边框高亮、封面轻微放大（若 gpui 支持 hover 变换；否则仅边框 + 阴影），
  右下浮出「继续阅读」按钮与「…」菜单。
- 单击卡片：可读 → 打开阅读器；不可读 → 打开书籍信息并提示「暂不支持在应用内阅读」；
  `missing` → 书籍信息（重新定位入口）。
- 右键 / 「…」菜单：阅读、书籍信息、编辑信息、在文件夹中显示、用系统程序打开、
  移出书架、删除文件（危险色）。

### 5.3 弹窗

- **编辑信息**：书名 Input、作者 Input、封面（预览 + 「选择图片」+ 「移除」）、
  「保存」「取消」。保存写入 `metadata_edited=true` 并立即持久化；封面替换时删除旧缓存。
- **书籍信息**：只读信息 + 操作按钮；不可读格式显示「暂不支持在应用内阅读」。
- **移出书架 / 删除文件**：`AlertDialog` 二次确认，文案明确是否删除磁盘文件。
- **重新定位**：`rfd` 选文件，校验扩展名与原格式一致；不一致时提示并拒绝。

### 5.4 加载与空态

- 启动时直接加载 `library.json`，书架立即可见；扫描在后台进行，完成后增量更新。
- 无任何书：`EmptyState` + 「从下载目录扫描」与「导入本地文件」两个按钮。
- 筛选无结果：`EmptyState` + 「清除筛选」。
- 封面图片加载失败：回落生成封面（不显示破图）。

## 六、集成

### 6.1 `RootView`

```rust
pub struct RootView {
    // ...
    reader: Option<Entity<ReaderPage>>,
}
```

- `render()`：`reader` 为 `Some` → 不渲染 Sidebar / 普通页面；`TitleBar` 在
  `!reader.is_immersive()` 时渲染；主区域渲染 ReaderPage。
- `ShowReader` 由书架触发（`RootView::open_reader(book_id, cx)`）。
- 导航 action（`ShowSearch` 等 / `NextPage` / `PrevPage`）：若阅读器打开，先
  `reader.close_and_flush()` 再导航；`ToggleSidebar` 在阅读器打开时忽略。
- `Quit`：先 flush 阅读进度与 `library.json`，再退出。
- 阅读器关闭后重建书架页？不重建：`LibraryPage` 实体常驻，进度变化通过 `books_version`
  失效缓存；`LibraryPage::new` 中 `cx.observe(&model, |_, _, cx| cx.notify())`，
  确保后台扫描 / 元数据 / 进度写回后书架立即重绘。

### 6.2 `AppModel` / `LibraryState`

```rust
pub struct LibraryState {
    pub store: LibraryStore,
    pub books_version: u64,
    pub filter_text: String,
    pub filter_status: Option<ShelfStatusFilter>,
    pub filter_favorite: bool,
    pub sort: ShelfSort,
    pub scan_rx: Option<Receiver<ScanEvent>>,
    pub meta_rx: Option<Receiver<MetaEvent>>,
    pub scan_in_flight: bool,
    pub meta_in_flight: bool,
    pub last_error: Option<String>,
}
```

- `drain_scan` / `drain_meta`：更新 store、`books_version += 1`、按需保存；
- `drain_progress_save`：距上次保存 ≥ 5 s 且有脏数据 → 保存；
- `library.json` 的初始加载在 `AppModel::new_with_wakeup` 中完成（失败走 2.2 的兜底）。

`AppModel` 提供给阅读器的接口：

```rust
impl AppModel {
    pub fn book_snapshot(&self, id: &str) -> Option<Book>;
    pub fn record_reading_progress(&mut self, id: &str, progress: ReadingProgress);
    pub fn set_reading_mode(&mut self, id: &str, mode: ReadingMode);
    pub fn add_reading_bookmark(&mut self, id: &str, bookmark: Bookmark) -> bool;
    pub fn remove_reading_bookmark(&mut self, id: &str, bookmark_id: &str);
    pub fn flush_library(&mut self); // 强制保存 library.json
}
```

进度写入只改内存并标脏；`flush_library` 在关闭阅读器、退出应用时调用。

### 6.3 国际化与可访问性

- 新增 `Library.*`（书架标题、筛选、排序、卡片状态、菜单、弹窗）与 `Reader.*`
  （加载、错误、面板、快捷键提示、主题名）三语 key；
- 更新 `src/i18n.rs` 的 key 完整性测试，确保三语齐备；
- 所有图标按钮设置 `accessibility_label`；卡片设置书名标签；面板可用 `Esc` 关闭。

## 七、性能与风险

### 7.1 性能目标

| 场景 | 目标 |
|---|---|
| 10 MB / 3000 章 TXT 打开 | ≤ 3 s（后台解析，UI 不卡） |
| 全书搜索（3000 章） | ≤ 2 s，可取消 |
| 翻页（已缓存分页） | < 16 ms |
| 书架 200 本书滚动 | 流畅；每批 60 张 |
| 进度保存 | 每 5 s 最多一次，写盘 < 5 ms |

### 7.2 风险与兜底

| 风险 | 兜底 |
|---|---|
| 测量宽度与实际渲染宽度不一致 | 渲染宽度与测量共用同一计算；异常时自动降级滚动模式并提示 |
| CJK 换行与标点规则导致分页偏差 | 测量与渲染使用同一 `TextSystem`；实机验收检查段首标点与行尾 |
| 超长单章整章测量卡顿 | 缓存当前章分页；后续可改为「只渲染当前页附近段落 + 绝对定位」，接口不变 |
| `ScrollHandle` 恢复偏移时机（首帧前） | 在内容就绪后的下一帧设置偏移；比例保存保证即使恢复失败也不丢位置 |
| EPUB 元数据提取与正文解析重复 IO | 元数据只读 OPF；正文仅在打开时读一次；两者都走 `spawn_blocking` |
| 大量封面图片内存 | 封面落盘按需读取，GPUI 纹理缓存管理；卡片分批渲染 |
| 配置节迁移 | 缺 `[reader]` 用默认值，不报错；保存时保留用户注释（`toml_edit`） |

## 八、测试策略

### 8.1 单元测试（无 GUI）

- `library::book`：`path_key` 规范化、ID 稳定性、状态派生、序号 / 百分比换算。
- `library::store`：JSON 往返、损坏文件兜底、保存原子性（临时目录）、扫描合并六种状态、
  手动编辑保护、隐藏 / 失效 / 恢复、导入去重、重新定位迁移、书签去重。
- `library::cover`：PNG/JPEG/GIF/WebP 魔数嗅探、未知格式丢弃、生成封面配色确定性。
- `library::meta`：EPUB fixture 元数据 / 封面提取；TXT 编码探测。
- `reader::content`：UTF-8 / BOM / GBK TXT 编码与章节切分；EPUB fixture 章节顺序；无正文错误。
- `reader::layout`：分页纯函数（空输入、单行、恰好一页、跨页段落、段距、视口不足一行、
  页边界吸附）、位置比例与页号互转。
- `reader::search`：ASCII 大小写、中文命中、每章 / 总数上限、取消、命中的字节偏移。
- `config`：`[reader]` 默认值、越界夹取、校验错误、旧配置缺节加载。

### 8.2 实机验收（Windows）

1. 空目录启动 → 书架空态 → 导入 TXT / EPUB → 卡片出现（EPUB 真实封面、TXT 生成封面）。
2. 打开 UTF-8 TXT、GBK TXT、EPUB；目录跳转；滚动与翻页各截一张图。
3. 调字号 / 行高 / 段距 / 页宽 / 主题 / 缩进；关闭重开，设置与位置均保持。
4. 窗口缩放后翻页无半行截断；测量异常场景自动降级。
5. 书内搜索关键词 → 进度 → 跳转 → 高亮；书签增删跳转；同一位置重复添加提示。
6. 沉浸模式进入 / 退出，窗口装饰与当前页恢复。
7. 外部删除文件 → 失效标记；重新定位后进度保留；应用内删除文件后书架移除。
8. 10 MB / 3000 章大书打开与搜索耗时符合目标。
9. 三语切换：书架与阅读器文案无缺失 key。
10. `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 全绿。

## 九、实施顺序

1. **数据层**：`src/library`、`src/reader`、`[reader]` 配置、路径、单测。
2. **书架 UI**：卡片网格、筛选排序、导入、元数据后台提取、封面缓存、编辑 / 信息 /
   重新定位 / 移出 / 删除弹窗、空态。
3. **阅读器基础**：覆盖式外壳、加载与错误态、滚动模式、目录、排版 / 主题、进度记忆。
4. **阅读器进阶**：翻页模式与分页缓存、书内搜索、书签、沉浸模式、快捷键、完成态。
5. **收尾**：三语文案、README、实机验收截图、全量校验、设计文档与计划归档。

每阶段结束都执行 `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、
`cargo test`，并在 `tasks/todo.md` 记录进度与遗留项。

## 十、验收标准

- 书架能自动收纳下载目录的书，也能引用式导入任意位置的文件；
- EPUB 展示真实书名 / 作者 / 封面，TXT 展示文件名元数据与生成封面；
- TXT（UTF-8 / GBK）与 EPUB 均可在应用内阅读，目录可用；
- 滚动与翻页两种模式均可用，翻页无半行截断，位置在字号 / 窗口变化后不丢；
- 重启应用回到上次章节与相对位置；
- 书内搜索可取消、可跳转、可高亮；书签可增删跳转；沉浸模式可进可退；
- 文件失效可识别并可重新定位，进度不丢；删除文件后书架同步；
- 三语文案完整，`cargo fmt` / `clippy` / `test` 全绿，实机验收通过。
