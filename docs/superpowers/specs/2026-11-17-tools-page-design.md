# 侧边栏「工具」页与小说工具箱 — 设计文档

- 日期：2026-11-17
- 状态：已确认（导出层内存化方案 B、首版三件套两项决策获用户批准）

## 背景与目标

桌面端目前有 5 个页面（搜索 / 任务 / 书库 / 书源 / 设置），全部面向「在线抓取 → 导出」。
导出层（`src/export/`）的能力只能通过下载任务触达：`Exporter::merge` 接收一个**章节目录**，
因此本地已有的 TXT / EPUB 文件无法复用这套渲染与打包逻辑。

目标：

1. 侧边栏新增「工具」页，收纳常用的小说文件工具。
2. 首版提供三件套：**格式转换**、**TXT 编码转换**、**简繁转换**。
3. 导出层重构为「内存章节」接口，下载链路与工具页共用同一套导出器，输出结果保持字节级兼容。
4. 本地文件处理全程不产生中间章节目录（HTML 导出也不再需要先写盘再打包）。

非目标（YAGNI）：批量转换、拖拽导入、保留原 EPUB 样式 / 内嵌图片 / 脚注、封面编辑、转换预览、任务取消。

## 已确认决策

| 决策点 | 结论 |
|---|---|
| 转换内核 | **方案 B**：重构 `Exporter` 接收内存章节，下载与工具共用 |
| 首版范围 | 三件套：格式转换、TXT 编码转换、简繁转换 |
| 输入格式 | `.txt` `.md` `.html` `.htm` `.epub` `.zip`（本应用导出的 HTML 包） |
| 章节切分 | 默认中文标题正则；支持用户自定义正则覆盖 |
| 简繁范围 | 仅 `.txt` / `.md`，全文转换，输出编码与输入一致 |
| 工具页形态 | 左侧工具列表 + 右侧表单，单文件、单任务 |

## 一、导出层重构

### 1.1 新接口

`src/export/exporter.rs`：

```rust
pub trait Exporter {
    fn ext(&self) -> &'static str;

    fn merge(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError>;

    fn merge_with_cover(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
        cover: Option<&[u8]>,
    ) -> Result<PathBuf, ExportError> {
        self.merge(book, chapters, out_dir)
    }
}
```

`RenderedChapter { order, title, body }` 语义调整为「已按目标格式渲染完成的一章」：

- `title` 是权威标题，TXT / Markdown / EPUB 直接使用，不再从文件名反推；
- `body` 是目标格式的正文（TXT 为纯文本、Markdown 为 `##` 开头、HTML/EPUB/PDF 为完整 HTML 文档）。

### 1.2 `load_chapter_files`

新增，集中原先散落在 5 个导出器里的读目录逻辑：

```rust
pub fn load_chapter_files(
    dir: &Path,
    format: ExportFormat,
) -> Result<Vec<RenderedChapter>, ExportError>;
```

行为：

1. 按 `format` 选择扩展名：Txt → `.txt`，Markdown → `.md`，Html / Epub / Pdf → `.html` / `.xhtml`。
2. 跳过文件名以 `0_` 开头的辅助文件（目录、封面）。
3. 按文件名首个 `_` 前的数字升序；无法解析的排最后。
4. 标题来源：
   - Txt / Markdown / Epub：文件名第一个 `_` 之后的部分（去扩展名），空则 `第 N 章`；
   - Html / Pdf：文件名不含标题，从正文提取——优先 `<title>`，其次首个 `<h1>`，再回落 `第 N 章`。
5. 读取失败 → `ExportError::Io`；结果为空 → `ExportError::NoChapters`。

`sort_chapter_files` 保留（测试与潜在外部使用），但导出器不再调用它。

### 1.3 各导出器改动

| 导出器 | 改动 |
|---|---|
| `txt.rs` | 删除读目录与文件名解析；遍历 `chapters`，沿用现有书目头（书名/作者/简介）+ 正文 + 换行规则 |
| `md.rs` | 同上；YAML front matter、`# 书名`、`## 目录`、锚点、正文全部来自 `book` + `chapters` |
| `epub.rs` | 章节正文用 `Cursor<Vec<u8>>` 直接喂给 `EpubContent`，文件名仍为 `{padded}.xhtml`；标题取 `chapter.title`；封面逻辑不变 |
| `html.rs` | 不再扫描目录再 zip：直接以内存章节生成 `NNN_.html` 条目与 `0_目录.txt`，zip 仍为扁平结构、Deflated、权限 `0o644` |
| `pdf/document.rs` | 遍历 `chapters`，`extract_chapter_content` 逻辑不变；标题优先 `<h1>`，缺失时用 `chapter.title` |

输出文件名、目录结构、TXT 编码、EPUB 元数据、PDF 排版常量全部保持不变。

`epub.rs` 的 `merge_with_cover_bytes(book, chapters, out_dir, cover_bytes)` 同步改成内存签名。

### 1.4 错误类型

```rust
pub enum ExportError {
    #[error("没有可导出的章节")]
    NoChapters,
    // Io / Epub / Zip / Encoding / Pdf 不变
}
```

`EmptyChaptersDir(PathBuf)` 删除；5 个导出器测试里的空目录断言改为空切片断言。

### 1.5 下载链路

`src/crawler/download.rs:368-371`：

```rust
let exporter = export::exporter_for(cfg.download.ext_name, &cfg.download.txt_encoding);
let final_path = {
    let chapters = export::load_chapter_files(&chapters_dir, cfg.download.ext_name)?;
    exporter.merge_with_cover(book, &chapters, out_dir, cover_bytes.as_deref())?
};
```

章节仍逐章写盘（断点续传 + `preserve_chapter_cache` 语义不变），合并前一次性读入内存。
内存峰值与现状相同：现有导出器本来就要整本读入。

## 二、业务层 `src/tools/`

### 2.1 模块结构

```
src/tools/
  mod.rs           ToolRequest / run / ToolError 重导出
  source_book.rs   SourceBook / SourceChapter / to_book()
  load/mod.rs      load_path() 按扩展名分派
  load/txt.rs      编码检测 + 解码 + 切分
  load/markdown.rs front matter + ATX 标题
  load/html.rs     单 HTML / 导出 zip
  load/epub.rs     container.xml → OPF → spine
  split.rs         默认正则 + 自定义正则
  render.rs        SourceBook → Vec<RenderedChapter>
  convert.rs       格式转换
  encoding.rs      TXT 转码
  zhconv.rs        简繁转换
```

模块在 `src/lib.rs` 注册 `pub mod tools;`。所有函数同步、无 GPUI 依赖，可直接单测。

### 2.2 数据结构

```rust
pub struct SourceBook {
    pub title: String,
    pub author: Option<String>,
    pub intro: Option<String>,
    pub language: Option<String>,
    pub cover: Option<Vec<u8>>,
    pub chapters: Vec<SourceChapter>,
}

pub struct SourceChapter {
    pub title: String,
    pub paragraphs: Vec<String>, // 已剥离标签、去空行
}
```

`SourceBook::to_book()` 是唯一的映射点：`book_name = title`，`author` 缺省「佚名」，
`language` 缺省 `"zh"`，`cover_url = None`。输出的书名/作者因此与工具页展示一致。

### 2.3 载入

统一入口：

```rust
pub fn load_path(path: &Path, opts: &LoadOptions) -> Result<SourceBook, ToolError>;
```

`LoadOptions` 包含 `source_encoding: Option<&'static Encoding>` 与 `chapter_regex: Option<&str>`。

| 输入 | 处理 |
|---|---|
| TXT | 先看 BOM（`Encoding::for_bom`），否则 `chardetng` 猜测；CRLF/CR → LF；按空行分段落 |
| Markdown | 可选 YAML front matter（手写行解析 `title/author/description`，去引号）；按 ATX 标题（`#`～`######`）切章；去掉标题后的正文做最小内联清理（链接/图片取文字、强调与代码标记） |
| 单 HTML/HTM | `<title>` 作书名，`<meta name="author">` 作作者；有 `<h1>`～`<h3>` 则按标题切章，否则转为纯文本后走默认切分 |
| 导出 ZIP | 读 `0_目录.txt` 建立 `文件名 → 标题` 映射；遍历 `NNN_*.html`，正文取段落；缺目录文件时用 `<title>` |
| EPUB | 读 `META-INF/container.xml` 找 OPF；解析 metadata（title/creator/description/language）、manifest、spine；按 spine 顺序取 XHTML；封面取 `properties="cover-image"` 或 `<meta name="cover">` 指向的资源字节 |

HTML/XHTML → 段落的公共函数放在 `load/html.rs`：块级标签（`p/div/br/li/h1-h6`）转换行，
删 `script/style`，剥标签，解码常见实体（`&amp; &lt; &gt; &quot; &#39; &nbsp;` 与数字实体），
按空行分段。EPUB 的样式、图片、脚注、目录页全部丢弃；仅保留封面字节。

EPUB 含 DRM/加密条目 → `ToolError::EpubStructure`，不尝试解密。

### 2.4 章节切分

`split.rs`：

```rust
pub fn split_paragraphs(
    paragraphs: Vec<String>,
    custom: Option<&str>,
) -> Result<Vec<SourceChapter>, ToolError>;
```

- 逐段判断：段落必须**单行**且整行匹配标题模式才视为章节起点（避免正文里出现「第一章」被误切）。
- 默认正则（编译期常量）：

  ```text
  ^\s*(?:第[0-9零一二三四五六七八九十百千两]+[章节回卷]|序[章言]|楔子|前[言序]|后记|尾声|番外(?:篇)?)(?:[\s:：、.．—-].*)?$
  ```

- 用户正则：`Regex::new(custom)`，非法 → `ToolError::InvalidRegex`；匹配方式同样是「整行匹配」，用户无需写 `(?m)`。
- 首个标题之前的非空段落 → 一章，标题「前言」。
- 全书无标题 → 单章，标题为书名。

### 2.5 渲染

```rust
pub fn render(
    book: &SourceBook,
    chapters: &[SourceChapter],
    target: ExportFormat,
) -> Vec<RenderedChapter>;
```

`order` 从 1 递增。正文按目标格式生成：

| 目标 | `body` |
|---|---|
| Txt | 标题 + 空行 + 每段前置两个全角空格 `\u{3000}\u{3000}`，段间单换行，结尾换行 |
| Markdown | `## {title}` + 空行 + 段落空行分隔 |
| Html / Epub / Pdf | 完整文档：`<!DOCTYPE html><html lang="zh"><head><meta charset="utf-8"><title>{title}</title></head><body><h1>{title}</h1><p>…</p></body></html>` |

HTML 特殊字符在标题与段落中做实体转义。

### 2.6 三个工具

```rust
pub enum ToolRequest {
    Convert {
        source: PathBuf,
        out_dir: PathBuf,
        target: ExportFormat,
        txt_encoding: String,
        source_encoding: Option<String>,
        chapter_regex: Option<String>,
    },
    Encoding {
        source: PathBuf,
        out_dir: PathBuf,
        source_encoding: Option<String>,
        target_encoding: String,
    },
    Zhconv {
        source: PathBuf,
        out_dir: PathBuf,
        target: LangType,
    },
}

pub fn run(req: ToolRequest) -> Result<PathBuf, ToolError>;
```

原设计中的 `ToolJob` 包装层删除，`ToolRequest` 本身即任务描述，避免无意义的一层结构。

三个工具在写产物前都调用 `std::fs::create_dir_all(out_dir)`，输出目录不存在时自动创建。
输入扩展名按下划线转小写后分派（`.TXT` 与 `.txt` 等价）。

**格式转换**：`load_path` → `split_paragraphs` → `render` → `exporter_for(target, txt_encoding)` →
`merge_with_cover(&book, &chapters, out_dir, book.cover.as_deref())`，返回成品路径。

**编码转换**：

1. 读原始字节，BOM 优先，其次显式源编码，最后 `chardetng`。
2. `encoding.decode` 得到文本；`had_errors` 时记录 `tracing::warn!` 并继续（不因个别坏字节失败）。
3. 目标编码用 `encoding_rs` 编码；不可编码字符 → `ToolError::Unencodable`。
4. 输出 `{stem} ({target}){ext}`，由 `unique_path` 处理重名。
5. 目标编码只提供可编码的 5 种：UTF-8 / GBK / GB18030 / Big5 / BIG5HKSCS（`encoding_rs` 的 UTF-16 仅支持解码）。

**简繁转换**：仅 `.txt` / `.md`；TXT 先按上述规则解码，`utils::zhconv::convert_text` 全文转换，
按**输入编码**写回；输出 `{stem} (简体|繁体){ext}`。目标变体：简体 `ZhCn`、繁体（台湾）`ZhTw`、
繁体（通用）`ZhHant`。

### 2.7 错误类型

```rust
pub enum ToolError {
    UnsupportedFormat(String),
    Io(#[from] std::io::Error),
    Decode(String),
    Unencodable(String),
    InvalidRegex(String),
    EpubStructure(String),
    EmptyDocument,
    Export(#[from] ExportError),
}
```

`Display` 走 thiserror；UI 只取 `to_string()` 放进 toast，技术上下文进 `tracing`。

## 三、UI 与导航

### 3.1 导航

- `NavPage` 增至 6 项，顺序：`Search, Tasks, Library, Sources, Tools, Settings`（设置保持最后）。
- 新增 `ShowTools` action，`ALL` 长度 5 → 6。
- `src/desktop/nav.rs:99` 的显式绑定改为 `cmd-5 → ShowTools`、新增 `cmd-6 → ShowSettings`；
  `src/desktop/{mod,nav,root}.rs` 的 action 导入与 `actions.rs` 的 `impl Eq` 同步补齐。
- `src/desktop/root.rs` 模块文档中的 `cmd-1`~`cmd-5` 改为 `cmd-1`~`cmd-6`。
- **行为变更**：`cmd-5` 由「设置」变为「工具」，`cmd-6` 为「设置」。在 PR 说明中注明。
- 图标 `IconName::SlidersHorizontal`（Lucide 集合无扳手/工具箱图标）。
- `src/desktop/nav.rs` 的 `label_key()` 增加 `Nav.tools`；`icon()` 增加分支。
- `src/desktop/root.rs` 增加 `tools_page: Entity<ToolsPage>` 字段、`cx.new` 创建、`render_current_page` 分支、
  `bind_nav_actions` 增加 `ShowTools`。
- `src/desktop/pages/mod.rs` 导出 `ToolsPage`，文档注释 5 → 6。

### 3.2 页面布局

```
PageHeader("工具") + subtitle
┌──────────────┬─────────────────────────────────────┐
│ 格式转换      │  源文件   [路径…]        [选择文件]   │
│ 编码转换      │  输出目录 [路径…]        [更改]      │
│ 简繁转换      │  目标格式 [TXT][EPUB][HTML][PDF][MD] │
│              │  章节正则 [__________________]        │
│              │  [开始转换]                           │
│              │  结果：/path/to/book.epub  [打开所在文件夹] │
└──────────────┴─────────────────────────────────────┘
```

- 外层 `v_flex().size_full().p_6().gap_4()`；右侧表单 `flex_1().min_w_0().overflow_y_scroll()`。
- 左侧每一项用 `Button::new(("tools-tab", i)).ghost().selected(active)`，内容为图标 + 名称 + 一行说明；
  说明用 `text_xs` + `theme.muted_foreground`。
- 字段行统一「标签（宽 96px）+ 控件」；路径用只读 `Input` 或 `div` 展示，超长省略。
- 目标格式 / 目标编码 / 简繁变体用一排 chip `Button`（`ghost().selected()`），不引入 `SelectState`。

### 3.3 状态与交互

```rust
pub struct ToolsPage {
    model: Entity<AppModel>,
    active: ToolKind,
    convert: ConvertForm,
    encoding: EncodingForm,
    zhconv: ZhconvForm,
    regex_input: Entity<InputState>,
}

struct ConvertForm {
    source: Option<PathBuf>,
    out_dir: Option<PathBuf>,       // None = 源文件父目录
    target: ExportFormat,
    txt_encoding: String,           // 初值取 config.download.txt_encoding
    source_encoding: Option<String>,// None = 自动
    detected_encoding: Option<String>,
    regex: String,
    running: bool,
    result: Option<PathBuf>,
    error: Option<String>,
}
```

- 选择文件后：重置结果/错误；`out_dir` 设为父目录；文本类输入在后台探测编码并显示「检测到 GBK」。
- 章节正则输入框仅在源文件为 `.txt/.md/.html/.htm` 时显示；`.epub/.zip` 按结构切章。
- 「开始」在无源文件或 `running` 时禁用；运行中按钮 `loading(true)`。
- 输出目录默认源文件父目录，可点「更改」用 `rfd::AsyncFileDialog::pick_folder` 修改。
- 切换左侧工具不取消正在运行的任务；各表单独立保存 `running/result/error`。

### 3.4 异步与提示

- 文件对话框沿用仓库现有写法：`cx.spawn(async move |_weak, async_cx| { … })` + `AsyncFileDialog`
  （Windows 下同步对话框在工作线程会返回 `None`，必须异步）。
- 转换在后台执行，不阻塞 UI：

  ```rust
  cx.spawn(async move |_this, cx| {
      let result = cx.background_executor().spawn(async move { tools::run(req) }).await;
      handle.update(cx, |page, cx| { /* 写回 result/error; cx.notify() */ });
  }).detach();
  ```

- 成功：表单显示成品路径 + 「打开所在文件夹」（`utils::system::reveal_in_folder`），
  并 `model.push_success(t!("Tools.toast.done", name = …))`。
- 失败：表单显示错误文案，并 `model.push_error(...)`；不 panic。
- 「打开所在文件夹」失败只记 `tracing::warn!`。

### 3.5 i18n

`locales/app.yml` 新增 `Nav.tools` 与 `Tools.*` 命名空间，**三个语言（zh-CN / zh-TW / en）都必须补齐**，
仓库没有 fallback，缺 key 会直接把 key 显示出来。

| key | 说明 |
|---|---|
| `Nav.tools` | 侧边栏标签 |
| `Tools.page_title` / `Tools.page_subtitle` | 页头 |
| `Tools.tab.convert` / `.encoding` / `.zhconv` | 工具名 |
| `Tools.tab.*_desc` | 一行说明 |
| `Tools.field.source` / `.output_dir` / `.target_format` / `.txt_encoding` / `.source_encoding` / `.chapter_regex` / `.encoding_target` / `.variant` | 字段标签 |
| `Tools.action.choose_file` / `.choose_dir` / `.run` / `.reveal` / `.detect` | 按钮 |
| `Tools.status.running` / `.done` / `.detected` | 状态行 |
| `Tools.encoding.auto` | 「自动检测」 |
| `Tools.variant.zh_cn` / `.zh_tw` / `.zh_hant` | 简繁选项 |
| `Tools.error.*` | 与 `ToolError` 变体一一对应的用户文案 |

## 四、测试与验证

### 4.1 单元测试

| 模块 | 用例 |
|---|---|
| `split.rs` | 默认正则识别章/节/回/卷/序章/楔子；首个标题前内容成「前言」；无标题整本一章；自定义正则覆盖；非法正则报错 |
| `load/txt.rs` | UTF-8 BOM、无 BOM 的 GBK 字节、CRLF 归一化 |
| `load/markdown.rs` | front matter 解析、ATX 切章、前置内容 |
| `load/html.rs` | 单 HTML 的 title/author/h1 切章；导出 zip 的 `0_目录.txt` 映射 |
| `load/epub.rs` | 用 `epub-builder` 现场造一本 EPUB，读回标题/作者/章节数/封面字节 |
| `render.rs` | TXT 全角缩进、Markdown `##`、HTML 文档结构与转义 |
| `encoding.rs` | GBK → UTF-8 内容等价；不可编码字符报 `Unencodable` |
| `zhconv.rs` | 简 → 繁文本变化；输出编码保持 |
| 导出器 | 5 个既有测试改为 `write_chapter_files` → `load_chapter_files` → `merge(&book, &chapters, &out)`，断言不变 |

### 4.2 验证门槛

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

手工验收：

1. 下载一本 EPUB 与一本 PDF，输出与改动前一致（对比文件大小/目录结构/TOC）。
2. TXT(GBK) → EPUB：阅读器打开有目录、章节标题正确。
3. TXT → PDF：中文正常渲染（本机有 CJK 字体）。
4. GBK TXT → UTF-8：`iconv`/编辑器打开内容一致。
5. 简 → 繁：抽样段落转换正确，输出文件名带「繁体」。
6. 非法章节正则：toast 报错，应用不崩溃。
7. 侧边栏第 5 项为「工具」，`cmd-5` / `cmd-6` 行为符合 3.1。

## 五、风险与缓解

| 风险 | 缓解 |
|---|---|
| `cmd-5` 行为变更影响老用户肌肉记忆 | PR 说明 + 验收清单注明；设置仍可通过 `cmd-6` 与侧边栏到达 |
| 整本读入内存 | 小说通常 < 20 MB；PDF/EPUB 现有实现本就如比；工具页单文件运行 |
| 中文标题正则误判 | 仅整行匹配；提供自定义正则；无匹配时退化为单章而非丢内容 |
| EPUB 结构差异（DRM、非标准 OPF） | 明确报 `EpubStructure`，不静默产出空书 |
| `encoding_rs` UTF-16 仅能解码 | 目标编码列表排除 UTF-16；源编码列表保留 |
| 输出重名覆盖 | 全部走 `unique_path` 追加 ` (1)` |
| i18n 缺 key | 三语言同步补齐；验收时切换语言检查工具页 |

## 六、实现顺序

1. 导出层重构 + 下载链路 + 5 个导出器测试迁移（先保证回归绿）。
2. `src/tools/` 数据结构、载入器、切分、渲染 + 单测。
3. 三个工具流程 + 单测。
4. 导航与 `ToolsPage` UI + i18n。
5. 手工验收 + `cargo fmt/clippy/test`。

## 七、验收标准

- [ ] 下载 EPUB/PDF/TXT/HTML/Markdown 的输出与改动前一致。
- [ ] 工具页可从侧边栏与 `cmd-5` 进入，设置页仍可从侧边栏与 `cmd-6` 进入。
- [ ] 三件套对各自的输入格式可用，成功/失败均有 toast 与页面内状态。
- [ ] 转换不产生临时章节目录，输出重名不覆盖。
- [ ] 全部用户可见文案三语言齐备。
- [ ] `cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test` 全绿。
