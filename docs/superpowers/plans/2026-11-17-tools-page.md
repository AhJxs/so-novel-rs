# 工具箱页面 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在桌面端侧边栏新增「工具箱」页面，提供本地小说文件的格式转换、TXT 编码转换、简繁转换三项工具。

**Architecture:** 先把导出层从“读章节缓存目录”重构为“接收内存章节”，让下载与工具共用同一套 `Exporter`；再新增 `src/tools/` 业务层，负责把 `.txt/.md/.html/.zip/.epub` 归一化为 `SourceBook`（标题 + 段落），按需切章、渲染、导出；最后接入第 6 个导航页 `ToolsPage`。

**Tech Stack:** Rust 2024 / rust-version 1.95、gpui-kit 0.10.0、tokio（`AppModel.runtime`）、rfd 0.18、encoding_rs 0.8、chardetng 1、epub-builder 0.8、zip 9、rust-i18n 4.2。

**Spec:** `docs/superpowers/specs/2026-11-17-tools-page-design.md`

## Global Constraints

- 不新增第三方依赖；只复用 `Cargo.toml` 已有 crate。
- 所有用户可见文案必须同时提供 `en` / `zh-CN` / `zh-TW` 三种翻译，写在 `locales/app.yml`；缺失 key 会直接显示 key，且 `src/i18n.rs` 有测试遍历 locale。
- 文件/文件夹选择器只能用 `rfd::AsyncFileDialog`（Windows 上同步对话框会静默返回 `None`）。
- CPU 密集的转换必须放在 `cx.background_executor()` 上执行，不能阻塞 UI 线程。
- 页面必须沿用现有约定：`PageHeader`、`v_flex().size_full().p_6().gap_4()`、`Button` + `.label()`/`.icon()`、`ActiveTheme` 的 `cx.theme()`。
- 导航顺序固定为 Search、Tasks、Library、Sources、Tools、Settings；快捷键 `cmd-1`…`cmd-6` 依次对应，`Settings` 从 `cmd-5` 移到 `cmd-6`。
- 每个任务结束前必须通过：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、相关 `cargo test`。
- 提交信息用中文 conventional commits，例如 `feat(tools): ...`、`refactor(export): ...`。

---

### Task 1: 导出层改为内存章节

**Files:**
- Modify: `src/export/exporter.rs:20-76`（`ExportError`、`Exporter`、新增 `load_chapter_files`）
- Modify: `src/export/mod.rs:15-19`（re-export）
- Modify: `src/export/txt.rs:84-136`（merge 签名与实现）
- Modify: `src/export/md.rs:22-95`
- Modify: `src/export/html.rs:29-123`
- Modify: `src/export/epub.rs:55-160`
- Modify: `src/export/pdf/document.rs:26-140`
- Modify: `src/crawler/download.rs:368-371`
- Test: `src/export/exporter.rs:269-459`、`src/export/{txt,md,html,epub}.rs`、`src/export/pdf/document.rs` 的 `#[cfg(test)]`

**Interfaces:**
- Consumes: `Book`、`ExportFormat`、现有 `pad_zero` / `unique_path` / `strip_html_tags`。
- Produces:
  - `ExportError::NoChapters`
  - `fn load_chapter_files(dir: &Path, format: ExportFormat) -> Result<Vec<RenderedChapter>, ExportError>`
  - `trait Exporter::merge(&self, book: &Book, chapters: &[RenderedChapter], out_dir: &Path) -> Result<PathBuf, ExportError>`
  - `trait Exporter::merge_with_cover(&self, book: &Book, chapters: &[RenderedChapter], out_dir: &Path, cover: Option<&[u8]>) -> Result<PathBuf, ExportError>`

- [ ] **Step 1: 先写 `load_chapter_files` 的失败测试**

在 `src/export/exporter.rs` 的 `mod tests` 内追加：

```rust
#[test]
fn load_chapter_files_sorts_and_skips_zero_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    std::fs::write(
        dir.join("002_.html"),
        "<html><head><title>第二章</title></head><body><h1>第二章</h1><p>b</p></body></html>",
    )
    .unwrap();
    std::fs::write(
        dir.join("001_.html"),
        "<html><head><title>第一章</title></head><body><h1>第一章</h1><p>a</p></body></html>",
    )
    .unwrap();
    std::fs::write(dir.join("0_目录.txt"), "toc").unwrap();
    std::fs::write(dir.join("notes.txt"), "ignored").unwrap();

    let chapters = load_chapter_files(dir, ExportFormat::Html).unwrap();

    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].order, 1);
    assert_eq!(chapters[0].title, "第一章");
    assert!(chapters[0].body.contains("<p>a</p>"));
    assert_eq!(chapters[1].order, 2);
    assert_eq!(chapters[1].title, "第二章");
}

#[test]
fn load_chapter_files_empty_returns_no_chapters() {
    let tmp = tempfile::tempdir().unwrap();
    let result = load_chapter_files(tmp.path(), ExportFormat::Txt);
    assert!(matches!(result, Err(ExportError::NoChapters)));
}
```

若测试模块还没有 `use crate::config::ExportFormat;`，补上。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test export::exporter::tests::load_chapter_files -- --nocapture`
Expected: 编译失败，报 `cannot find function load_chapter_files` / `no variant NoChapters`。

- [ ] **Step 3: 实现 `NoChapters` 与 `load_chapter_files`**

把 `src/export/exporter.rs` 的 `ExportError` 第一个变体替换为：

```rust
/// 没有任何可合并的章节（目录不存在、为空或全是 `0_` 辅助文件）。
#[error("没有可合并的章节")]
NoChapters,
```

在 `sort_chapter_files` 之后新增（`exporter.rs` 顶部补 `use std::sync::LazyLock;` 与 `use regex::Regex;`）：

```rust
/// 读取章节缓存目录，按顺序号排序并跳过 `0_` 辅助文件。
///
/// 标题来源：TXT / Markdown / EPUB 取文件名第一个 `_` 之后的部分；
/// HTML / PDF 优先取正文 `<title>`，再取 `<h1>`，最后回退文件名。
///
/// # Errors
///
/// 读取目录或章节文件失败时返回 [`ExportError::Io`]；没有章节时返回
/// [`ExportError::NoChapters`]。
pub fn load_chapter_files(
    dir: &Path,
    format: ExportFormat,
) -> Result<Vec<RenderedChapter>, ExportError> {
    let files = sort_chapter_files(dir)?;
    let mut chapters = Vec::new();
    for path in files {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with("0_") || !has_format_ext(&path, format) {
            continue;
        }
        let body = std::fs::read_to_string(&path)?;
        let order = (chapters.len() + 1) as u32;
        let mut title = match format {
            ExportFormat::Html | ExportFormat::Pdf => extract_body_title(&body).unwrap_or_default(),
            ExportFormat::Txt | ExportFormat::Markdown | ExportFormat::Epub => String::new(),
        };
        if title.trim().is_empty() {
            title = title_from_filename(name);
        }
        if title.is_empty() {
            title = format!("第 {order} 章");
        }
        chapters.push(RenderedChapter { order, title, body });
    }
    if chapters.is_empty() {
        return Err(ExportError::NoChapters);
    }
    Ok(chapters)
}

fn has_format_ext(path: &Path, format: ExportFormat) -> bool {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match format {
        ExportFormat::Txt => ext == "txt",
        ExportFormat::Markdown => ext == "md",
        ExportFormat::Html | ExportFormat::Pdf => ext == "html",
        ExportFormat::Epub => ext == "html" || ext == "xhtml",
    }
}

fn title_from_filename(name: &str) -> String {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(name);
    match stem.split_once('_') {
        Some((_, title)) => title.trim().to_string(),
        None => stem.trim().to_string(),
    }
}

fn extract_body_title(body: &str) -> Option<String> {
    static TITLE_RE: LazyLock<Regex> =
        LazyLock::new(|| compile_static_re(r"(?is)<title[^>]*>(.*?)</title>"));
    static H1_RE: LazyLock<Regex> =
        LazyLock::new(|| compile_static_re(r"(?is)<h1[^>]*>(.*?)</h1>"));
    for re in [&*TITLE_RE, &*H1_RE] {
        if let Some(cap) = re.captures(body) {
            let title = strip_html_tags(&cap[1]);
            if !title.is_empty() {
                return Some(title);
            }
        }
    }
    None
}
```

同时把 `sort_chapter_files` 文档里“跳过 `0_`”的错误描述删掉——它只排序，过滤由调用方负责。

在 `src/export/mod.rs` 的 re-export 中加入 `load_chapter_files`：

```rust
pub use exporter::{
    ExportError, Exporter, RenderedChapter, build_book_dir_name, exporter_for, load_chapter_files,
    sort_chapter_files, write_chapter_files, write_single_chapter,
};
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test export::exporter::tests::load_chapter_files -- --nocapture`
Expected: 2 passed。

- [ ] **Step 5: 修改 `Exporter` trait 签名**

把 `src/export/exporter.rs` 的 trait 改为：

```rust
pub trait Exporter {
    /// 输出扩展名，不带点。
    fn ext(&self) -> &'static str;

    /// 把内存章节合并为单个文件，返回产物路径。
    fn merge(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError>;

    /// 带封面的合并；默认忽略封面。
    fn merge_with_cover(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
        _cover_bytes: Option<&[u8]>,
    ) -> Result<PathBuf, ExportError> {
        self.merge(book, chapters, out_dir)
    }
}
```

- [ ] **Step 6: 改造 TXT 导出器**

`src/export/txt.rs` 的 `merge` 替换为：

```rust
    fn merge(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        if chapters.is_empty() {
            return Err(ExportError::NoChapters);
        }

        std::fs::create_dir_all(out_dir)?;
        let filename = fs::sanitize_filename(&format!("{}({}).txt", book.book_name, book.author));
        let out_path = unique_path(out_dir, &filename);
        let file = std::fs::File::create(&out_path)?;
        let mut writer =
            TxtEncodedWriter::new(BufWriter::new(file), self.encoding, &self.encoding_label);

        writer.write_str(&format!("书名：{}\n", book.book_name))?;
        writer.write_str(&format!("作者：{}\n", book.author))?;
        let intro = book.intro.as_deref().unwrap_or("");
        let intro_clean = strip_html_tags(intro);
        writer.write_str(&format!(
            "简介：{}\n\n",
            if intro_clean.is_empty() {
                "暂无"
            } else {
                intro_clean.as_str()
            }
        ))?;

        for chapter in chapters {
            writer.write_str(&chapter.body)?;
            if !chapter.body.ends_with("\n\n") {
                writer.write_str("\n")?;
            }
        }
        writer.finish()?;
        Ok(out_path)
    }
```

文件顶部 import 增加 `RenderedChapter`，删掉不再使用的 `sort_chapter_files`：

```rust
use crate::export::exporter::{
    ExportError, Exporter, RenderedChapter, strip_html_tags, unique_path,
};
```

TXT 测试模块加统一 helper，并把所有 `exp.merge(&book, &chapters, &out)` 改成 `load_and_merge(...)`：

```rust
fn load_and_merge(
    exp: &dyn Exporter,
    book: &Book,
    dir: &Path,
    out: &Path,
    format: ExportFormat,
) -> PathBuf {
    let chapters = crate::export::load_chapter_files(dir, format).unwrap();
    exp.merge(book, &chapters, out).unwrap()
}
```

空目录测试改为直接传空切片：

```rust
#[test]
fn empty_chapters_dir_returns_typed_error() {
    let tmp = tempfile::tempdir().unwrap();
    let book = Book::default();
    let exp = TxtExporter::new("UTF-8");
    let result = exp.merge(&book, &[], tmp.path());
    assert!(matches!(result, Err(ExportError::NoChapters)));
}
```

Run: `cargo test export::txt -- --nocapture`
Expected: 全部通过。

- [ ] **Step 7: 改造 Markdown 导出器**

`src/export/md.rs` 的 `merge` 替换为：

```rust
    fn merge(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        if chapters.is_empty() {
            return Err(ExportError::NoChapters);
        }
        std::fs::create_dir_all(out_dir)?;
        let filename = fs::sanitize_filename(&format!("{}({}).md", book.book_name, book.author));
        let out_path = unique_path(out_dir, &filename);

        let mut out = String::new();
        out.push_str("---\n");
        out.push_str(&format!("title: {}\n", book.book_name));
        out.push_str(&format!("author: {}\n", book.author));
        if let Some(intro) = book.intro.as_deref() {
            let cleaned = strip_html_tags(intro);
            if !cleaned.is_empty() {
                out.push_str("description: |\n");
                for line in cleaned.lines() {
                    out.push_str("  ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        out.push_str("---\n\n");
        out.push_str(&format!("# {}\n\n", book.book_name));
        out.push_str("## 目录\n\n");
        for (idx, chapter) in chapters.iter().enumerate() {
            out.push_str(&format!("- [{}](#chapter-{})\n", chapter.title, idx + 1));
        }
        out.push('\n');
        for (idx, chapter) in chapters.iter().enumerate() {
            out.push_str(&format!("<a id=\"chapter-{}\"></a>\n", idx + 1));
            out.push_str(chapter.body.trim_end());
            out.push_str("\n\n");
        }
        std::fs::write(&out_path, out)?;
        Ok(out_path)
    }
```

Import 去掉 `sort_chapter_files` / `chapter_title_from_path`（若不再使用），加 `RenderedChapter`。

测试同样加 `load_and_merge` helper（`ExportFormat::Markdown`），空测试传 `&[]`。

Run: `cargo test export::md -- --nocapture`
Expected: 全部通过。

- [ ] **Step 8: 改造 HTML 导出器（直接从内存打包 zip）**

`src/export/html.rs` 的 `merge` 替换为：

```rust
    fn merge(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        if chapters.is_empty() {
            return Err(ExportError::NoChapters);
        }
        std::fs::create_dir_all(out_dir)?;
        let filename = fs::sanitize_filename(&format!("{}({}).zip", book.book_name, book.author));
        let out_path = unique_path(out_dir, &filename);

        let file = std::fs::File::create(&out_path)?;
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        let digit = chapters.len().to_string().len().max(3);
        let mut toc = String::from("文件名\t\t\t\t章节名\n");
        for (idx, chapter) in chapters.iter().enumerate() {
            let name = format!("{}.html", pad_zero((idx + 1) as u32, digit));
            zip.start_file(&name, options)
                .map_err(|e| ExportError::Zip(e.to_string()))?;
            zip.write_all(chapter.body.as_bytes())?;
            let title = extract_title(&chapter.body).unwrap_or_else(|| chapter.title.clone());
            toc.push_str(&format!("{name}\t\t{title}\n"));
        }
        zip.start_file("0_目录.txt", options)
            .map_err(|e| ExportError::Zip(e.to_string()))?;
        zip.write_all(toc.as_bytes())?;
        zip.finish().map_err(|e| ExportError::Zip(e.to_string()))?;
        Ok(out_path)
    }
```

删除不再需要的 `zip_directory`；`extract_title` 保留供 TOC 用。测试 helper 用 `ExportFormat::Html`，断言 zip 中仍有 `001_.html`、`002_.html`、`0_目录.txt`。

Run: `cargo test export::html -- --nocapture`
Expected: 全部通过。

- [ ] **Step 9: 改造 EPUB 导出器**

把 `src/export/epub.rs` 的 `merge` / `merge_with_cover` / `merge_with_cover_bytes` 参数统一为 `chapters: &[RenderedChapter]`：

```rust
    fn merge(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        merge_with_cover_bytes(book, chapters, out_dir, None)
    }

    fn merge_with_cover(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
        cover_bytes: Option<&[u8]>,
    ) -> Result<PathBuf, ExportError> {
        merge_with_cover_bytes(book, chapters, out_dir, cover_bytes)
    }
```

`merge_with_cover_bytes` 中删除 `sort_chapter_files` 与 `File::open`，章节循环改为：

```rust
    if chapters.is_empty() {
        return Err(ExportError::NoChapters);
    }
    let total_chapters = chapters.len();
    let digit = total_chapters.to_string().len().max(3);
    for (idx, chapter) in chapters.iter().enumerate() {
        let id = pad_zero((idx + 1) as u32, digit);
        let entry_name = format!("{id}.xhtml");
        let title = if chapter.title.trim().is_empty() {
            format!("第 {} 章", idx + 1)
        } else {
            chapter.title.clone()
        };
        builder
            .add_content(
                EpubContent::new(&entry_name, chapter.body.as_bytes())
                    .title(title)
                    .reftype(ReferenceType::Text),
            )
            .map_err(|e| ExportError::Epub(format!("add chapter {entry_name}: {e}")))?;
    }
```

封面分支保留。删除 `is_chapter_xhtml`、`chapter_title_from_filename` 与 `use std::fs::File`（若仅封面不再使用 File；封面是 `&[u8]`）。测试 helper 用 `ExportFormat::Epub`。

Run: `cargo test export::epub -- --nocapture`
Expected: 全部通过。

- [ ] **Step 10: 改造 PDF 导出器**

`src/export/pdf/document.rs` 的 `merge` 前半段替换为：

```rust
    fn merge(
        &self,
        book: &Book,
        chapters: &[RenderedChapter],
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        if chapters.is_empty() {
            return Err(ExportError::NoChapters);
        }
        let parsed: Vec<(Option<String>, Vec<String>)> = chapters
            .iter()
            .map(|chapter| {
                let body = extract_body(&chapter.body).unwrap_or_else(|| chapter.body.trim().to_string());
                let body = strip_nav_bar(&body);
                let (title, paragraphs) = extract_chapter_content(&body);
                let title = title.or_else(|| {
                    (!chapter.title.trim().is_empty()).then(|| chapter.title.clone())
                });
                (title, paragraphs)
            })
            .collect();
```

后面的 `let total_chapters = chapters.len();` 改为 `let total_chapters = parsed.len();`，分页循环改为遍历 `parsed.iter()`。`tracing::instrument` 的 `fields(chapters_dir = %chapters_dir.display())` 改为 `fields(chapters = chapters.len())`。删除 `sort_chapter_files` import 与文件读取逻辑。

测试 helper 用 `ExportFormat::Pdf`，样本正文写成 `<h1>第一章</h1><p>正文</p>`。

Run: `cargo test export::pdf -- --nocapture`
Expected: 全部通过。

- [ ] **Step 11: 更新下载调用方**

`src/crawler/download.rs:368-371` 改为：

```rust
            let exporter = export::exporter_for(cfg.download.ext_name, &cfg.download.txt_encoding);
            let out_dir = std::path::Path::new(&cfg.download.download_path);
            let chapters = export::load_chapter_files(&chapters_dir, cfg.download.ext_name)?;
            let final_path =
                exporter.merge_with_cover(book, &chapters, out_dir, cover_bytes.as_deref())?;
```

`CrawlerError` 已有 `Export(#[from] crate::export::ExportError)`，`?` 可直接转换。

- [ ] **Step 12: 跑完整导出层测试与静态检查**

Run:
```bash
cargo test export::
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```
Expected: 全绿；若 `cargo fmt --check` 失败，运行 `cargo fmt` 后重跑。

- [ ] **Step 13: 提交**

```bash
git add src/export src/crawler/download.rs
git commit -m "refactor(export): 导出器改为接收内存章节"
```

---

### Task 2: 工具业务层骨架、SourceBook 与编码探测

**Files:**
- Modify: `src/lib.rs`（注册 `pub mod tools;`）
- Create: `src/tools/mod.rs`
- Create: `src/tools/source_book.rs`
- Create: `src/tools/encoding.rs`
- Test: `src/tools/source_book.rs`、`src/tools/encoding.rs` 内联 `#[cfg(test)]`

**Interfaces:**
- Consumes: `crate::models::Book`、`crate::config::{ExportFormat, LangType}`、`crate::export::ExportError`。
- Produces:
  - `pub enum ToolRequest { Convert { .. }, Encoding { .. }, Zhconv { .. } }`
  - `pub enum ToolError`（含 `message_key() -> &'static str`、`detail() -> String`）
  - `pub struct SourceBook { title, author, intro, language, cover, chapters }`
  - `pub struct SourceChapter { title, paragraphs }`
  - `pub fn decode_bytes(bytes: &[u8], explicit: Option<&'static Encoding>) -> (String, &'static Encoding)`
  - `pub fn detect_encoding(bytes: &[u8]) -> &'static Encoding`
  - `pub fn detect_file_encoding(path: &Path) -> Result<&'static Encoding, ToolError>`

- [ ] **Step 1: 写编码探测与 SourceBook 的失败测试**

`src/tools/encoding.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use encoding_rs::{GBK, UTF_8};

    use super::{decode_bytes, detect_encoding};

    #[test]
    fn utf8_bom_is_detected_and_stripped() {
        let bytes = [0xEF, 0xBB, 0xBF, b'h', b'i'];
        assert_eq!(detect_encoding(&bytes), UTF_8);
        let (text, encoding) = decode_bytes(&bytes, None);
        assert_eq!(text, "hi");
        assert_eq!(encoding, UTF_8);
    }

    #[test]
    fn gbk_text_is_detected() {
        let text = "这是一段用于检测编码的中文文本，应该被识别为 GBK 编码而不是 UTF-8。";
        let (bytes, _, _) = GBK.encode(text);
        assert_eq!(detect_encoding(&bytes), GBK);
    }

    #[test]
    fn explicit_encoding_wins_without_bom() {
        let text = "中文";
        let (bytes, _, _) = GBK.encode(text);
        let (decoded, used) = decode_bytes(&bytes, Some(GBK));
        assert_eq!(decoded, text);
        assert_eq!(used, GBK);
    }
}
```

`src/tools/source_book.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::Path;

    use super::SourceBook;

    #[test]
    fn to_book_fills_missing_metadata() {
        let book = SourceBook::default().to_book(Path::new("本地书.txt"));
        assert_eq!(book.book_name, "本地书");
        assert_eq!(book.author, "佚名");
        assert_eq!(book.language, "zh");
        assert!(book.intro.is_none());
    }

    #[test]
    fn to_book_keeps_real_metadata() {
        let source = SourceBook {
            title: "剑来".to_string(),
            author: "烽火戏诸侯".to_string(),
            intro: Some("简介".to_string()),
            language: "zh-CN".to_string(),
            ..SourceBook::default()
        };
        let book = source.to_book(Path::new("x.epub"));
        assert_eq!(book.book_name, "剑来");
        assert_eq!(book.author, "烽火戏诸侯");
        assert_eq!(book.intro.as_deref(), Some("简介"));
        assert_eq!(book.language, "zh-CN");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test tools:: -- --nocapture`
Expected: 编译失败，`crate::tools` 不存在。

- [ ] **Step 3: 注册模块并创建 `src/tools/mod.rs`**

`src/lib.rs` 的模块声明处按字母序加入：

```rust
pub mod tools;
```

`src/tools/mod.rs`：

```rust
//! 本地小说文件工具：格式转换 / 编码转换 / 简繁转换。
//!
//! UI 只依赖本模块的 [`ToolRequest`] / [`ToolError`]；具体实现按职责拆到
//! `load`（输入解析）、`split`（切章）、`render`（章节渲染）、`convert` /
//! `encoding` / `zhconv`（三条流程）。

pub mod encoding;
pub mod source_book;

use std::path::PathBuf;

use thiserror::Error;

use crate::config::{ExportFormat, LangType};
use crate::export::ExportError;

pub use encoding::detect_file_encoding;
pub use source_book::{SourceBook, SourceChapter};

/// 一次工具请求（UI → 业务层）。
#[derive(Debug, Clone)]
pub enum ToolRequest {
    /// 本地文件 → EPUB / TXT / HTML / PDF / Markdown。
    Convert {
        source: PathBuf,
        out_dir: PathBuf,
        target: ExportFormat,
        txt_encoding: String,
        source_encoding: Option<String>,
        chapter_regex: Option<String>,
    },
    /// TXT 文件编码转换。
    Encoding {
        source: PathBuf,
        out_dir: PathBuf,
        source_encoding: Option<String>,
        target_encoding: String,
    },
    /// TXT / Markdown 简繁转换。
    Zhconv {
        source: PathBuf,
        out_dir: PathBuf,
        target: LangType,
    },
}

/// 工具业务层错误。
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("不支持的文件格式: {0}")]
    UnsupportedFormat(String),
    #[error("读取文件失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("解码文件失败: {0}")]
    Decode(String),
    #[error("章节正则无效: {0}")]
    InvalidRegex(String),
    #[error("ZIP 结构异常: {0}")]
    ZipStructure(String),
    #[error("EPUB 结构异常: {0}")]
    EpubStructure(String),
    #[error("文件没有可转换的正文")]
    NoContent,
    #[error("目标编码无法表示全部字符: {0}")]
    Unencodable(String),
    #[error("未知编码: {0}")]
    UnknownEncoding(String),
    #[error("导出失败: {0}")]
    Export(#[from] ExportError),
}

impl ToolError {
    /// 对应的 i18n key（`Tools.error.*`），UI 用它展示三语错误摘要。
    #[must_use]
    pub fn message_key(&self) -> &'static str {
        match self {
            Self::UnsupportedFormat(_) => "Tools.error.unsupported_format",
            Self::Io(_) => "Tools.error.io",
            Self::Decode(_) => "Tools.error.decode",
            Self::InvalidRegex(_) => "Tools.error.invalid_regex",
            Self::ZipStructure(_) => "Tools.error.zip_structure",
            Self::EpubStructure(_) => "Tools.error.epub_structure",
            Self::NoContent => "Tools.error.no_content",
            Self::Unencodable(_) => "Tools.error.unencodable",
            Self::UnknownEncoding(_) => "Tools.error.unknown_encoding",
            Self::Export(_) => "Tools.error.export",
        }
    }

    /// 技术细节（可能含路径 / 正则错误），UI 以弱化小字展示。
    #[must_use]
    pub fn detail(&self) -> String {
        self.to_string()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::ToolError;

    #[test]
    fn every_error_has_message_key() {
        assert_eq!(ToolError::NoContent.message_key(), "Tools.error.no_content");
        assert_eq!(
            ToolError::InvalidRegex("x".to_string()).message_key(),
            "Tools.error.invalid_regex"
        );
        assert_eq!(
            ToolError::UnknownEncoding("x".to_string()).message_key(),
            "Tools.error.unknown_encoding"
        );
    }
}
```

- [ ] **Step 4: 创建 `src/tools/source_book.rs`**

```rust
//! 与格式无关的书稿模型：加载器产出它，转换流程再把它投影成导出层 `Book`。

use std::path::Path;

use crate::models::Book;

/// 从本地文件加载出的书稿。
#[derive(Debug, Clone, Default)]
pub struct SourceBook {
    pub title: String,
    pub author: String,
    pub intro: Option<String>,
    /// BCP-47 语言标签，例如 `zh` / `zh-CN`。
    pub language: String,
    /// EPUB 封面原始字节（其他格式为 `None`）。
    pub cover: Option<Vec<u8>>,
    pub chapters: Vec<SourceChapter>,
}

/// 章节 = 标题 + 已归一化的纯文本段落。
#[derive(Debug, Clone, Default)]
pub struct SourceChapter {
    pub title: String,
    pub paragraphs: Vec<String>,
}

impl SourceBook {
    /// 投影成导出层 `Book`；`source` 只用于缺书名时回退文件名。
    #[must_use]
    pub fn to_book(&self, source: &Path) -> Book {
        let fallback = source
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("未命名");
        Book {
            book_name: if self.title.trim().is_empty() {
                fallback.to_string()
            } else {
                self.title.trim().to_string()
            },
            author: if self.author.trim().is_empty() {
                "佚名".to_string()
            } else {
                self.author.trim().to_string()
            },
            intro: self
                .intro
                .clone()
                .filter(|intro| !intro.trim().is_empty()),
            language: if self.language.trim().is_empty() {
                "zh".to_string()
            } else {
                self.language.clone()
            },
            ..Book::default()
        }
    }
}
```

- [ ] **Step 5: 创建 `src/tools/encoding.rs`（仅探测/解码）**

```rust
//! 编码探测与解码。BOM 优先，其次用户显式指定，最后 chardetng 猜测。

use std::path::Path;

use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, UTF_8};

use super::ToolError;

/// 解码字节；返回 (文本, 实际使用的编码)。
#[must_use]
pub fn decode_bytes(
    bytes: &[u8],
    explicit: Option<&'static Encoding>,
) -> (String, &'static Encoding) {
    if let Some((encoding, bom_len)) = sniff_bom(bytes) {
        let (text, _, _) = encoding.decode(&bytes[bom_len..]);
        return (text.into_owned(), encoding);
    }
    let encoding = explicit.unwrap_or_else(|| detect_encoding(bytes));
    let (text, _, _) = encoding.decode(bytes);
    (text.into_owned(), encoding)
}

/// 只做探测，不消费文本。
#[must_use]
pub fn detect_encoding(bytes: &[u8]) -> &'static Encoding {
    if let Some((encoding, _)) = sniff_bom(bytes) {
        return encoding;
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    detector.guess(None, true)
}

/// 读取文件并探测编码。
///
/// # Errors
///
/// 文件读取失败时返回 [`ToolError::Io`]。
pub fn detect_file_encoding(path: &Path) -> Result<&'static Encoding, ToolError> {
    let bytes = std::fs::read(path)?;
    Ok(detect_encoding(&bytes))
}

/// 解析用户传入的编码名；`auto` / 空串表示交给探测。
///
/// # Errors
///
/// 标签无法识别时返回 [`ToolError::UnknownEncoding`]。
pub fn parse_encoding(label: Option<&str>) -> Result<Option<&'static Encoding>, ToolError> {
    match label.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("auto") => Ok(None),
        Some(label) => Encoding::for_label(label.as_bytes())
            .map(Some)
            .ok_or_else(|| ToolError::UnknownEncoding(label.to_string())),
    }
}

fn sniff_bom(bytes: &[u8]) -> Option<(&'static Encoding, usize)> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        Some((UTF_8, 3))
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Some((UTF_16LE, 2))
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        Some((UTF_16BE, 2))
    } else {
        None
    }
}
```

（`parse_encoding` 放在这里，`load` 模块直接调用 `super::encoding::parse_encoding`。）

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test tools:: -- --nocapture`
Expected: 5 passed。

- [ ] **Step 7: 提交**

```bash
git add src/lib.rs src/tools
git commit -m "feat(tools): 新增工具业务层骨架与书稿模型"
```

---

### Task 3: 章节切分器

**Files:**
- Modify: `src/tools/mod.rs`（`pub mod split;`）
- Create: `src/tools/split.rs`
- Test: `src/tools/split.rs` 内联 `#[cfg(test)]`

**Interfaces:**
- Consumes: `SourceChapter`。
- Produces:
  - `pub const DEFAULT_CHAPTER_PATTERN: &str`
  - `pub fn compile_pattern(custom: Option<&str>) -> Result<Regex, ToolError>`
  - `pub fn split_chapters(paragraphs: &[String], regex: &Regex, book_title: &str) -> Vec<SourceChapter>`

- [ ] **Step 1: 写失败测试**

`src/tools/split.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::{compile_pattern, split_chapters};

    fn paragraphs(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn splits_default_headings_with_preface() {
        let paragraphs = paragraphs(&["简介", "第一章 起航", "甲", "第2章 风", "乙"]);
        let chapters = split_chapters(&paragraphs, &compile_pattern(None).unwrap(), "书");
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].title, "前言");
        assert_eq!(chapters[0].paragraphs, vec!["简介"]);
        assert_eq!(chapters[1].title, "第一章 起航");
        assert_eq!(chapters[1].paragraphs, vec!["甲"]);
        assert_eq!(chapters[2].title, "第2章 风");
        assert_eq!(chapters[2].paragraphs, vec!["乙"]);
    }

    #[test]
    fn chapter_at_first_line_has_no_preface() {
        let paragraphs = paragraphs(&["第1章 开始", "甲"]);
        let chapters = split_chapters(&paragraphs, &compile_pattern(None).unwrap(), "书");
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "第1章 开始");
    }

    #[test]
    fn custom_regex_overrides_default() {
        let paragraphs = paragraphs(&["第一话 初见", "甲", "第二话 重逢", "乙"]);
        let regex = compile_pattern(Some(r"^第.+话(\s+.*)?$")).unwrap();
        let chapters = split_chapters(&paragraphs, &regex, "书");
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "第一话 初见");
        assert_eq!(chapters[1].title, "第二话 重逢");
    }

    #[test]
    fn no_heading_returns_single_chapter_named_after_book() {
        let paragraphs = paragraphs(&["甲", "乙"]);
        let chapters = split_chapters(&paragraphs, &compile_pattern(None).unwrap(), "无名书");
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "无名书");
        assert_eq!(chapters[0].paragraphs, vec!["甲", "乙"]);
    }

    #[test]
    fn heading_with_body_in_same_paragraph_is_not_a_heading() {
        let paragraphs = paragraphs(&["第一章 起航\n正文"]);
        let chapters = split_chapters(&paragraphs, &compile_pattern(None).unwrap(), "书");
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "书");
    }

    #[test]
    fn invalid_custom_regex_returns_error() {
        let result = compile_pattern(Some("("));
        assert!(matches!(result, Err(super::ToolError::InvalidRegex(_))));
    }

    #[test]
    fn blank_custom_regex_falls_back_to_default() {
        let regex = compile_pattern(Some("   ")).unwrap();
        assert!(regex.is_match("第三章 归途"));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test tools::split -- --nocapture`
Expected: 编译失败，`tools::split` 不存在。

- [ ] **Step 3: 实现切分器**

`src/tools/mod.rs` 模块声明加入：

```rust
pub mod split;
```

`src/tools/split.rs`：

```rust
//! 按标题行把段落流切成章节。

use std::sync::LazyLock;

use regex::Regex;

use super::ToolError;
use super::source_book::SourceChapter;

/// 内置章节标题规则：第 N 章/节/回/卷，以及序章、楔子、前言、后记、尾声、番外。
pub const DEFAULT_CHAPTER_PATTERN: &str = r"^\s*(?:第[0-9零一二三四五六七八九十百千两]+[章节回卷]|序[章言]|楔子|前[言序]|后记|尾声|番外(?:篇)?)(?:[\s:：、.．—-].*)?$";

/// 编译用户自定义正则；空白或 `None` 回落到内置规则。
///
/// # Errors
///
/// 自定义正则语法错误时返回 [`ToolError::InvalidRegex`]。
pub fn compile_pattern(custom: Option<&str>) -> Result<Regex, ToolError> {
    match custom.map(str::trim).filter(|value| !value.is_empty()) {
        Some(pattern) => {
            Regex::new(pattern).map_err(|error| ToolError::InvalidRegex(error.to_string()))
        }
        None => Ok(default_regex()),
    }
}

/// 标题必须独占一个段落（单行），正文里的“第一章”不会被误判。
#[must_use]
pub fn split_chapters(
    paragraphs: &[String],
    regex: &Regex,
    book_title: &str,
) -> Vec<SourceChapter> {
    let mut chapters = Vec::new();
    let mut saw_heading = false;
    let mut current = SourceChapter {
        title: "前言".to_string(),
        paragraphs: Vec::new(),
    };
    for paragraph in paragraphs {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            continue;
        }
        let is_heading = !trimmed.contains('\n') && regex.is_match(trimmed);
        if is_heading {
            saw_heading = true;
            if !current.paragraphs.is_empty() {
                chapters.push(current);
            }
            current = SourceChapter {
                title: trimmed.to_string(),
                paragraphs: Vec::new(),
            };
        } else {
            current.paragraphs.push(trimmed.to_string());
        }
    }
    if !current.paragraphs.is_empty() {
        if !saw_heading && chapters.is_empty() {
            current.title = book_title.to_string();
        }
        chapters.push(current);
    }
    if chapters.is_empty() {
        let body: Vec<String> = paragraphs
            .iter()
            .map(|paragraph| paragraph.trim().to_string())
            .filter(|paragraph| !paragraph.is_empty())
            .collect();
        if !body.is_empty() {
            chapters.push(SourceChapter {
                title: book_title.to_string(),
                paragraphs: body,
            });
        }
    }
    chapters
}

fn default_regex() -> Regex {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(DEFAULT_CHAPTER_PATTERN)
            .unwrap_or_else(|error| unreachable!("内置章节正则必须可编译: {error}"))
    });
    RE.clone()
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test tools::split -- --nocapture`
Expected: 7 passed。

- [ ] **Step 5: 提交**

```bash
git add src/tools/mod.rs src/tools/split.rs
git commit -m "feat(tools): 新增章节切分器"
```

---

### Task 4: TXT 与 Markdown 加载器

**Files:**
- Modify: `src/tools/mod.rs`（`pub mod load;`）
- Create: `src/tools/load/mod.rs`
- Create: `src/tools/load/txt.rs`
- Create: `src/tools/load/markdown.rs`
- Test: `src/tools/load/txt.rs`、`src/tools/load/markdown.rs`

**Interfaces:**
- Consumes: `decode_bytes`、`parse_encoding`、`split_chapters`、`SourceBook`。
- Produces:
  - `pub struct LoadOptions<'a> { pub source_encoding: Option<&'static Encoding>, pub chapter_regex: Option<&'a str> }`（`Copy + Default`）
  - `pub fn load_path(path: &Path, opts: &LoadOptions<'_>) -> Result<SourceBook, ToolError>`

- [ ] **Step 1: 写失败测试**

`src/tools/load/txt.rs`：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::super::LoadOptions;
    use super::super::super::load_path;

    fn write_gbk(path: &std::path::Path, text: &str) {
        let (bytes, _, _) = encoding_rs::GBK.encode(text);
        std::fs::write(path, &bytes).unwrap();
    }

    #[test]
    fn loads_gbk_txt_and_splits_chapters() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("测试书.txt");
        write_gbk(
            &path,
            "简介\n\n第一章 起航\n\n第一段\n\n第2章 风\n\n第二段\n",
        );

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "测试书");
        assert_eq!(book.chapters.len(), 3);
        assert_eq!(book.chapters[0].title, "前言");
        assert_eq!(book.chapters[0].paragraphs, vec!["简介"]);
        assert_eq!(book.chapters[1].title, "第一章 起航");
        assert_eq!(book.chapters[1].paragraphs, vec!["第一段"]);
        assert_eq!(book.chapters[2].title, "第2章 风");
    }

    #[test]
    fn txt_without_blank_lines_uses_one_paragraph_per_line() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("无空行.txt");
        std::fs::write(&path, "第一段\n第二段\n").unwrap();

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.chapters.len(), 1);
        assert_eq!(book.chapters[0].title, "无空行");
        assert_eq!(book.chapters[0].paragraphs, vec!["第一段", "第二段"]);
    }

    #[test]
    fn empty_txt_returns_no_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("空.txt");
        std::fs::write(&path, "  \n\n  ").unwrap();
        let result = load_path(&path, &LoadOptions::default());
        assert!(matches!(result, Err(super::super::super::ToolError::NoContent)));
    }
}
```

`src/tools/load/markdown.rs`：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::super::LoadOptions;
    use super::super::super::load_path;
    use std::path::Path;

    fn write_md(path: &Path, text: &str) {
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn reads_front_matter_and_atx_headings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("剑来.md");
        write_md(
            &path,
            "---\ntitle: 剑来\nauthor: 烽火戏诸侯\ndescription: 一个少年\n---\n\n# 第一章\n\n甲\n\n## 第二节\n\n乙\n",
        );

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "剑来");
        assert_eq!(book.author, "烽火戏诸侯");
        assert_eq!(book.intro.as_deref(), Some("一个少年"));
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "第一章");
        assert_eq!(book.chapters[1].title, "第二节");
    }

    #[test]
    fn markdown_without_front_matter_falls_back_to_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("无元数据.md");
        write_md(&path, "# 第一章\n\n甲\n");
        let book = load_path(&path, &LoadOptions::default()).unwrap();
        assert_eq!(book.title, "无元数据");
        assert_eq!(book.chapters.len(), 1);
    }

    #[test]
    fn multiline_description_is_joined() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("书.md");
        write_md(
            &path,
            "---\ntitle: 书\ndescription: |\n  第一行\n  第二行\n---\n\n# 第一章\n\n甲\n",
        );
        let book = load_path(&path, &LoadOptions::default()).unwrap();
        assert_eq!(book.intro.as_deref(), Some("第一行\n第二行"));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test tools::load -- --nocapture`
Expected: 编译失败，`tools::load` 不存在。

- [ ] **Step 3: 创建 `src/tools/load/mod.rs`**

`src/tools/mod.rs` 加入：

```rust
pub mod load;
```

`src/tools/load/mod.rs`：

```rust
//! 本地文件 → [`SourceBook`]。按扩展名分派，所有文本都归一化为纯文本段落。

pub mod markdown;
pub mod txt;

use std::path::Path;

use encoding_rs::Encoding;
use regex::Regex;

use super::ToolError;
use super::source_book::SourceBook;
use super::split;

/// 加载选项。
#[derive(Debug, Clone, Copy, Default)]
pub struct LoadOptions<'a> {
    /// 用户显式指定的源编码；`None` = BOM / 自动探测。
    pub source_encoding: Option<&'static Encoding>,
    /// 用户自定义章节正则；`None` = 内置规则。
    pub chapter_regex: Option<&'a str>,
}

/// 按扩展名加载文件。
///
/// # Errors
///
/// 扩展名不支持、解码失败、正则无效、文件无正文时返回 [`ToolError`]。
pub fn load_path(path: &Path, opts: &LoadOptions<'_>) -> Result<SourceBook, ToolError> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "txt" => txt::load(path, opts, &split::compile_pattern(opts.chapter_regex)?),
        "md" | "markdown" => {
            markdown::load(path, opts, &split::compile_pattern(opts.chapter_regex)?)
        }
        _ => Err(ToolError::UnsupportedFormat(ext)),
    }
}

pub(super) fn stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_string()
}
```

- [ ] **Step 4: 实现 TXT 加载器**

`src/tools/load/txt.rs`：

```rust
//! 纯文本加载：BOM/显式编码/chardetng 解码，按空行分段（全篇无空行时按行分段）。

use std::path::Path;

use regex::Regex;

use super::{LoadOptions, stem};
use crate::tools::ToolError;
use crate::tools::encoding::decode_bytes;
use crate::tools::source_book::SourceBook;
use crate::tools::split::split_chapters;

pub(super) fn load(
    path: &Path,
    opts: &LoadOptions<'_>,
    regex: &Regex,
) -> Result<SourceBook, ToolError> {
    let bytes = std::fs::read(path)?;
    let (text, _encoding) = decode_bytes(&bytes, opts.source_encoding);
    let paragraphs = paragraphs_from_text(&text);
    if paragraphs.is_empty() {
        return Err(ToolError::NoContent);
    }
    let title = stem(path);
    let chapters = split_chapters(&paragraphs, regex, &title);
    Ok(SourceBook {
        title,
        chapters,
        ..SourceBook::default()
    })
}

fn paragraphs_from_text(text: &str) -> Vec<String> {
    let has_blank_line = text.lines().any(|line| line.trim().is_empty());
    let mut paragraphs = Vec::new();
    if has_blank_line {
        let mut current: Vec<&str> = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                if !current.is_empty() {
                    paragraphs.push(current.join("\n").trim().to_string());
                    current.clear();
                }
            } else {
                current.push(line.trim_end());
            }
        }
        if !current.is_empty() {
            paragraphs.push(current.join("\n").trim().to_string());
        }
    } else {
        for line in text.lines() {
            let line = line.trim();
            if !line.is_empty() {
                paragraphs.push(line.to_string());
            }
        }
    }
    paragraphs.retain(|paragraph| !paragraph.is_empty());
    paragraphs
}
```

- [ ] **Step 5: 实现 Markdown 加载器**

`src/tools/load/markdown.rs`：

```rust
//! Markdown 加载：手写解析 YAML front matter，ATX 一至三级标题视作章节标题。

use std::path::Path;

use regex::Regex;

use super::{LoadOptions, stem};
use crate::tools::ToolError;
use crate::tools::encoding::decode_bytes;
use crate::tools::source_book::SourceBook;
use crate::tools::split::split_chapters;

#[derive(Debug, Default)]
struct FrontMatter {
    title: Option<String>,
    author: Option<String>,
    description: Option<String>,
}

pub(super) fn load(
    path: &Path,
    opts: &LoadOptions<'_>,
    regex: &Regex,
) -> Result<SourceBook, ToolError> {
    let bytes = std::fs::read(path)?;
    let (text, _encoding) = decode_bytes(&bytes, opts.source_encoding);
    let (meta, body) = split_front_matter(&text);
    let title = meta.title.unwrap_or_else(|| stem(path));
    let paragraphs = markdown_paragraphs(&body);
    if paragraphs.is_empty() {
        return Err(ToolError::NoContent);
    }
    let chapters = split_chapters(&paragraphs, regex, &title);
    Ok(SourceBook {
        title,
        author: meta.author.unwrap_or_default(),
        intro: meta.description,
        chapters,
        ..SourceBook::default()
    })
}

fn split_front_matter(text: &str) -> (FrontMatter, String) {
    let mut meta = FrontMatter::default();
    let Some(rest) = text.strip_prefix("---") else {
        return (meta, text.to_string());
    };
    let Some(rest) = rest.strip_prefix('\n').or_else(|| rest.strip_prefix("\r\n")) else {
        return (meta, text.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (meta, text.to_string());
    };
    let (front, after) = rest.split_at(end);
    let body = after
        .trim_start_matches('\n')
        .strip_prefix("---")
        .unwrap_or(after)
        .trim_start_matches('\n')
        .to_string();

    let mut description_lines: Vec<String> = Vec::new();
    let mut in_block_description = false;
    for line in front.lines() {
        if in_block_description {
            if line.starts_with(' ') || line.starts_with('\t') {
                description_lines.push(line.trim().to_string());
                continue;
            }
            in_block_description = false;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "title" if !value.is_empty() => meta.title = Some(unquote(value)),
            "author" if !value.is_empty() => meta.author = Some(unquote(value)),
            "description" if value == "|" => in_block_description = true,
            "description" if !value.is_empty() => meta.description = Some(unquote(value)),
            _ => {}
        }
    }
    if meta.description.is_none() && !description_lines.is_empty() {
        meta.description = Some(description_lines.join("\n"));
    }
    (meta, body)
}

fn unquote(value: &str) -> String {
    value
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_string()
}

fn markdown_paragraphs(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush(&mut current, &mut out);
            continue;
        }
        if let Some(heading) = atx_heading(trimmed) {
            flush(&mut current, &mut out);
            out.push(heading);
            continue;
        }
        current.push(line.trim_end().to_string());
    }
    flush(&mut current, &mut out);
    out
}

fn flush(current: &mut Vec<String>, out: &mut Vec<String>) {
    if current.is_empty() {
        return;
    }
    let paragraph = current.join("\n");
    let paragraph = paragraph.trim();
    if !paragraph.is_empty() {
        out.push(paragraph.to_string());
    }
    current.clear();
}

fn atx_heading(line: &str) -> Option<String> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let rest = line[hashes..].trim_start();
    let rest = rest.trim_end_matches('#').trim();
    (!rest.is_empty()).then(|| rest.to_string())
}
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test tools::load -- --nocapture`
Expected: 6 passed。

- [ ] **Step 7: 提交**

```bash
git add src/tools/mod.rs src/tools/load
git commit -m "feat(tools): 支持加载 TXT 与 Markdown"
```

---

### Task 5: HTML 与 ZIP 加载器

**Files:**
- Modify: `src/tools/load/mod.rs`（声明 `pub mod html;`，分派 `html/htm/zip`）
- Create: `src/tools/load/html.rs`
- Test: `src/tools/load/html.rs`

**Interfaces:**
- Consumes: `decode_bytes`、`split_chapters`、`SourceBook`。
- Produces:
  - `pub(super) fn load_file(path: &Path, opts: &LoadOptions<'_>, regex: &Regex) -> Result<SourceBook, ToolError>`
  - `pub(super) fn load_zip(path: &Path) -> Result<SourceBook, ToolError>`
  - `pub(crate) fn html_to_paragraphs(fragment: &str) -> Vec<String>`
  - `pub(crate) fn extract_tag_text(html: &str, tag: &str) -> Option<String>`
  - `pub(crate) fn strip_html(input: &str) -> String`
  - `pub(crate) fn decode_entities(input: &str) -> String`

- [ ] **Step 1: 写失败测试**

`src/tools/load/html.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::io::Write;

    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    use super::super::LoadOptions;
    use super::super::super::load_path;

    #[test]
    fn loads_single_html_with_metadata_and_headings() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("测试书.html");
        std::fs::write(
            &path,
            r#"<!DOCTYPE html><html><head><title>测试书</title><meta name="author" content="作者甲"/></head><body><script>alert(1)</script><h1>第一章</h1><p>第一段&amp;续</p><h1>第二章</h1><p>第二段</p></body></html>"#,
        )
        .unwrap();

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "测试书");
        assert_eq!(book.author, "作者甲");
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "第一章");
        assert_eq!(book.chapters[0].paragraphs, vec!["第一段&续"]);
        assert!(!book.chapters[0].paragraphs[0].contains("alert"));
    }

    #[test]
    fn loads_zip_with_toc_titles() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("导出书.zip");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        zip.start_file("001_.html", options).unwrap();
        zip.write_all(
            "<html><head><title>第一章</title></head><body><h1>第一章</h1><p>甲&amp;乙</p></body></html>"
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("002_.html", options).unwrap();
        zip.write_all(
            "<html><head><title>第二章</title></head><body><h1>第二章</h1><p>丙</p></body></html>"
                .as_bytes(),
        )
        .unwrap();
        zip.start_file("0_目录.txt", options).unwrap();
        zip.write_all("文件名\t\t\t\t章节名\n001_.html\t\t第一章\n002_.html\t\t第二章\n".as_bytes())
            .unwrap();
        zip.finish().unwrap();

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "导出书");
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "第一章");
        assert_eq!(book.chapters[1].title, "第二章");
        assert_eq!(book.chapters[0].paragraphs, vec!["甲&乙"]);
    }

    #[test]
    fn html_paragraphs_decode_entities_and_keep_line_breaks() {
        let paragraphs = super::html_to_paragraphs(
            "<div>第一行<br/>第二行</div><div>&hellip;&#x4e2d;</div>",
        );
        assert_eq!(paragraphs, vec!["第一行\n第二行", "…中"]);
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test tools::load::html -- --nocapture`
Expected: 编译失败，`tools::load::html` 不存在。

- [ ] **Step 3: 扩展 `load/mod.rs` 分派**

模块声明加入 `pub mod html;`，`match ext.as_str()` 增加：

```rust
        "html" | "htm" => html::load_file(path, opts, &split::compile_pattern(opts.chapter_regex)?),
        "zip" => html::load_zip(path),
```

- [ ] **Step 4: 实现 HTML / ZIP 加载器**

`src/tools/load/html.rs`：

```rust
//! HTML / ZIP 加载。ZIP 优先读导出器生成的 `0_目录.txt`，否则按文件名数字前缀排序。

use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use zip::ZipArchive;

use super::{LoadOptions, stem};
use crate::tools::ToolError;
use crate::tools::encoding::decode_bytes;
use crate::tools::source_book::{SourceBook, SourceChapter};
use crate::tools::split::split_chapters;

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?s)<[^>]+>"));
static BODY_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<body\b[^>]*>(.*?)</body>"));
static SCRIPT_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<script\b[^>]*>.*?</script>"));
static STYLE_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<style\b[^>]*>.*?</style>"));
static COMMENT_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<!--.*?-->"));
static BR_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?i)<br\s*/?>"));
static BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile_regex(
        r"(?is)</?(?:p|div|section|article|li|ul|ol|tr|td|th|table|blockquote|pre)\b[^>]*>",
    )
});
static H1_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<h1\b[^>]*>(.*?)</h1>"));
static H2_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<h2\b[^>]*>(.*?)</h2>"));
static H3_RE: LazyLock<Regex> =
    LazyLock::new(|| compile_regex(r"(?is)<h3\b[^>]*>(.*?)</h3>"));

pub(super) fn load_file(
    path: &Path,
    opts: &LoadOptions<'_>,
    regex: &Regex,
) -> Result<SourceBook, ToolError> {
    let bytes = std::fs::read(path)?;
    let (html, _encoding) = decode_bytes(&bytes, opts.source_encoding);
    let title = extract_tag_text(&html, "title").unwrap_or_else(|| stem(path));
    let author = meta_content(&html, "author").unwrap_or_default();
    let intro = meta_content(&html, "description");
    let body = BODY_RE
        .captures(&html)
        .map_or(html.as_str(), |cap| cap.get(1).map_or("", |m| m.as_str()))
        .to_string();
    let paragraphs = html_to_paragraphs(&body);
    if paragraphs.is_empty() {
        return Err(ToolError::NoContent);
    }
    let chapters = split_chapters(&paragraphs, regex, &title);
    Ok(SourceBook {
        title,
        author,
        intro,
        chapters,
        ..SourceBook::default()
    })
}

pub(super) fn load_zip(path: &Path) -> Result<SourceBook, ToolError> {
    let file = std::fs::File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| ToolError::ZipStructure(error.to_string()))?;

    let mut names = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| ToolError::ZipStructure(error.to_string()))?;
        if entry.is_file() {
            names.push(entry.name().to_string());
        }
    }

    let mut toc: Vec<(String, String)> = Vec::new();
    if let Some(toc_name) = names
        .iter()
        .find(|name| base_name(name) == "0_目录.txt")
        .cloned()
    {
        let toc_bytes = read_entry(&mut archive, &toc_name)?;
        let toc_text = String::from_utf8_lossy(&toc_bytes);
        for line in toc_text.lines().skip(1) {
            let mut parts = line.split('\t').map(str::trim).filter(|part| !part.is_empty());
            if let (Some(file), Some(title)) = (parts.next(), parts.next()) {
                toc.push((file.to_string(), title.to_string()));
            }
        }
    }

    let mut chapter_names: Vec<String> = names
        .iter()
        .filter(|name| is_html(name) && !base_name(name).starts_with("0_"))
        .cloned()
        .collect();
    if toc.is_empty() {
        chapter_names.sort_by_key(|name| numeric_prefix(base_name(name)));
    } else {
        let mut ordered = Vec::new();
        for (file, _title) in &toc {
            if let Some(found) = chapter_names
                .iter()
                .find(|name| base_name(name) == base_name(file))
                .cloned()
            {
                ordered.push(found);
            }
        }
        for name in &chapter_names {
            if !ordered.contains(name) {
                ordered.push(name.clone());
            }
        }
        chapter_names = ordered;
    }

    let mut chapters = Vec::new();
    for name in chapter_names {
        let bytes = read_entry(&mut archive, &name)?;
        let html = String::from_utf8_lossy(&bytes);
        let toc_title = toc
            .iter()
            .find(|(file, _title)| base_name(file) == base_name(&name))
            .map(|(_file, title)| title.clone());
        let title = extract_tag_text(&html, "h1")
            .or(toc_title)
            .or_else(|| extract_tag_text(&html, "title"))
            .unwrap_or_else(|| format!("第 {} 章", chapters.len() + 1));
        let mut paragraphs = html_to_paragraphs(&html);
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
        return Err(ToolError::ZipStructure("ZIP 中未找到 HTML 章节".to_string()));
    }
    Ok(SourceBook {
        title: stem(path),
        chapters,
        ..SourceBook::default()
    })
}

fn read_entry(archive: &mut ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>, ToolError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| ToolError::ZipStructure(format!("{name}: {error}")))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| ToolError::ZipStructure(format!("{name}: {error}")))?;
    Ok(bytes)
}

fn base_name(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

fn is_html(name: &str) -> bool {
    let name = base_name(name).to_ascii_lowercase();
    name.ends_with(".html") || name.ends_with(".htm") || name.ends_with(".xhtml")
}

fn numeric_prefix(name: &str) -> u32 {
    let digits: String = name
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or(u32::MAX)
}

fn meta_content(html: &str, name: &str) -> Option<String> {
    static META_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<meta\b[^>]*>"));
    for cap in META_RE.captures_iter(html) {
        let tag = &cap[0];
        if attr(tag, "name").is_some_and(|value| value.eq_ignore_ascii_case(name)) {
            return attr(tag, "content")
                .map(|value| decode_entities(&value).trim().to_string())
                .filter(|value| !value.is_empty());
        }
    }
    None
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = tag[from..].find(name) {
        let start = from + pos;
        let before_ok = start == 0
            || tag.as_bytes()[start - 1].is_ascii_whitespace()
            || tag.as_bytes()[start - 1] == b'<';
        if before_ok {
            let after = tag[start + name.len()..].trim_start();
            if let Some(rest) = after.strip_prefix('=') {
                let rest = rest.trim_start();
                let quote = rest.chars().next()?;
                if quote == '"' || quote == '\'' {
                    let inner = &rest[1..];
                    if let Some(end) = inner.find(quote) {
                        return Some(inner[..end].to_string());
                    }
                }
            }
        }
        from = start + name.len();
    }
    None
}

pub(crate) fn extract_tag_text(html: &str, tag: &str) -> Option<String> {
    let pattern = format!(r"(?is)<{tag}\b[^>]*>(.*?)</{tag}>");
    let regex = Regex::new(&pattern).ok()?;
    regex
        .captures(html)
        .map(|cap| strip_html(&cap[1]))
        .filter(|value| !value.is_empty())
}

pub(crate) fn strip_html(input: &str) -> String {
    let plain = TAG_RE.replace_all(input, " ");
    decode_entities(&plain)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn html_to_paragraphs(fragment: &str) -> Vec<String> {
    let mut html = SCRIPT_RE.replace_all(fragment, "").into_owned();
    html = STYLE_RE.replace_all(&html, "").into_owned();
    html = COMMENT_RE.replace_all(&html, "").into_owned();
    for regex in [&*H1_RE, &*H2_RE, &*H3_RE] {
        html = regex.replace_all(&html, "\n\n$1\n\n").into_owned();
    }
    html = BR_RE.replace_all(&html, "\n").into_owned();
    html = BLOCK_RE.replace_all(&html, "\n\n").into_owned();
    let text = TAG_RE.replace_all(&html, "").into_owned();
    let text = decode_entities(&text);
    text.split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub(crate) fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        let Some(semi) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" | "emsp" | "ensp" => Some(' '),
            "hellip" => Some('…'),
            "mdash" => Some('—'),
            "ndash" => Some('–'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            _ => {
                if let Some(hex) = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else if let Some(dec) = entity.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32)
                } else {
                    None
                }
            }
        };
        if let Some(ch) = decoded {
            out.push(ch);
        } else {
            out.push_str(&tail[..=semi]);
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    out
}

fn compile_regex(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|error| unreachable!("内置正则必须可编译: {error}"))
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test tools::load::html -- --nocapture`
Expected: 3 passed。

- [ ] **Step 6: 提交**

```bash
git add src/tools/load
git commit -m "feat(tools): 支持加载 HTML 与导出 ZIP"
```

---

### Task 6: EPUB 加载器

**Files:**
- Modify: `src/tools/load/mod.rs`（声明 `pub mod epub;`，分派 `epub`）
- Create: `src/tools/load/epub.rs`
- Test: `src/tools/load/epub.rs`

**Interfaces:**
- Consumes: `load::html::{extract_tag_text, html_to_paragraphs}`、`SourceBook`。
- Produces: `pub(super) fn load(path: &Path) -> Result<SourceBook, ToolError>`。

- [ ] **Step 1: 写失败测试**

`src/tools/load/epub.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use std::path::Path;

    use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

    use super::super::super::load_path;
    use super::super::LoadOptions;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    fn write_epub(path: &Path) {
        let zip = ZipLibrary::new().unwrap();
        let mut builder = EpubBuilder::new(zip).unwrap();
        builder.metadata("title", "测试书").unwrap();
        builder.metadata("author", "作者甲").unwrap();
        builder.metadata("language", "zh-CN").unwrap();
        builder.metadata("description", "一本书").unwrap();
        builder
            .add_cover_image("cover.png", PNG, "image/png")
            .unwrap();
        builder
            .add_content(
                EpubContent::new(
                    "c1.xhtml",
                    "<html><head><title>第一章</title></head><body><h1>第一章</h1><p>甲</p></body></html>"
                        .as_bytes(),
                )
                .title("第一章"),
            )
            .unwrap();
        builder
            .add_content(
                EpubContent::new(
                    "c2.xhtml",
                    "<html><head><title>第二章</title></head><body><h1>第二章</h1><p>乙</p></body></html>"
                        .as_bytes(),
                )
                .title("第二章"),
            )
            .unwrap();
        let mut file = std::fs::File::create(path).unwrap();
        builder.generate(&mut file).unwrap();
    }

    #[test]
    fn loads_metadata_spine_and_cover() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("书.epub");
        write_epub(&path);

        let book = load_path(&path, &LoadOptions::default()).unwrap();

        assert_eq!(book.title, "测试书");
        assert_eq!(book.author, "作者甲");
        assert_eq!(book.language, "zh-CN");
        assert_eq!(book.intro.as_deref(), Some("一本书"));
        assert_eq!(book.chapters.len(), 2);
        assert_eq!(book.chapters[0].title, "第一章");
        assert_eq!(book.chapters[0].paragraphs, vec!["甲"]);
        assert_eq!(book.chapters[1].paragraphs, vec!["乙"]);
        assert!(book.cover.as_ref().is_some_and(|bytes| !bytes.is_empty()));
    }

    #[test]
    fn missing_container_returns_epub_structure_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("坏.epub");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("mimetype", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.finish().unwrap();

        let result = load_path(&path, &LoadOptions::default());
        assert!(matches!(
            result,
            Err(super::super::super::ToolError::EpubStructure(_))
        ));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test tools::load::epub -- --nocapture`
Expected: 编译失败，`tools::load::epub` 不存在。

- [ ] **Step 3: 扩展 `load/mod.rs`**

模块声明加入 `pub mod epub;`，`match ext.as_str()` 增加：

```rust
        "epub" => epub::load(path),
```

- [ ] **Step 4: 实现 EPUB 加载器**

`src/tools/load/epub.rs`：

```rust
//! EPUB 加载：container.xml → OPF metadata / manifest / spine，按 spine 顺序读正文。

use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use zip::ZipArchive;

use super::html::{extract_tag_text, html_to_paragraphs};
use super::stem;
use crate::tools::ToolError;
use crate::tools::source_book::{SourceBook, SourceChapter};

static ITEM_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<item\b[^>]*>"));
static ITEMREF_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<itemref\b[^>]*>"));
static ROOTFILE_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<rootfile\b[^>]*>"));
static META_RE: LazyLock<Regex> = LazyLock::new(|| compile_regex(r"(?is)<meta\b[^>]*>"));

#[derive(Debug)]
struct ManifestItem {
    id: String,
    href: String,
    media_type: String,
    properties: String,
}

pub(super) fn load(path: &Path) -> Result<SourceBook, ToolError> {
    let file = std::fs::File::open(path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| ToolError::EpubStructure(error.to_string()))?;

    if archive.by_name("META-INF/encryption.xml").is_ok() {
        return Err(ToolError::EpubStructure(
            "不支持 DRM 加密的 EPUB".to_string(),
        ));
    }

    let container = read_entry(&mut archive, "META-INF/container.xml")?;
    let opf_path = ROOTFILE_RE
        .captures(&container)
        .and_then(|cap| attr(&cap[0], "full-path"))
        .ok_or_else(|| ToolError::EpubStructure("container.xml 缺少 rootfile".to_string()))?;
    let opf = String::from_utf8_lossy(&read_entry(&mut archive, &opf_path)?).into_owned();
    let base = opf_path
        .rsplit_once('/')
        .map(|(dir, _file)| dir.to_string())
        .unwrap_or_default();

    let items = manifest_items(&opf);
    let spine = spine_ids(&opf);
    let cover_id = cover_manifest_id(&opf, &items);

    let mut chapters = Vec::new();
    for idref in spine {
        let Some(item) = items.iter().find(|item| item.id == idref) else {
            continue;
        };
        if !item.media_type.contains("html") {
            continue;
        }
        let name = resolve_path(&base, &item.href);
        let html = String::from_utf8_lossy(&read_entry(&mut archive, &name)?).into_owned();
        let title = extract_tag_text(&html, "h1")
            .or_else(|| extract_tag_text(&html, "h2"))
            .or_else(|| extract_tag_text(&html, "title"))
            .unwrap_or_else(|| format!("第 {} 章", chapters.len() + 1));
        let mut paragraphs = html_to_paragraphs(&html);
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
        .and_then(|id| items.iter().find(|item| item.id == id))
        .and_then(|item| {
            let name = resolve_path(&base, &item.href);
            let bytes = read_entry(&mut archive, &name).ok()?;
            (!bytes.is_empty()).then_some(bytes)
        });

    Ok(SourceBook {
        title: dc_text(&opf, "title").unwrap_or_else(|| stem(path)),
        author: dc_text(&opf, "creator").unwrap_or_default(),
        intro: dc_text(&opf, "description"),
        language: dc_text(&opf, "language").unwrap_or_else(|| "zh".to_string()),
        cover,
        chapters,
    })
}

fn manifest_items(opf: &str) -> Vec<ManifestItem> {
    ITEM_RE
        .captures_iter(opf)
        .filter_map(|cap| {
            Some(ManifestItem {
                id: attr(&cap[0], "id")?,
                href: attr(&cap[0], "href")?,
                media_type: attr(&cap[0], "media-type").unwrap_or_default(),
                properties: attr(&cap[0], "properties").unwrap_or_default(),
            })
        })
        .collect()
}

fn spine_ids(opf: &str) -> Vec<String> {
    ITEMREF_RE
        .captures_iter(opf)
        .filter_map(|cap| attr(&cap[0], "idref"))
        .collect()
}

fn cover_manifest_id(opf: &str, items: &[ManifestItem]) -> Option<String> {
    if let Some(item) = items
        .iter()
        .find(|item| item.properties.split_whitespace().any(|value| value == "cover-image"))
    {
        return Some(item.id.clone());
    }
    META_RE.captures_iter(opf).find_map(|cap| {
        let tag = &cap[0];
        (attr(tag, "name").is_some_and(|value| value.eq_ignore_ascii_case("cover")))
            .then(|| attr(tag, "content"))
            .flatten()
    })
}

fn dc_text(opf: &str, tag: &str) -> Option<String> {
    let pattern = format!(r"(?is)<dc:{tag}\b[^>]*>(.*?)</dc:{tag}>");
    let regex = Regex::new(&pattern).ok()?;
    regex
        .captures(opf)
        .map(|cap| super::html::strip_html(&cap[1]))
        .filter(|value| !value.is_empty())
}

fn read_entry(archive: &mut ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>, ToolError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| ToolError::EpubStructure(format!("{name}: {error}")))?;
    let mut bytes = Vec::new();
    entry
        .read_to_end(&mut bytes)
        .map_err(|error| ToolError::EpubStructure(format!("{name}: {error}")))?;
    Ok(bytes)
}

fn resolve_path(base: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let href = percent_decode(href);
    let mut parts: Vec<&str> = base.split('/').filter(|part| !part.is_empty()).collect();
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                out.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = tag[from..].find(name) {
        let start = from + pos;
        let before_ok = start == 0
            || tag.as_bytes()[start - 1].is_ascii_whitespace()
            || tag.as_bytes()[start - 1] == b'<';
        if before_ok {
            let after = tag[start + name.len()..].trim_start();
            if let Some(rest) = after.strip_prefix('=') {
                let rest = rest.trim_start();
                let quote = rest.chars().next()?;
                if quote == '"' || quote == '\'' {
                    let inner = &rest[1..];
                    if let Some(end) = inner.find(quote) {
                        return Some(inner[..end].to_string());
                    }
                }
            }
        }
        from = start + name.len();
    }
    None
}

fn compile_regex(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|error| unreachable!("内置正则必须可编译: {error}"))
}
```

注意：`EpubBuilder` 生成的 OPF 位于 `OEBPS/content.opf`，`base` 解析为 `OEBPS`，`resolve_path` 会拼出 `OEBPS/c1.xhtml`。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test tools::load::epub -- --nocapture`
Expected: 2 passed。

- [ ] **Step 6: 提交**

```bash
git add src/tools/load
git commit -m "feat(tools): 支持加载 EPUB"
```

---

### Task 7: 章节渲染

**Files:**
- Modify: `src/tools/mod.rs`（`pub mod render;`）
- Create: `src/tools/render.rs`
- Test: `src/tools/render.rs`

**Interfaces:**
- Consumes: `SourceChapter`、`ExportFormat`。
- Produces:
  - `pub fn render_chapter(chapter: &SourceChapter, target: ExportFormat) -> String`
  - `pub(crate) fn escape_html(input: &str) -> String`

- [ ] **Step 1: 写失败测试**

`src/tools/render.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use crate::config::ExportFormat;
    use crate::tools::source_book::SourceChapter;

    use super::render_chapter;

    fn chapter() -> SourceChapter {
        SourceChapter {
            title: "第一章 <起航>".to_string(),
            paragraphs: vec!["甲 & 乙".to_string(), "丙\n丁".to_string()],
        }
    }

    #[test]
    fn txt_puts_title_then_indented_paragraphs() {
        let out = render_chapter(&chapter(), ExportFormat::Txt);
        assert!(out.starts_with("第一章 <起航>\n\n"));
        assert!(out.contains("\u{3000}\u{3000}甲 & 乙\n"));
        assert!(out.contains("\u{3000}\u{3000}丙\n丁\n"));
    }

    #[test]
    fn markdown_uses_heading_and_blank_lines() {
        let out = render_chapter(&chapter(), ExportFormat::Markdown);
        assert!(out.starts_with("## 第一章 <起航>\n\n"));
        assert!(out.contains("甲 & 乙\n\n丙\n丁\n"));
    }

    #[test]
    fn html_escapes_title_and_wraps_paragraphs() {
        let out = render_chapter(&chapter(), ExportFormat::Html);
        assert!(out.starts_with("<!DOCTYPE html>"));
        assert!(out.contains("<title>第一章 &lt;起航&gt;</title>"));
        assert!(out.contains("<h1>第一章 &lt;起航&gt;</h1>"));
        assert!(out.contains("<p>甲 &amp; 乙</p>"));
        assert!(out.contains("<p>丙<br/>丁</p>"));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test tools::render -- --nocapture`
Expected: 编译失败，`tools::render` 不存在。

- [ ] **Step 3: 实现渲染器**

`src/tools/render.rs`：

```rust
//! 把归一化章节渲染成各导出器需要的内存正文。
//!
//! TXT / Markdown 保留纯文本；HTML / EPUB / PDF 共用完整 HTML 文档，
//! 满足 HTML 目录 `<title>`、EPUB 正文与 PDF 的 `<h1>` / `<p>` 提取约定。

use crate::config::ExportFormat;
use crate::tools::source_book::SourceChapter;

/// 中文排版首行缩进（两个全角空格）。
const INDENT: &str = "\u{3000}\u{3000}";

/// 按目标格式渲染一章。
#[must_use]
pub fn render_chapter(chapter: &SourceChapter, target: ExportFormat) -> String {
    match target {
        ExportFormat::Txt => render_txt(chapter),
        ExportFormat::Markdown => render_markdown(chapter),
        ExportFormat::Html | ExportFormat::Epub | ExportFormat::Pdf => render_html(chapter),
    }
}

fn render_txt(chapter: &SourceChapter) -> String {
    let mut out = String::new();
    out.push_str(&chapter.title);
    out.push_str("\n\n");
    for paragraph in &chapter.paragraphs {
        out.push_str(INDENT);
        out.push_str(paragraph);
        out.push('\n');
    }
    out
}

fn render_markdown(chapter: &SourceChapter) -> String {
    let mut out = format!("## {}\n\n", chapter.title);
    for (index, paragraph) in chapter.paragraphs.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(paragraph.trim());
        out.push('\n');
    }
    out
}

fn render_html(chapter: &SourceChapter) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html lang=\"zh\">\n<head>\n<meta charset=\"utf-8\"/>\n");
    out.push_str("<title>");
    out.push_str(&escape_html(&chapter.title));
    out.push_str("</title>\n</head>\n<body>\n<h1>");
    out.push_str(&escape_html(&chapter.title));
    out.push_str("</h1>\n");
    for paragraph in &chapter.paragraphs {
        out.push_str("<p>");
        out.push_str(&escape_html(paragraph).replace('\n', "<br/>"));
        out.push_str("</p>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// HTML 文本转义。
#[must_use]
pub(crate) fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test tools::render -- --nocapture`
Expected: 3 passed。

- [ ] **Step 5: 提交**

```bash
git add src/tools/mod.rs src/tools/render.rs
git commit -m "feat(tools): 章节按目标格式渲染"
```

---

### Task 8: 格式转换流程

**Files:**
- Modify: `src/tools/mod.rs`（`pub mod convert;`）
- Create: `src/tools/convert.rs`
- Test: `src/tools/convert.rs`

**Interfaces:**
- Consumes: `load::{load_path, LoadOptions}`、`render::render_chapter`、`encoding::parse_encoding`、`export::{RenderedChapter, exporter_for}`。
- Produces:
  - `pub fn run_convert(source: &Path, out_dir: &Path, target: ExportFormat, txt_encoding: &str, source_encoding: Option<&str>, chapter_regex: Option<&str>) -> Result<PathBuf, ToolError>`

- [ ] **Step 1: 写失败测试**

`src/tools/convert.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use crate::config::ExportFormat;
    use crate::tools::ToolError;

    use super::run_convert;

    #[test]
    fn converts_txt_to_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(
            &source,
            "第一章 起航\n\n第一段\n\n第二章 到达\n\n第二段\n",
        )
        .unwrap();

        let path = run_convert(
            &source,
            &tmp.path().join("out"),
            ExportFormat::Markdown,
            "UTF-8",
            None,
            None,
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("## 第一章 起航"));
        assert!(text.contains("## 第二章 到达"));
        assert!(text.contains("第一段"));
    }

    #[test]
    fn converts_txt_to_epub() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "第1章 开始\n\n正文\n").unwrap();

        let path = run_convert(
            &source,
            &tmp.path().join("out"),
            ExportFormat::Epub,
            "UTF-8",
            None,
            None,
        )
        .unwrap();

        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("epub"));
        assert_eq!(std::fs::read(&path).unwrap().first(), Some(&b'P'));
    }

    #[test]
    fn custom_regex_controls_splitting() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "一\n\n甲\n\n二\n\n乙\n").unwrap();

        let path = run_convert(
            &source,
            &tmp.path().join("out"),
            ExportFormat::Txt,
            "UTF-8",
            None,
            Some("^[一二]$"),
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("一"));
        assert!(text.contains("二"));
        assert!(text.contains("甲"));
        assert!(text.contains("乙"));
    }

    #[test]
    fn rejects_unsupported_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.docx");
        std::fs::write(&source, b"x").unwrap();

        let result = run_convert(
            &source,
            tmp.path(),
            ExportFormat::Txt,
            "UTF-8",
            None,
            None,
        );

        assert!(matches!(result, Err(ToolError::UnsupportedFormat(_))));
    }
}
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test tools::convert -- --nocapture`
Expected: 编译失败，`tools::convert` 不存在。

- [ ] **Step 3: 实现转换流程**

`src/tools/convert.rs`：

```rust
//! 本地文件 → 导出格式。加载器归一化后逐章渲染，再交给现有导出器合并。

use std::path::{Path, PathBuf};

use crate::config::ExportFormat;
use crate::export::{RenderedChapter, exporter_for};
use crate::tools::ToolError;
use crate::tools::encoding::parse_encoding;
use crate::tools::load::{LoadOptions, load_path};
use crate::tools::render::render_chapter;

/// 执行格式转换，返回产物路径。
///
/// # Errors
///
/// 文件格式不支持、读取 / 解码失败、正则无效或导出失败时返回 [`ToolError`]。
pub fn run_convert(
    source: &Path,
    out_dir: &Path,
    target: ExportFormat,
    txt_encoding: &str,
    source_encoding: Option<&str>,
    chapter_regex: Option<&str>,
) -> Result<PathBuf, ToolError> {
    let options = LoadOptions {
        source_encoding: parse_encoding(source_encoding)?,
        chapter_regex,
    };
    let book = load_path(source, &options)?;
    let rendered: Vec<RenderedChapter> = book
        .chapters
        .iter()
        .enumerate()
        .map(|(index, chapter)| RenderedChapter {
            order: u32::try_from(index + 1).unwrap_or(u32::MAX),
            title: chapter.title.clone(),
            body: render_chapter(chapter, target),
        })
        .collect();

    std::fs::create_dir_all(out_dir)?;
    let exporter = exporter_for(target, txt_encoding);
    let cover = if matches!(target, ExportFormat::Epub) {
        book.cover.as_deref()
    } else {
        None
    };
    Ok(exporter.merge_with_cover(&book.to_book(source), &rendered, out_dir, cover)?)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test tools::convert -- --nocapture`
Expected: 4 passed。

- [ ] **Step 5: 提交**

```bash
git add src/tools/mod.rs src/tools/convert.rs
git commit -m "feat(tools): 本地文件格式转换"
```

---

### Task 9: TXT 编码转换

**Files:**
- Modify: `src/tools/encoding.rs`（新增 `run_encoding`，补充 `PathBuf` 导入）
- Test: `src/tools/encoding.rs`

**Interfaces:**
- Consumes: `decode_bytes`、`parse_encoding`、`ToolError`、`crate::export::exporter::unique_path`。
- Produces:
  - `pub fn run_encoding(source: &Path, out_dir: &Path, source_encoding: Option<&str>, target_encoding: &str) -> Result<PathBuf, ToolError>`

- [ ] **Step 1: 写失败测试**

在 `src/tools/encoding.rs` 已有的 `#[cfg(test)] mod tests` 中追加：

```rust
    #[test]
    fn converts_utf8_to_gbk() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "中文测试").unwrap();

        let path = super::run_encoding(&source, tmp.path(), None, "GBK").unwrap();

        assert!(path.file_name().unwrap().to_string_lossy().contains("(GBK)"));
        let bytes = std::fs::read(&path).unwrap();
        let (text, _, _) = GBK.decode(&bytes);
        assert_eq!(text, "中文测试");
    }

    #[test]
    fn strips_bom_when_writing_utf8() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("中文".as_bytes());
        std::fs::write(&source, bytes).unwrap();

        let path = super::run_encoding(&source, tmp.path(), None, "UTF-8").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), "中文".as_bytes());
    }

    #[test]
    fn rejects_unknown_target_encoding() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "中文").unwrap();

        let result = super::run_encoding(&source, tmp.path(), None, "nope");

        assert!(matches!(result, Err(super::ToolError::UnknownEncoding(_))));
    }

    #[test]
    fn rejects_non_txt_source() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.md");
        std::fs::write(&source, "中文").unwrap();

        let result = super::run_encoding(&source, tmp.path(), None, "GBK");

        assert!(matches!(result, Err(super::ToolError::UnsupportedFormat(_))));
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test tools::encoding::tests::converts_utf8_to_gbk -- --nocapture`
Expected: 编译失败，`run_encoding` 不存在。

- [ ] **Step 3: 实现 `run_encoding`**

把文件顶部导入改为：

```rust
use std::path::{Path, PathBuf};

use encoding_rs::{Encoding, UTF_16BE, UTF_16LE, UTF_8};

use super::ToolError;
```

文件末尾追加：

```rust
/// TXT 编码转换：解码后按目标编码重新写出，文件名追加编码标签。
///
/// # Errors
///
/// 源文件不是 `.txt`、编码未知、目标编码无法表示全部字符或写入失败时返回 [`ToolError`]。
pub fn run_encoding(
    source: &Path,
    out_dir: &Path,
    source_encoding: Option<&str>,
    target_encoding: &str,
) -> Result<PathBuf, ToolError> {
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "txt" {
        return Err(ToolError::UnsupportedFormat(extension));
    }

    let bytes = std::fs::read(source)?;
    let (text, _detected) = decode_bytes(&bytes, parse_encoding(source_encoding)?);

    let target = Encoding::for_label(target_encoding.trim().as_bytes())
        .ok_or_else(|| ToolError::UnknownEncoding(target_encoding.to_string()))?;
    // encoding_rs 的 UTF-16 只用于解码，`output_encoding` 会回落到 UTF-8。
    if target.output_encoding() != target {
        return Err(ToolError::UnknownEncoding(target_encoding.to_string()));
    }
    let (encoded, _, had_errors) = target.encode(&text);
    if had_errors {
        return Err(ToolError::Unencodable(target.name().to_string()));
    }

    std::fs::create_dir_all(out_dir)?;
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("未命名");
    let filename = format!("{stem} ({}).txt", target.name());
    let path = crate::export::exporter::unique_path(out_dir, &filename);
    std::fs::write(&path, encoded.as_ref())?;
    Ok(path)
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test tools::encoding -- --nocapture`
Expected: 7 passed（原 3 + 新 4）。

- [ ] **Step 5: 提交**

```bash
git add src/tools/encoding.rs
git commit -m "feat(tools): TXT 编码转换"
```

---

### Task 10: 简繁转换与请求分派

**Files:**
- Modify: `src/tools/mod.rs`（`pub mod zhconv;`、`pub fn run`）
- Create: `src/tools/zhconv.rs`
- Test: `src/tools/zhconv.rs`、`src/tools/mod.rs`

**Interfaces:**
- Consumes: `decode_bytes`、`parse_encoding`、`crate::utils::zhconv::convert_text`、`LangType`。
- Produces:
  - `pub fn run_zhconv(source: &Path, out_dir: &Path, target: LangType) -> Result<PathBuf, ToolError>`
  - `pub fn run(request: ToolRequest) -> Result<PathBuf, ToolError>`

- [ ] **Step 1: 写失败测试**

`src/tools/zhconv.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use crate::config::LangType;

    use super::run_zhconv;

    #[test]
    fn converts_simplified_txt_to_traditional() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "国家的书").unwrap();

        let path = run_zhconv(&source, tmp.path(), LangType::ZhTw).unwrap();

        assert!(path.file_name().unwrap().to_string_lossy().contains("(繁体)"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "國家的書");
    }

    #[test]
    fn markdown_output_is_utf8() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.md");
        std::fs::write(&source, "# 国家\n").unwrap();

        let path = run_zhconv(&source, tmp.path(), LangType::ZhHant).unwrap();

        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("md"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# 國家\n");
    }

    #[test]
    fn rejects_epub() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.epub");
        std::fs::write(&source, b"x").unwrap();

        let result = run_zhconv(&source, tmp.path(), LangType::ZhCn);

        assert!(matches!(
            result,
            Err(crate::tools::ToolError::UnsupportedFormat(_))
        ));
    }
}
```

`src/tools/mod.rs` 的 tests 模块追加：

```rust
    #[test]
    fn run_dispatches_convert() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "第1章 开始\n\n正文\n").unwrap();

        let path = super::run(super::ToolRequest::Convert {
            source,
            out_dir: tmp.path().join("out"),
            target: crate::config::ExportFormat::Txt,
            txt_encoding: "UTF-8".to_string(),
            source_encoding: None,
            chapter_regex: None,
        })
        .unwrap();

        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("txt"));
    }
```

- [ ] **Step 2: 运行确认失败**

Run: `cargo test tools::zhconv -- --nocapture`
Expected: 编译失败，`tools::zhconv` 不存在。

- [ ] **Step 3: 实现简繁转换**

`src/tools/zhconv.rs`：

```rust
//! TXT / Markdown 简繁转换：整文件转换，TXT 保持源编码。

use std::path::{Path, PathBuf};

use crate::config::LangType;
use crate::tools::ToolError;
use crate::tools::encoding::{decode_bytes, parse_encoding};
use crate::utils::zhconv::convert_text;

/// 执行简繁转换，返回产物路径。
///
/// # Errors
///
/// 源文件不是 `.txt` / `.md`、读取失败或目标编码无法表示转换结果时返回 [`ToolError`]。
pub fn run_zhconv(source: &Path, out_dir: &Path, target: LangType) -> Result<PathBuf, ToolError> {
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "txt" && extension != "md" {
        return Err(ToolError::UnsupportedFormat(extension));
    }

    let bytes = std::fs::read(source)?;
    let (text, detected) = decode_bytes(&bytes, parse_encoding(None)?);
    let converted = convert_text(&text, &target);

    std::fs::create_dir_all(out_dir)?;
    let stem = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("未命名");
    let suffix = match target {
        LangType::ZhCn => "简体",
        LangType::ZhTw | LangType::ZhHant => "繁体",
    };
    let filename = format!("{stem} ({suffix}).{extension}");
    let path = crate::export::exporter::unique_path(out_dir, &filename);

    if extension == "md" {
        std::fs::write(&path, converted.as_bytes())?;
    } else {
        let (encoded, _, had_errors) = detected.encode(&converted);
        if had_errors {
            return Err(ToolError::Unencodable(detected.name().to_string()));
        }
        std::fs::write(&path, encoded.as_ref())?;
    }
    Ok(path)
}
```

- [ ] **Step 4: 注册模块并实现 `run` 分派**

`src/tools/mod.rs` 的模块声明改为：

```rust
pub mod convert;
pub mod encoding;
pub mod load;
pub mod render;
pub mod source_book;
pub mod split;
pub mod zhconv;
```

文件末尾（`impl ToolError` 之后、tests 之前）追加：

```rust
/// 执行一次工具请求，返回产物路径。
///
/// # Errors
///
/// 透传各业务流程的 [`ToolError`]。
pub fn run(request: ToolRequest) -> Result<PathBuf, ToolError> {
    match request {
        ToolRequest::Convert {
            source,
            out_dir,
            target,
            txt_encoding,
            source_encoding,
            chapter_regex,
        } => convert::run_convert(
            &source,
            &out_dir,
            target,
            &txt_encoding,
            source_encoding.as_deref(),
            chapter_regex.as_deref(),
        ),
        ToolRequest::Encoding {
            source,
            out_dir,
            source_encoding,
            target_encoding,
        } => encoding::run_encoding(
            &source,
            &out_dir,
            source_encoding.as_deref(),
            &target_encoding,
        ),
        ToolRequest::Zhconv {
            source,
            out_dir,
            target,
        } => zhconv::run_zhconv(&source, &out_dir, target),
    }
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test tools:: -- --nocapture`
Expected: 全部通过（含 `run_dispatches_convert` 与 3 个 zhconv 测试）。

- [ ] **Step 6: 提交**

```bash
git add src/tools
git commit -m "feat(tools): 简繁转换与请求分派"
```

---

### Task 11: 导航接线与工具箱页面（含格式转换 UI）

**Files:**
- Modify: `src/desktop/actions.rs`
- Modify: `src/desktop/nav.rs`
- Modify: `src/desktop/mod.rs`
- Modify: `src/desktop/root.rs`
- Modify: `src/desktop/pages/mod.rs`
- Modify: `locales/app.yml`
- Create: `src/desktop/pages/tools/mod.rs`
- Create: `src/desktop/pages/tools/tools_page.rs`

**Interfaces:**
- Consumes: `crate::tools::{ToolRequest, run, detect_file_encoding, ToolError}`、`PageHeader`、`Input`/`InputState`、`AppModel::push_success/push_error`、`reveal_in_folder`。
- Produces: `pub struct ToolsPage`，实现 `Render` 且提供 `pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self`；新增 `desktop::ShowTools` action、`NavPage::Tools`。

- [ ] **Step 1: 注册 action 与导航页**

`src/desktop/actions.rs` 的 action 列表在 `ShowSources` 后插入：

```rust
        /// 切到工具箱页。
        ShowTools,
```

并在 impl 区加入 `impl Eq for ShowTools {}`。

`src/desktop/mod.rs` 的 re-export 改为：

```rust
pub use actions::{
    NextPage, PrevPage, Quit, ShowLibrary, ShowSearch, ShowSettings, ShowSources, ShowTasks,
    ShowTools, ToggleSidebar,
};
```

`src/desktop/nav.rs` 完整替换为：

```rust
//! 6 个一级导航页面 (`NavPage`) + 全局 key bindings + GPUI actions 注册。
//!
//! - `NavPage` enum + label/icon/next/prev helpers;
//! - [`register_key_bindings`] 在 `desktop::run` 启动时调一次 (actions 声明在 `desktop/actions.rs`)。
//!
//! 翻页不用 `Ctrl+Tab`: `gpui_kit::component` 的 `InputState` 把 `tab` / `shift-tab` 绑到
//! 自己的 `IndentInline` / `OutdentInline`, 焦点在 Input 时 Tab 事件被消费, 应用级翻页
//! action 拿不到。改用 `F6` 避开。

use gpui_kit::component::IconName;
use gpui_kit::{App, KeyBinding, SharedString};
use rust_i18n::t;

// `actions!` 宏在 `crate::desktop::actions` 调用, 生成的 action 类型经 `desktop` re-export
// 暴露为 `desktop::*`, 这里只 re-export 给 root.rs 用。
pub(super) use crate::desktop::{
    NextPage, PrevPage, ShowLibrary, ShowSearch, ShowSettings, ShowSources, ShowTasks, ShowTools,
    ToggleSidebar,
};

/// GPUI key context 名。`div().key_context("AppShell")` 时激活。
pub(super) const KEY_CONTEXT: &str = "AppShell";

/// 6 个一级导航页面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavPage {
    /// 搜索页（默认页）。
    #[default]
    Search,
    /// 下载任务页。
    Tasks,
    /// 本地书库页。
    Library,
    /// 书源管理页。
    Sources,
    /// 工具箱页。
    Tools,
    /// 设置页。
    Settings,
}

impl NavPage {
    /// `NavPage` → i18n key (`t!` 用)。
    pub(super) const fn label_key(self) -> &'static str {
        match self {
            Self::Search => "Nav.search",
            Self::Tasks => "Nav.tasks",
            Self::Library => "Nav.library",
            Self::Sources => "Nav.sources",
            Self::Tools => "Nav.tools",
            Self::Settings => "Nav.settings",
        }
    }

    /// 当前应用语言下的用户可见 label —— `t!` 走全局 locale (语言切换时由
    /// `gpui_kit::component::set_locale` 同步), 所以这里不需要 `lang` 参数。
    pub(super) fn label(self) -> SharedString {
        t!(self.label_key()).into()
    }

    pub(super) const fn icon(self) -> IconName {
        match self {
            Self::Search => IconName::Search,
            Self::Tasks => IconName::Inbox,
            Self::Library => IconName::BookOpen,
            Self::Sources => IconName::Globe,
            Self::Tools => IconName::SlidersHorizontal,
            Self::Settings => IconName::Settings,
        }
    }

    pub(super) const ALL: [Self; 6] = [
        Self::Search,
        Self::Tasks,
        Self::Library,
        Self::Sources,
        Self::Tools,
        Self::Settings,
    ];

    /// 下一个 page (循环)。
    pub(super) fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// 上一个 page (循环)。
    pub(super) fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// 全局 key bindings 注册。`desktop::run` 启动时调一次。
///
/// `cmd-1..6` 跳 6 个 page, `F6`/`Shift+F6` 循环翻页, `cmd-b` 折叠 sidebar。
#[tracing::instrument(name = "nav::register_key_bindings", skip_all)]
pub fn register_key_bindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-1", ShowSearch, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-2", ShowTasks, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-3", ShowLibrary, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-4", ShowSources, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-5", ShowTools, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-6", ShowSettings, Some(KEY_CONTEXT)),
        KeyBinding::new("f6", NextPage, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f6", PrevPage, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-b", ToggleSidebar, Some(KEY_CONTEXT)),
    ]);
}
```

- [ ] **Step 2: 接线 `RootView`**

`src/desktop/root.rs` 按下列位置修改：

1. 模块文档：`SidebarMenuItem × 5` → `× 6`，`cmd-1~cmd-5` → `cmd-1~cmd-6`。
2. 导入：

```rust
use crate::desktop::pages::{
    LibraryPage, SearchPage, SettingsPage, SourcesPage, TasksPage, ToolsPage,
};
use crate::desktop::{
    NextPage, PrevPage, ShowLibrary, ShowSearch, ShowSettings, ShowSources, ShowTasks, ShowTools,
    ToggleSidebar,
};
```

3. 字段与构造：

```rust
    // 6 个 page entity 一次性创建, 跨切换保持内部状态 (输入框 / 滚动位置)。
    library_page: Entity<LibraryPage>,
    search_page: Entity<SearchPage>,
    tasks_page: Entity<TasksPage>,
    sources_page: Entity<SourcesPage>,
    tools_page: Entity<ToolsPage>,
    settings_page: Entity<SettingsPage>,
```

```rust
        let sources_page = cx.new(|cx| SourcesPage::new(model.clone(), window, cx));
        let tools_page = cx.new(|cx| ToolsPage::new(model.clone(), window, cx));
        let settings_page = cx.new(|cx| SettingsPage::new(model.clone(), window, cx));
```

```rust
            sources_page,
            tools_page,
            settings_page,
```

4. `render_current_page` match 增加：

```rust
            NavPage::Tools => self.tools_page.clone().into_any_element(),
```

5. `render_sidebar` 的页面数组在 `NavPage::Sources` 后插入 `NavPage::Tools`。

6. `bind_nav_actions` 的文档 `8 个` → `9 个`，并在 `ShowSources` 后插入：

```rust
        .on_action(cx.listener(|this, _: &ShowTools, _, cx| this.navigate(NavPage::Tools, cx)))
```

- [ ] **Step 3: 注册页面模块**

`src/desktop/pages/mod.rs`：

```rust
//! 6 个一级导航页面的渲染入口。
//!
//! 每个 page 是独立 entity，由 `RootView` 在 `current_page` 切换时挂载。
//! 这种"每页一个 entity"模式便于：
//! - 页面状态（输入框内容 / 滚动位置 / 上次选择）跨切换保持
//! - 复杂页面（Search）的弹窗 / 子视图独立管理

pub mod library;
pub mod search;
pub mod settings;
pub mod sources;
pub mod tasks;
pub mod tools;

pub use library::LibraryPage;
pub use search::SearchPage;
pub use settings::SettingsPage;
pub use sources::SourcesPage;
pub use tasks::TasksPage;
pub use tools::ToolsPage;
```

`src/desktop/pages/tools/mod.rs`：

```rust
mod tools_page;

pub use tools_page::ToolsPage;
```

- [ ] **Step 4: 创建工具箱页面**

`src/desktop/pages/tools/tools_page.rs`：

```rust
//! 工具箱：格式转换 / 编码转换 / 简繁转换。
//!
//! 左侧工具导航 + 右侧表单；三种工具共享「源文件 / 输出目录 / 运行 / 结果」骨架，
//! 只有选项区不同。任务在 `background_executor` 上跑，避免阻塞 UI。

use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable, Icon, IconName, Selectable, Sizable, h_flex, v_flex,
};
use gpui_kit::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled, Window,
    div, px,
};
use rust_i18n::t;

use crate::config::{ExportFormat, LangType};
use crate::desktop::components::PageHeader;
use crate::desktop::model::AppModel;
use crate::tools::{ToolRequest, detect_file_encoding, run};

/// 支持的源文件扩展名（文件选择器过滤用）。
const SOURCE_EXTENSIONS: &[&str] = &["txt", "md", "markdown", "html", "htm", "epub", "zip"];

/// 三个工具。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolKind {
    Convert,
    Encoding,
    Zhconv,
}

impl ToolKind {
    const ALL: [Self; 3] = [Self::Convert, Self::Encoding, Self::Zhconv];

    const fn index(self) -> usize {
        match self {
            Self::Convert => 0,
            Self::Encoding => 1,
            Self::Zhconv => 2,
        }
    }

    const fn label_key(self) -> &'static str {
        match self {
            Self::Convert => "Tools.kind.convert",
            Self::Encoding => "Tools.kind.encoding",
            Self::Zhconv => "Tools.kind.zhconv",
        }
    }

    const fn description_key(self) -> &'static str {
        match self {
            Self::Convert => "Tools.desc.convert",
            Self::Encoding => "Tools.desc.encoding",
            Self::Zhconv => "Tools.desc.zhconv",
        }
    }

    const fn icon(self) -> IconName {
        match self {
            Self::Convert => IconName::ArrowRightLeft,
            Self::Encoding => IconName::FileOutput,
            Self::Zhconv => IconName::Languages,
        }
    }
}

/// 单个工具的表单状态。
#[derive(Debug, Clone)]
struct ToolForm {
    source: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    detected: Option<SharedString>,
    target_format: ExportFormat,
    txt_encoding: String,
    source_encoding: String,
    target_encoding: String,
    zhconv_target: LangType,
    running: bool,
    result: Option<PathBuf>,
    error: Option<(SharedString, String)>,
}

impl Default for ToolForm {
    fn default() -> Self {
        Self {
            source: None,
            out_dir: None,
            detected: None,
            target_format: ExportFormat::default(),
            txt_encoding: "UTF-8".to_string(),
            source_encoding: "auto".to_string(),
            target_encoding: "UTF-8".to_string(),
            zhconv_target: LangType::ZhTw,
            running: false,
            result: None,
            error: None,
        }
    }
}

/// 工具箱页面 entity。
pub struct ToolsPage {
    model: Entity<AppModel>,
    kind: ToolKind,
    forms: [ToolForm; 3],
    regex_input: Entity<InputState>,
}

impl ToolsPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let txt_encoding = model.read(cx).config.download.txt_encoding.clone();
        let placeholder: SharedString = t!("Tools.convert.chapter_regex_placeholder").into();
        let regex_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let mut forms = [ToolForm::default(), ToolForm::default(), ToolForm::default()];
        forms[ToolKind::Convert.index()].txt_encoding = txt_encoding;
        Self {
            model,
            kind: ToolKind::Convert,
            forms,
            regex_input,
        }
    }

    fn pick_source(&mut self, cx: &mut Context<Self>) {
        let kind = self.kind;
        let page = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            let file = rfd::AsyncFileDialog::new()
                .add_filter(t!("Tools.pick.filter").as_ref(), SOURCE_EXTENSIONS)
                .set_title(t!("Tools.pick.title").as_ref())
                .pick_file()
                .await;
            let Some(file) = file else {
                return;
            };
            let path = file.path().to_path_buf();
            let detect_text = supports_chapter_regex(&path);
            let _ = page.update(async_cx, |this, cx| {
                let form = &mut this.forms[kind.index()];
                form.source = Some(path.clone());
                form.out_dir = path.parent().map(Path::to_path_buf);
                form.detected = None;
                form.result = None;
                form.error = None;
                cx.notify();

                if !detect_text {
                    return;
                }
                let detect_path = path;
                cx.spawn(async move |this, async_cx| {
                    let detected = async_cx
                        .background_executor()
                        .spawn(async move { detect_file_encoding(&detect_path) })
                        .await;
                    if let Ok(encoding) = detected {
                        let name: SharedString = encoding.name().into();
                        let _ = this.update(async_cx, |this, cx| {
                            this.forms[kind.index()].detected = Some(name);
                            cx.notify();
                        });
                    }
                })
                .detach();
            });
        })
        .detach();
    }

    fn pick_out_dir(&mut self, cx: &mut Context<Self>) {
        let kind = self.kind;
        let form = &self.forms[kind.index()];
        let start = form.out_dir.clone().or_else(|| {
            form.source
                .as_ref()
                .and_then(|path| path.parent())
                .map(Path::to_path_buf)
        });
        let page = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            let mut dialog = rfd::AsyncFileDialog::new()
                .set_title(t!("Tools.pick.choose_folder").as_ref());
            if let Some(start) = start {
                dialog = dialog.set_directory(start);
            }
            let Some(folder) = dialog.pick_folder().await else {
                return;
            };
            let path = folder.path().to_path_buf();
            let _ = page.update(async_cx, |this, cx| {
                this.forms[kind.index()].out_dir = Some(path);
                cx.notify();
            });
        })
        .detach();
    }

    fn run_tool(&mut self, cx: &mut Context<Self>) {
        let kind = self.kind;
        let form = self.forms[kind.index()].clone();
        if form.running {
            return;
        }
        let Some(source) = form.source.clone() else {
            return;
        };
        let out_dir = form
            .out_dir
            .clone()
            .or_else(|| source.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let chapter_regex = if kind == ToolKind::Convert && supports_chapter_regex(&source) {
            let value = self.regex_input.read(cx).value().to_string();
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        } else {
            None
        };
        let request = match kind {
            ToolKind::Convert => ToolRequest::Convert {
                source,
                out_dir,
                target: form.target_format,
                txt_encoding: form.txt_encoding.clone(),
                source_encoding: normalize_encoding(&form.source_encoding),
                chapter_regex,
            },
            ToolKind::Encoding => ToolRequest::Encoding {
                source,
                out_dir,
                source_encoding: normalize_encoding(&form.source_encoding),
                target_encoding: form.target_encoding.clone(),
            },
            ToolKind::Zhconv => ToolRequest::Zhconv {
                source,
                out_dir,
                target: form.zhconv_target,
            },
        };

        let model = self.model.clone();
        {
            let form = &mut self.forms[kind.index()];
            form.running = true;
            form.result = None;
            form.error = None;
        }
        cx.notify();

        cx.spawn(async move |this, async_cx| {
            let outcome = async_cx
                .background_executor()
                .spawn(async move { run(request) })
                .await;
            let _ = this.update(async_cx, |this, cx| {
                let form = &mut this.forms[kind.index()];
                form.running = false;
                match outcome {
                    Ok(path) => {
                        form.result = Some(path.clone());
                        let message =
                            t!("Tools.toast.success", name = path.display().to_string()).to_string();
                        model.update(cx, |model, _cx| model.push_success(message));
                    }
                    Err(error) => {
                        let summary: SharedString = t!(error.message_key()).into();
                        let detail = error.detail();
                        form.error = Some((summary.clone(), detail.clone()));
                        model.update(cx, |model, _cx| model.push_error(format!("{summary}: {detail}")));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .w(px(220.0))
            .flex_none()
            .gap_1()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(cx.theme().muted)
            .children(ToolKind::ALL.iter().map(|kind| {
                let kind = *kind;
                let selected = self.kind == kind;
                let mut item = v_flex().gap_1().child(
                    Button::new(("tool-rail", kind.index() as u64))
                        .ghost()
                        .w_full()
                        .selected(selected)
                        .icon(Icon::new(kind.icon()))
                        .label(t!(kind.label_key()).into())
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.kind = kind;
                            cx.notify();
                        })),
                );
                if selected {
                    item = item.child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!(kind.description_key()).into()),
                    );
                }
                item
            }))
    }

    fn render_form(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let form = &self.forms[self.kind.index()];
        v_flex()
            .flex_1()
            .gap_4()
            .child(
                PageHeader::new(t!("Tools.page_title").into())
                    .subtitle(t!(self.kind.description_key()).into()),
            )
            .child(self.render_source_row(form, cx))
            .child(self.render_out_dir_row(form, cx))
            .child(self.render_options(cx))
            .child(self.render_actions(form, cx))
    }

    fn render_source_row(&self, form: &ToolForm, cx: &mut Context<Self>) -> impl IntoElement {
        let path_text: SharedString = form.source.as_ref().map_or_else(
            || t!("Tools.pick.none").into(),
            |path| path.display().to_string().into(),
        );
        let mut column = v_flex().gap_1().child(
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(path_text),
                )
                .child(
                    Button::new(("tool-pick-source", self.kind.index() as u64))
                        .small()
                        .label(t!("Tools.pick.choose_file").into())
                        .on_click(cx.listener(|this, _, _window, cx| this.pick_source(cx))),
                ),
        );
        if let Some(detected) = &form.detected {
            column = column.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("Tools.pick.detected", encoding = detected.to_string()).into()),
            );
        }
        column
    }

    fn render_out_dir_row(&self, form: &ToolForm, cx: &mut Context<Self>) -> impl IntoElement {
        let dir_text: SharedString = form.out_dir.as_ref().map_or_else(
            || t!("Tools.pick.out_dir_default").into(),
            |path| path.display().to_string().into(),
        );
        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(dir_text),
            )
            .child(
                Button::new(("tool-pick-out", self.kind.index() as u64))
                    .small()
                    .label(t!("Tools.pick.choose_folder").into())
                    .on_click(cx.listener(|this, _, _window, cx| this.pick_out_dir(cx))),
            )
    }

    fn render_options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let form = &self.forms[self.kind.index()];
        let mut panel = v_flex().gap_3();
        match self.kind {
            ToolKind::Convert => {
                let mut row = h_flex().gap_2();
                for (index, format) in [
                    ExportFormat::Epub,
                    ExportFormat::Txt,
                    ExportFormat::Html,
                    ExportFormat::Pdf,
                    ExportFormat::Markdown,
                ]
                .into_iter()
                .enumerate()
                {
                    let selected = form.target_format == format;
                    row = row.child(
                        Button::new(("tool-format", index as u64))
                            .small()
                            .ghost()
                            .selected(selected)
                            .label(t!(format_key(format)).into())
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.forms[ToolKind::Convert.index()].target_format = format;
                                cx.notify();
                            })),
                    );
                }
                panel = panel.child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().child(t!("Tools.convert.output_format").into()))
                        .child(row),
                );
                if form.source.as_deref().is_some_and(supports_chapter_regex) {
                    panel = panel.child(
                        v_flex()
                            .gap_2()
                            .child(div().text_sm().child(t!("Tools.convert.chapter_regex").into()))
                            .child(Input::new(&self.regex_input).w_full()),
                    );
                }
            }
            ToolKind::Encoding | ToolKind::Zhconv => {}
        }
        panel
    }

    fn render_actions(&self, form: &ToolForm, cx: &mut Context<Self>) -> impl IntoElement {
        let label: SharedString = if form.running {
            t!("Tools.run.running").into()
        } else {
            t!("Tools.run.start").into()
        };
        let mut column = v_flex().gap_2().child(
            Button::new(("tool-run", self.kind.index() as u64))
                .label(label)
                .loading(form.running)
                .disabled(form.source.is_none() || form.running)
                .on_click(cx.listener(|this, _, _window, cx| this.run_tool(cx))),
        );
        if let Some(result) = &form.result {
            let result = result.clone();
            column = column.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(result.display().to_string()),
                    )
                    .child(
                        Button::new("tool-reveal")
                            .small()
                            .ghost()
                            .label(t!("Tools.run.reveal").into())
                            .on_click(cx.listener(move |_this, _, _window, _cx| {
                                let _ = crate::utils::system::reveal_in_folder(&result);
                            })),
                    ),
            );
        }
        if let Some((summary, detail)) = &form.error {
            column = column.child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(summary.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(detail.clone()),
                    ),
            );
        }
        column
    }
}

impl Render for ToolsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(self.render_rail(cx))
            .child(self.render_form(cx))
    }
}

fn supports_chapter_regex(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("txt" | "md" | "markdown" | "html" | "htm")
    )
}

fn normalize_encoding(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty() && trimmed != "auto").then(|| trimmed.to_string())
}

const fn format_key(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Epub => "Tools.format.epub",
        ExportFormat::Txt => "Tools.format.txt",
        ExportFormat::Html => "Tools.format.html",
        ExportFormat::Pdf => "Tools.format.pdf",
        ExportFormat::Markdown => "Tools.format.markdown",
    }
}
```

- [ ] **Step 5: 补 i18n**

`locales/app.yml` 的 `Nav` 段在 `sources` 后插入：

```yaml
    tools: {en: Tools, zh-CN: 工具箱, zh-TW: 工具箱}
```

在 `Nav` 段之后、`Toasts` 段之前新增 `Tools` 段（本任务先加页面骨架、格式转换、通用动作与错误文案；编码 / 简繁选项文案在 Task 12 补齐）：

```yaml
  Tools:
    page_title: {en: "Toolbox", zh-CN: "工具箱", zh-TW: "工具箱"}
    page_subtitle: {en: "Convert local novel files", zh-CN: "转换本地小说文件", zh-TW: "轉換本地小說檔案"}
    kind:
      convert: {en: "Format conversion", zh-CN: "格式转换", zh-TW: "格式轉換"}
      encoding: {en: "Encoding", zh-CN: "编码转换", zh-TW: "編碼轉換"}
      zhconv: {en: "Simplified / Traditional", zh-CN: "简繁转换", zh-TW: "簡繁轉換"}
    desc:
      convert: {en: "TXT / Markdown / HTML / EPUB / ZIP to EPUB, TXT, HTML, PDF or Markdown", zh-CN: "TXT / Markdown / HTML / EPUB / ZIP 转为 EPUB、TXT、HTML、PDF 或 Markdown", zh-TW: "TXT / Markdown / HTML / EPUB / ZIP 轉為 EPUB、TXT、HTML、PDF 或 Markdown"}
      encoding: {en: "Re-encode a TXT file to UTF-8, GBK or Big5", zh-CN: "把 TXT 重新编码为 UTF-8、GBK 或 Big5", zh-TW: "把 TXT 重新編碼為 UTF-8、GBK 或 Big5"}
      zhconv: {en: "Convert TXT / Markdown between Simplified and Traditional Chinese", zh-CN: "在简体与繁体之间转换 TXT / Markdown", zh-TW: "在簡體與繁體之間轉換 TXT / Markdown"}
    pick:
      filter: {en: "Novel files", zh-CN: "小说文件", zh-TW: "小說檔案"}
      title: {en: "Choose a novel file", zh-CN: "选择小说文件", zh-TW: "選擇小說檔案"}
      none: {en: "No file selected", zh-CN: "尚未选择文件", zh-TW: "尚未選擇檔案"}
      choose_file: {en: "Choose file", zh-CN: "选择文件", zh-TW: "選擇檔案"}
      choose_folder: {en: "Choose output folder", zh-CN: "选择输出目录", zh-TW: "選擇輸出目錄"}
      detected: {en: "Detected encoding: %{encoding}", zh-CN: "检测到编码：%{encoding}", zh-TW: "偵測到編碼：%{encoding}"}
      out_dir_default: {en: "Output folder defaults to the source folder", zh-CN: "输出目录默认与源文件相同", zh-TW: "輸出目錄預設與來源檔案相同"}
    convert:
      output_format: {en: "Output format", zh-CN: "输出格式", zh-TW: "輸出格式"}
      chapter_regex: {en: "Chapter heading regex (optional)", zh-CN: "章节标题正则（可选）", zh-TW: "章節標題正則（可選）"}
      chapter_regex_placeholder: {en: "Leave blank to use the built-in 第N章 pattern", zh-CN: "留空使用内置“第N章”规则", zh-TW: "留空使用內建「第N章」規則"}
    format:
      epub: {en: EPUB, zh-CN: EPUB, zh-TW: EPUB}
      txt: {en: TXT, zh-CN: TXT, zh-TW: TXT}
      html: {en: HTML, zh-CN: HTML, zh-TW: HTML}
      pdf: {en: PDF, zh-CN: PDF, zh-TW: PDF}
      markdown: {en: Markdown, zh-CN: Markdown, zh-TW: Markdown}
    run:
      start: {en: "Convert", zh-CN: "开始转换", zh-TW: "開始轉換"}
      running: {en: "Converting…", zh-CN: "转换中…", zh-TW: "轉換中…"}
      reveal: {en: "Show in folder", zh-CN: "打开所在文件夹", zh-TW: "開啟所在資料夾"}
    toast:
      success: {en: "Saved to %{name}", zh-CN: "已保存到 %{name}", zh-TW: "已儲存到 %{name}"}
    error:
      unsupported_format: {en: "Unsupported file format", zh-CN: "不支持的文件格式", zh-TW: "不支援的檔案格式"}
      io: {en: "Could not read or write the file", zh-CN: "文件读写失败", zh-TW: "檔案讀寫失敗"}
      decode: {en: "Could not decode the file", zh-CN: "文件解码失败", zh-TW: "檔案解碼失敗"}
      invalid_regex: {en: "Invalid chapter regex", zh-CN: "章节正则无效", zh-TW: "章節正則無效"}
      zip_structure: {en: "Malformed ZIP archive", zh-CN: "ZIP 结构异常", zh-TW: "ZIP 結構異常"}
      epub_structure: {en: "Malformed EPUB file", zh-CN: "EPUB 结构异常", zh-TW: "EPUB 結構異常"}
      no_content: {en: "No chapter text found", zh-CN: "文件没有可转换的正文", zh-TW: "檔案沒有可轉換的正文"}
      unencodable: {en: "Target encoding cannot represent every character", zh-CN: "目标编码无法表示全部字符", zh-TW: "目標編碼無法表示全部字元"}
      unknown_encoding: {en: "Unknown encoding", zh-CN: "未知编码", zh-TW: "未知編碼"}
      export: {en: "Export failed", zh-CN: "导出失败", zh-TW: "匯出失敗"}
```

- [ ] **Step 6: 编译检查**

Run: `cargo check --all-targets`
Expected: 编译通过（无 warning 视为通过；有 warning 按 `cargo clippy` 规则处理）。

- [ ] **Step 7: 提交**

```bash
git add src/desktop locales/app.yml
git commit -m "feat(desktop): 工具箱页面与格式转换 UI"
```

---

### Task 12: 编码转换与简繁转换选项

**Files:**
- Modify: `src/desktop/pages/tools/tools_page.rs`
- Modify: `locales/app.yml`

**Interfaces:**
- Consumes: Task 11 的 `ToolsPage` / `ToolForm` / `chip_row` 骨架。
- Produces: `ToolKind::Encoding` 与 `ToolKind::Zhconv` 的选项区；TXT 目标格式下的输出编码选项。

- [ ] **Step 1: 增加编码常量与可复用 chip 行**

`src/desktop/pages/tools/tools_page.rs` 的 `SOURCE_EXTENSIONS` 后追加：

```rust
/// 源文件编码选项（`auto` 交给 BOM / chardetng 探测）。
const SOURCE_ENCODINGS: &[&str] = &["auto", "UTF-8", "GBK", "GB18030", "Big5", "Big5-HKSCS"];
/// 可写出的目标编码；UTF-16 在 encoding_rs 中只支持解码，故意不列入。
const TARGET_ENCODINGS: &[&str] = &["UTF-8", "GBK", "GB18030", "Big5", "Big5-HKSCS"];
```

文件末尾（`format_key` 之后）追加：

```rust
/// 渲染一行单选 chip；`on_pick` 在主线程回调里更新对应表单字段。
fn chip_row(
    prefix: &'static str,
    options: &'static [&'static str],
    selected: &str,
    on_pick: impl Fn(&mut ToolsPage, String) + 'static,
    cx: &mut Context<ToolsPage>,
) -> impl IntoElement {
    let on_pick: std::rc::Rc<dyn Fn(&mut ToolsPage, String)> = std::rc::Rc::new(on_pick);
    let mut row = h_flex().gap_2();
    for (index, option) in options.iter().enumerate() {
        let value = (*option).to_string();
        let on_pick = on_pick.clone();
        row = row.child(
            Button::new((prefix, index as u64))
                .small()
                .ghost()
                .selected(selected == value)
                .label(*option)
                .on_click(cx.listener(move |this, _, _window, cx| {
                    on_pick(this, value.clone());
                    cx.notify();
                })),
        );
    }
    row
}
```

- [ ] **Step 2: 补全 `render_options`**

把 `render_options` 的 `match self.kind` 改为：

```rust
        match self.kind {
            ToolKind::Convert => {
                let mut row = h_flex().gap_2();
                for (index, format) in [
                    ExportFormat::Epub,
                    ExportFormat::Txt,
                    ExportFormat::Html,
                    ExportFormat::Pdf,
                    ExportFormat::Markdown,
                ]
                .into_iter()
                .enumerate()
                {
                    let selected = form.target_format == format;
                    row = row.child(
                        Button::new(("tool-format", index as u64))
                            .small()
                            .ghost()
                            .selected(selected)
                            .label(t!(format_key(format)).into())
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.forms[ToolKind::Convert.index()].target_format = format;
                                cx.notify();
                            })),
                    );
                }
                panel = panel.child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().child(t!("Tools.convert.output_format").into()))
                        .child(row),
                );
                if form.target_format == ExportFormat::Txt {
                    panel = panel.child(
                        v_flex()
                            .gap_2()
                            .child(div().text_sm().child(t!("Tools.convert.txt_encoding").into()))
                            .child(chip_row(
                                "tool-txt-enc",
                                TARGET_ENCODINGS,
                                &form.txt_encoding,
                                |page, value| {
                                    page.forms[ToolKind::Convert.index()].txt_encoding = value;
                                },
                                cx,
                            )),
                    );
                }
                if form.source.as_deref().is_some_and(supports_chapter_regex) {
                    panel = panel
                        .child(
                            v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .child(t!("Tools.encoding.source_encoding").into()),
                                )
                                .child(chip_row(
                                    "tool-conv-src-enc",
                                    SOURCE_ENCODINGS,
                                    &form.source_encoding,
                                    |page, value| {
                                        page.forms[ToolKind::Convert.index()].source_encoding =
                                            value;
                                    },
                                    cx,
                                )),
                        )
                        .child(
                            v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_sm()
                                        .child(t!("Tools.convert.chapter_regex").into()),
                                )
                                .child(Input::new(&self.regex_input).w_full()),
                        );
                }
            }
            ToolKind::Encoding => {
                panel = panel
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("Tools.encoding.source_encoding").into()),
                            )
                            .child(chip_row(
                                "tool-src-enc",
                                SOURCE_ENCODINGS,
                                &form.source_encoding,
                                |page, value| {
                                    page.forms[ToolKind::Encoding.index()].source_encoding = value;
                                },
                                cx,
                            )),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .child(t!("Tools.encoding.target_encoding").into()),
                            )
                            .child(chip_row(
                                "tool-dst-enc",
                                TARGET_ENCODINGS,
                                &form.target_encoding,
                                |page, value| {
                                    page.forms[ToolKind::Encoding.index()].target_encoding = value;
                                },
                                cx,
                            )),
                    );
            }
            ToolKind::Zhconv => {
                let mut row = h_flex().gap_2();
                for (index, (target, key)) in [
                    (LangType::ZhCn, "Tools.zhconv.zh_cn"),
                    (LangType::ZhTw, "Tools.zhconv.zh_tw"),
                    (LangType::ZhHant, "Tools.zhconv.zh_hant"),
                ]
                .into_iter()
                .enumerate()
                {
                    row = row.child(
                        Button::new(("tool-zhconv", index as u64))
                            .small()
                            .ghost()
                            .selected(form.zhconv_target == target)
                            .label(t!(key).into())
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.forms[ToolKind::Zhconv.index()].zhconv_target = target;
                                cx.notify();
                            })),
                    );
                }
                panel = panel.child(
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().child(t!("Tools.zhconv.target").into()))
                        .child(row),
                );
            }
        }
```

- [ ] **Step 3: 补 i18n**

在 `locales/app.yml` 的 `Tools.convert` 段追加：

```yaml
      txt_encoding: {en: "TXT encoding", zh-CN: "TXT 编码", zh-TW: "TXT 編碼"}
```

在 `Tools.format` 段之后新增：

```yaml
    encoding:
      source_encoding: {en: "Source encoding", zh-CN: "源编码", zh-TW: "來源編碼"}
      target_encoding: {en: "Target encoding", zh-CN: "目标编码", zh-TW: "目標編碼"}
    zhconv:
      target: {en: "Target script", zh-CN: "目标字形", zh-TW: "目標字形"}
      zh_cn: {en: "Simplified (China)", zh-CN: "简体（大陆）", zh-TW: "簡體（大陸）"}
      zh_tw: {en: "Traditional (Taiwan)", zh-CN: "繁体（台湾）", zh-TW: "繁體（台灣）"}
      zh_hant: {en: "Traditional (Hong Kong)", zh-CN: "繁体（香港）", zh-TW: "繁體（香港）"}
```

- [ ] **Step 4: 运行页面的格式与编译检查**

Run: `cargo fmt --all && cargo check --all-targets`
Expected: 编译通过。

- [ ] **Step 5: 提交**

```bash
git add src/desktop/pages/tools locales/app.yml
git commit -m "feat(desktop): 工具箱编码与简繁选项"
```

---

### Task 13: 全量验证与手工验收

**Files:**
- Modify: `tasks/todo.md`（实现清单 + review 段；若文件不存在则创建）

**Interfaces:**
- Consumes: 前 12 个任务的全部产物。
- Produces: 可交付的工具箱功能与验收记录。

- [ ] **Step 1: 静态检查**

Run: `cargo fmt --all --check`
Expected: 无输出（退出码 0）。

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 0 warning / 0 error。

- [ ] **Step 2: 全量测试**

Run: `cargo test`
Expected: 全部通过，包括新增的 `tools::*` 与改造后的 `export::*` 测试。

- [ ] **Step 3: 手工验收（GUI）**

Run: `cargo run --bin so-novel-rs`（`Cargo.toml` 的 `[[bin]]` 名称）。

逐项确认：

1. 侧边栏出现「工具箱」，位于「书源管理」与「设置」之间；`Cmd+5` 进工具箱，`Cmd+6` 进设置，`F6` 循环 6 页。
2. 准备 `test.txt`（内容含 `第一章 起航` / 正文 / `第二章 到达` / 正文）：
   - 选文件后路径显示，检测编码显示 `UTF-8`，输出目录默认是文件所在目录。
   - 选 EPUB → 运行 → 生成 `test(佚名).epub`，用阅读器打开确认两章、目录、正文正常。
   - 选 PDF → 运行 → 生成 PDF，确认中文不出现方框。
   - 选 Markdown → 运行 → 生成 `.md`，确认 `## 第一章 起航` 与正文。
3. 准备 GBK 编码的 `gbk.txt`：编码工具 → 目标 UTF-8 → 生成 `gbk (UTF-8).txt`，用文本编辑器确认中文正常。
4. 准备简体 `fan.txt`：简繁工具 → 繁体（台湾） → 生成 `fan (繁体).txt`，确认「國家」等字形已转换且编码与源文件一致。
5. 错误路径：选择一个 `.docx`（手动改扩展名或从文件对话框输入路径）→ 运行 → 出现红色错误摘要 + toast，应用不崩溃。
6. 切换语言到 English / 繁體中文 → 重启 → 工具箱文案完整，无 i18n key 泄漏（界面不出现 `Tools.` 字样）。

- [ ] **Step 4: 记录 todo / review**

在 `tasks/todo.md` 末尾记录：完成项清单、上述手工验收结果、发现的问题与后续 TODO（若有）。若实现过程中用户做过纠正，同步更新 `tasks/lessons.md`。

- [ ] **Step 5: 提交**

```bash
git add tasks/todo.md tasks/lessons.md
git commit -m "docs(tasks): 工具箱实现验收记录"
```
