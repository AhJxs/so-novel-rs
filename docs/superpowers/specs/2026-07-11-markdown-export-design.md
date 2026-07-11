# Markdown 导出格式支持

- Date: 2026-07-11
- Status: Approved
- Scope: 导出层加 Markdown（`.md`）格式。触达 `export::md` 新模块、`ExportFormat` 枚举、桌面 + Web + CLI 三处 UI 选项。
- Related:
  - 现有导出器：`src/export/{txt,html,epub,pdf}.rs`、`src/export/render.rs`、`src/export/exporter.rs`
  - 枚举定义：`src/config/types.rs:10` `ExportFormat`
  - 桌面 UI：`src/desktop/pages/settings/page_general.rs:71-77` `ext_options`、`src/desktop/pages/settings/fields.rs:216-233` `ext_value` / `ext_from_str`
  - Web UI：`web-ui/src/routes/settings.tsx:33` `FORMAT_OPTIONS`、`web-ui/src/lib/types.ts:115` `ExportFormat`
  - CLI：`src/cli/args.rs:127` `format` 参数

## 背景

so-novel-rs 已支持 4 种导出格式（EPUB / TXT / HTML / PDF）。用户希望新增 Markdown：

- 用例：把小说导入 Obsidian、Logseq、Hugo 站，或纯文本版本控制。
- Markdown 自身为纯文本，逻辑最贴近 TXT，但需要章节锚点 TOC 和 YAML front matter 才能被笔记/静态站点生态解析。

## 目标

新增 `ExportFormat::Markdown`：

- 输出单文件 `<书名>(<作者>).md`，UTF-8。
- 文件头带 Hugo/Jekyll 风格 YAML front matter（title / author）。
- 顶部生成章节锚点 TOC（每行 `- [标题](#chapter-N)`）。
- 每章用 `## 标题` + 段落（段落间双换行）。
- 桌面、Web、CLI 三处下拉/参数都接受 `markdown`。

不在范围：封面嵌入、多文件（按章拆分）输出、引入 `pulldown-cmark` 等新依赖、`txt_encoding` UI 行为调整。

## 方案（已选）：仿 TXT 同形态，新文件 `md.rs`

复用现有 `render → write_chapter_files → Exporter::merge` 三段式管线。新增一个 `RenderTarget` 变体 + 一个 `Exporter` 实现 + 一处枚举扩展 + 三处 UI 选项。其它格式零侵入。

### 1) 枚举与配置层

`src/config/types.rs:10` `ExportFormat`：

```rust
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
    #[default]
    Epub,
    Txt,
    Html,
    Pdf,
    Markdown,  // 新增；保持 Epub 默认
}
```

`as_lower()` / `parse()` 各加 `"markdown"` 分支。`parse` 回落仍是 `Epub`。

### 2) 渲染层（`src/export/render.rs`）

- `RenderTarget` 增加 `Markdown` 变体。
- `render_chapter` 的 `match target` 加 `Markdown => render_md(&filtered.title, &formatted_html)`。
- `render_md(title, p_html)`：
  - 输出 `## {title}\n\n`（章节级别用二级标题；front matter 之外的"书标题"由 merge 阶段单独处理）。
  - 用 `(?s)<p>(.*?)</p>` 抽段，每段 `trim()` 后追加 `\n\n`；空段跳过。
  - 无 `<p>` 兜底：把整段 `p_html.trim()` 当一段（与 `render_txt` 同行为）。
  - 末尾保留一个尾随 `\n`（与 TXT 一致）。
- 简繁转换走 `convert_text`（TXT 同路径），新增 `RenderTarget::Markdown => convert_text(&body, ...)` 分支。
- 新增 `From<ExportFormat> for RenderTarget` 的 `Markdown => Markdown` 映射。

### 3) 章节缓存层（`src/export/exporter.rs`）

- `write_chapter_files` 与 `write_single_chapter` 的 `match format` 加：

  ```rust
  ExportFormat::Markdown => format!("{order}_{safe_title}.md"),
  ```

- 不引入新工具（`sanitize_filename` / `unique_path` 已够用）。
- 章节 `.md` 文件内容就是 `render_md` 的输出，单章独立可读。

### 4) Exporter 实现（`src/export/md.rs`，新文件）

```rust
//! Markdown 导出。对应单文件 .md 合并。
//!
//! 与 TXT 同形态：合并 `chapters_dir` 下每章 .md → 单文件。
//! 多出两点：
//! - YAML front matter（Hugo/Jekyll 风格）
//! - 章节锚点 TOC（`- [标题](#chapter-N)`）

use std::path::{Path, PathBuf};

use crate::export::exporter::{
    ExportError, Exporter, sort_chapter_files, strip_html_tags, unique_path,
};
use crate::models::Book;
use crate::utils::fs::sanitize_filename;

pub struct MdExporter;

impl Exporter for MdExporter {
    fn ext(&self) -> &'static str { "md" }

    fn merge(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        let files: Vec<PathBuf> = sort_chapter_files(chapters_dir)?
            .into_iter()
            .filter(|p| {
                p.file_name().and_then(|s| s.to_str())
                    .is_some_and(|s| !s.starts_with("0_"))
            })
            .filter(|p| {
                p.extension().and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            })
            .collect();
        if files.is_empty() {
            return Err(ExportError::EmptyChaptersDir(chapters_dir.to_path_buf()));
        }

        std::fs::create_dir_all(out_dir)?;
        let out_name = sanitize_filename(&format!("{}({}).md", book.book_name, book.author));
        let out_path = unique_path(out_dir, &out_name);

        let mut out = String::new();
        // 1) YAML front matter
        out.push_str("---\n");
        out.push_str(&format!("title: {}\n", book.book_name));
        out.push_str(&format!("author: {}\n", book.author));
        if let Some(intro) = book.intro.as_deref() {
            let cleaned = strip_html_tags(intro);
            if !cleaned.is_empty() {
                // 多行字段用 | 块标量（literal block scalar）保留换行。
                out.push_str("description: |\n");
                for line in cleaned.lines() {
                    out.push_str(&format!("  {line}\n"));
                }
            }
        }
        out.push_str("---\n\n");

        // 2) 顶部 H1
        out.push_str(&format!("# {}\n\n", book.book_name));

        // 3) 章节锚点 TOC
        out.push_str("## 目录\n\n");
        for (idx, path) in files.iter().enumerate() {
            let title = chapter_title_from_path(path);
            out.push_str(&format!("- [{title}](#chapter-{})\n", idx + 1));
        }
        out.push('\n');

        // 4) 每章正文
        for (idx, path) in files.iter().enumerate() {
            let title = chapter_title_from_path(path);
            let body = std::fs::read_to_string(path)?;
            // 章节渲染时已自带 `## 标题` 行；为保证锚点有效，正文里追加一个
            // 不可见的 `<a id="chapter-N"></a>` HTML 锚点（CommonMark + GFM 兼容）。
            out.push_str(&format!(
                "<a id=\"chapter-{}\"></a>\n\n{}\n\n",
                idx + 1,
                body.trim_end()
            ));
        }

        std::fs::write(&out_path, out)?;
        Ok(out_path)
    }
}

/// 从 `001_第1章 起航.md` 抽出 `第1章 起航`。
fn chapter_title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.split_once('_').map(|(_, t)| t.to_string()).unwrap_or_else(|| s.to_string()))
        .unwrap_or_default()
}
```

要点：

- 锚点方案：标题里可能有中文 / 标点，slugify 不可靠；用 `chapter-N` 序号 + 内联 `<a id="...">` 是 markdown 阅读器（VSCode、Typora、Obsidian、Hugo）兼容性最稳的方案。
- TOC 标题来源：`001_第1章 起航.md` 的 stem 去掉 `001_` 前缀。`sanitize_filename` 已把 `/ \ : * ? " < > |` 替换成 `_`，回看时无需再清洗。
- 不调用 `merge_with_cover`：与 `HtmlExporter` 同。
- 复用 `strip_html_tags`：与 TXT 的 intro 处理一致。

### 5) 工厂与导出目录

- `src/export/exporter.rs:72` `exporter_for`：

  ```rust
  ExportFormat::Markdown => Box::new(super::md::MdExporter),
  ```

- `src/export/mod.rs` 加 `pub mod md;`（无需新增 re-export，复用 `exporter_for` 即可）。

### 6) 桌面 UI

- `src/desktop/pages/settings/fields.rs:216-233`：

  ```rust
  pub(super) const fn ext_value(e: ExportFormat) -> &'static str {
      match e {
          ExportFormat::Epub => "epub",
          ExportFormat::Txt => "txt",
          ExportFormat::Html => "html",
          ExportFormat::Pdf => "pdf",
          ExportFormat::Markdown => "markdown",  // 新增
      }
  }
  ```

  以及 `ext_from_str` 加 `"markdown" => Some(ExportFormat::Markdown)`。

- `src/desktop/pages/settings/page_general.rs:71-77` `ext_options` 追加：

  ```rust
  (ext_value(ExportFormat::Markdown).into(), "md".into()),
  ```

  label 用 `"md"`（与 `txt`/`html`/`pdf` 风格一致：值用扩展名后缀）。

- **txt_encoding 下拉**：md 是 UTF-8-only，但本 PR 不主动禁用控件（避免 UI 抖动），`MdExporter` 路径中不读 `txt_encoding`。Settings 仍保留编码值，下次切回 txt 时仍在。

### 7) Web UI

- `web-ui/src/routes/settings.tsx:33`：

  ```ts
  const FORMAT_OPTIONS: ExportFormat[] = ['epub', 'txt', 'html', 'pdf', 'markdown']
  ```

- `web-ui/src/lib/types.ts:115`：

  ```ts
  export type ExportFormat = 'epub' | 'txt' | 'html' | 'pdf' | 'markdown'
  ```

- `normalizeFormat` 已兼容（小写匹配 + 未知回落 `epub`），无需改。

### 8) CLI

- `src/cli/args.rs:127` `format` 参数 `value_name = "epub|txt|html"` → `"epub|txt|html|pdf|markdown"`。
- 解析走 `ExportFormat::parse`，加 `"markdown"` 分支后 CLI 自动接受 `--format markdown`。

### 9) 测试

#### `src/config/types.rs` `parse` 单测（追加）
- `parse("markdown") -> Markdown`
- `parse("MARKDOWN") -> Markdown`（不区分大小写）
- `parse("xxx") -> Epub`（回落不变）

#### `src/export/render.rs::render_md` 单测（新增）
- 闭合 `<p>`：`<p>a</p><p>b</p>` + 标题 `第1章` → 输出含 `## 第1章`、`a\n\nb`。
- 空 `<p>` 跳过。
- 无 `<p>` 兜底：整段 HTML trim 后作为一段。
- 末尾保留一个 `\n`。
- 简繁转换：源 `zh_CN` + 目标 `zh_TW` → 段落内的 `头发` 转 `頭髮`。

#### `src/export/md.rs::MdExporter` 单测（新增）
- front matter：`---\ntitle: 起航\nauthor: 苹果\ndescription: |\n  ...\n---\n`。
- 空 intro：front matter 不含 `description:`。
- TOC 行数 = 章节数，格式 `- [第1章 楔子](#chapter-1)`。
- 锚点：`body` 含 `<a id="chapter-N"></a>`。
- 跳过 `0_` 前缀与 `非 .md` 文件。
- `EmptyChaptersDir` 错误（无任何 `.md` 章节文件）。
- `unique_path` 同名去重（第二次输出加 ` (1)`）。
- 文件存在性 / 大于 0 字节。

#### `src/export/exporter.rs` 既有测试
不动；若 filename 分支加 `Markdown => .md`，加一个最小测试：`write_chapter_files(chapters, ExportFormat::Markdown)` 写出 `001_起航.md` 类文件。

### 10) 不在范围 / 已知折衷

- **不**嵌入封面（Markdown 没有等价 EPUB 的 `cover.jpg` 概念；Obsidian 用 front matter `cover:` 字段指向外部图片，但本 PR 不实现）。
- **不**生成多文件（按章拆分）输出。用户已选单文件。
- **不**引入新 crate（`pulldown-cmark` / `markdown-rs`）。理由：渲染层已经能从 `<p>` → 段落，HTML→MD 的重型 crate 收益不抵依赖成本。
- **不**支持 `txt_encoding` 切换（md 强制 UTF-8）。Settings 里 encoding 字段保留旧值，切回 txt 时不丢。
- **不**做 slugify；锚点用 `chapter-N`。理由：中文标题 slugify 各家实现差异大；`chapter-N` 唯一且各阅读器都识别 `<a id>`。

## 风险

- **`ExportFormat` 新增变体的兼容性**：`ExportFormat` 已被 serde 序列化进 `config.toml`。新增 `Markdown` 是**纯加项**，老配置（Epub/Txt/Html/Pdf）反序列化仍 OK；新写入的 markdown 配置在老版本反序列化会失败 → 加 fallback：与 `parse` 一致，未知值回落 `Epub`。`toml_io::load_config` 已用 `toml_edit` 解析非 serde 反序列化路径，但若有调用方直接 serde 反序列化 `AppConfig`，需验证。**Mitigation**：实现时跑一遍 `cargo test` + 检查 `src/config/tests.rs`，必要时加迁移。
- **HTML 锚点 `<a id="chapter-N">` 在严格 CommonMark 解析器中可能无效**：GitHub Flavored Markdown / Obsidian / VSCode 均支持；纯 CommonMark 严格模式（`--strict`）会忽略 HTML。本项目用 GFM 兼容阅读器足够。若未来需要纯 CM，可改 `[章节标题](#chapter-N)` 链接形式（不嵌 HTML）。**Mitigation**：暂用 HTML 锚点；在 README/COMMENT 注明。
- **超大文件内存峰值**：单文件写出整本小说 + TOC 进 `String` 一次性 `fs::write`。TXT 走 `BufWriter` 流式编码；md 不需要编码转换，可以流式写。**Mitigation**：实现时改用 `BufWriter` + `write!` 增量写法（性能等价 `String` 拼接前 ~150 MB，单本小说典型 2-10 MB 无忧；超大书仍走流式更稳）。此调整不影响 API 设计。

## 验证清单

- `cargo test --workspace`：所有现有测试不变 + 新增单测通过。
- `cargo build`：桌面 + Web + CLI 三条 target 都能识别 `Markdown`。
- 手动 smoke：CLI `so-novel-rs download <url> --format markdown -o ./tmp` 产出可读 `.md`，front matter、TOC、锚点跳转均工作。
- 桌面 Settings 切换到 markdown，下次下载走 md 路径。
- Web UI 同上。
- 一本含 200+ 章的书，端到端时间与 TXT 接近。

## 触达清单（代码修改点一览）

| 文件 | 改动类型 | 行数估计 |
|---|---|---|
| `src/config/types.rs` | 加变体 + 2 match | +6 |
| `src/export/render.rs` | 加 `RenderTarget::Markdown` + `render_md` + 简繁分支 + 测试 | +80 |
| `src/export/exporter.rs` | 加 `Markdown => .md` filename 分支（×2）+ 1 测试 | +6 |
| `src/export/md.rs`（新） | `MdExporter` 实现 + 测试 | +200 |
| `src/export/mod.rs` | `pub mod md;` | +1 |
| `src/desktop/pages/settings/fields.rs` | `ext_value` + `ext_from_str` 各 1 行 | +2 |
| `src/desktop/pages/settings/page_general.rs` | `ext_options` 追加 1 行 | +1 |
| `src/cli/args.rs` | `value_name` 字符串 | +1 |
| `web-ui/src/routes/settings.tsx` | `FORMAT_OPTIONS` 数组追加 | +1 |
| `web-ui/src/lib/types.ts` | `ExportFormat` type 联合追加 | +1 |
| 合计 | | ~+300 |

i18n 不需要新增 key（label 直接用扩展名 `md`，与 `txt/html/pdf` 一致）。