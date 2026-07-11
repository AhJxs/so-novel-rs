# Markdown 导出格式支持 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `ExportFormat::Markdown`，把小说导出为单文件 `.md`（带 Hugo/Jekyll YAML front matter + 章节锚点 TOC），桌面 / Web / CLI 三处入口全部支持。

**Architecture:** 复用现有 `render → write_chapter_files → Exporter::merge` 三段式管线。新增 `RenderTarget::Markdown` 变体 + `render_md()` 函数 + `.md` 文件名分支 + `MdExporter`（`src/export/md.rs` 新文件）+ 三处 UI 选项（桌面 `fields.rs`/`page_general.rs`、Web `settings.tsx`/`types.ts`、CLI `value_name`）。零新增 crate 依赖，零 `Cargo.toml` 改动。

**Tech Stack:** Rust + `regex`（既有）+ `zhconv`（既有）+ `serde`（既有）+ rust_i18n（既有）+ React + TypeScript。

## File Structure

| 文件 | 变更 | 职责 |
|------|------|------|
| `src/config/types.rs` | modify (+enum variant + 2 match arms) | `ExportFormat` 加 `Markdown` |
| `src/export/render.rs` | modify (+RenderTarget variant + render_md + zhconv branch + tests) | 章节 → Markdown 字符串 |
| `src/export/exporter.rs` | modify (+`.md` filename 分支 ×2 + 1 test) | 章节缓存命名 |
| `src/export/md.rs` | create（`MdExporter` impl + tests） | YAML front matter + TOC + 合并到单 `.md` |
| `src/export/mod.rs` | modify (`pub mod md;`) | 模块导出 |
| `src/desktop/pages/settings/fields.rs` | modify (+`Markdown` 分支 ×2) | 桌面枚举 ↔ 字符串桥 |
| `src/desktop/pages/settings/page_general.rs` | modify (+1 行 `ext_options`) | 桌面下拉项 |
| `src/cli/args.rs` | modify (`value_name` 字符串追加) | CLI help 文本 |
| `web-ui/src/routes/settings.tsx` | modify (`FORMAT_OPTIONS` 追加) | Web 选项数组 |
| `web-ui/src/lib/types.ts` | modify (`ExportFormat` 联合追加) | Web 类型 |

零新增 crate 依赖，零 `Cargo.toml` 改动。

## Global Constraints

来自 spec：
- `Markdown` 默认仍是 `Epub`（spec §1 明示），不挪 `#[default]`
- `parse()` 未知值回落 `Epub` 不变（spec §1）
- 章节 `.md` 文件名格式 `{order}_{safe_title}.md`（与 TXT 同形态；spec §3）
- Markdown 是 UTF-8 only；UI 不主动禁用 `txt_encoding` 下拉（避免 UI 抖动），`MdExporter` 路径不读 `txt_encoding`（spec §6）
- 锚点用 `<a id="chapter-N"></a>` HTML 形式（不 slugify；spec §10）
- 末尾保留一个 `\n`（与 `render_txt` 一致）
- 文件末尾 LF；项目提交前会自动 git core.autocrlf → CRLF
- i18n 不增 key（label 直接用 `md`，spec 触达清单明示）

---

## Task 1: TDD — `ExportFormat` 加 `Markdown` 变体

**Files:**
- Modify: `src/config/types.rs:9-38`（在 enum 末尾、`as_lower` / `parse` 末尾各加 1 行）
- Test: `src/config/tests.rs` 现有顶层测试 module（追加 3 个 `parse` 单测）

**Interfaces:**
- Consumes: 既有 `Serialize` / `Deserialize` / `Default` 派生（无变化）
- Produces:
  - `pub enum ExportFormat { Epub, Txt, Html, Pdf, Markdown }` —— 新增最后一项
  - `as_lower()` 新增 `Self::Markdown => "markdown"`
  - `parse()` 新增 `"markdown" => Self::Markdown`（回落仍是 `Epub`）

- [ ] **Step 1: 写 3 个失败测试**

打开 `src/config/tests.rs`，文件末尾（`save_config_writes_to_new_path` 测试之后）追加：

```rust
#[test]
fn export_format_parse_accepts_markdown_lowercase() {
    assert_eq!(ExportFormat::parse("markdown"), ExportFormat::Markdown);
}

#[test]
fn export_format_parse_is_case_insensitive_for_markdown() {
    assert_eq!(ExportFormat::parse("MARKDOWN"), ExportFormat::Markdown);
    assert_eq!(ExportFormat::parse("  Markdown  "), ExportFormat::Markdown);
}

#[test]
fn export_format_parse_falls_back_to_epub_for_unknown() {
    // 既有行为不变：未知值回落 Epub（默认）
    assert_eq!(ExportFormat::parse("not-a-format"), ExportFormat::Epub);
    assert_eq!(ExportFormat::parse(""), ExportFormat::Epub);
}
```

- [ ] **Step 2: 跑测试确认 3 个都因"找不到 `ExportFormat::Markdown` variant"编译失败**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib config::tests::export_format_parse --no-run 2>&1 | tail -20`
Expected: 编译错误 —— `no variant or associated item named Markdown found for enum ExportFormat`。这是想要的红。

- [ ] **Step 3: 实现 —— 修改 `src/config/types.rs`**

在 enum 定义（line 9-18）末尾追加 `Markdown` 变体：

```rust
/// 导出文件格式。EPUB / TXT / HTML / PDF / Markdown。
#[derive(Debug, Copy, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
    #[default]
    Epub,
    Txt,
    Html,
    /// 阶段一不实现 PDF 导出，仅保留枚举以便兼容旧配置，
    /// UI 选择 PDF 时会显示提示并降级。详见 audit §6.4。
    Pdf,
    /// Markdown 单文件输出（`.md`），UTF-8 only。详见 docs/superpowers/specs/2026-07-11-markdown-export-design.md。
    Markdown,
}
```

`as_lower()`（line 20-28）末尾追加：

```rust
pub const fn as_lower(self) -> &'static str {
    match self {
        Self::Epub => "epub",
        Self::Txt => "txt",
        Self::Html => "html",
        Self::Pdf => "pdf",
        Self::Markdown => "markdown",
    }
}
```

`parse()`（line 30-37）末尾追加（注意：`to_ascii_lowercase()` 已统一大小写）：

```rust
pub fn parse(s: &str) -> Self {
    match s.trim().to_ascii_lowercase().as_str() {
        "txt" => Self::Txt,
        "html" => Self::Html,
        "pdf" => Self::Pdf,
        "markdown" => Self::Markdown,
        _ => Self::Epub,
    }
}
```

- [ ] **Step 4: 跑测试确认 3 个新测通过，且已有测试无回归**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib config::tests::`
Expected: `test config::tests::export_format_parse_accepts_markdown_lowercase ... ok` + 3 others ✓；既有 `loads_default_when_missing` 等 11 个测试不变通过。

`loads_default_when_missing` 验证：默认仍是 `Epub`（`assert_eq!(cfg.crawl.min_interval, 200)` 不动；只是我们没新增 `*_default_markdown` 这类断言 —— 因为 spec 明示默认不挪）。

- [ ] **Step 5: 提交**

Run:

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add src/config/types.rs src/config/tests.rs && \
git commit -m "feat(config): add ExportFormat::Markdown variant + parse/as_lower arms" \
  -m "Third new variant paired with lowercase key handling: parse() is" \
  -m "case-insensitive via trim+to_ascii_lowercase; unknown values still" \
  -m "fall back to Epub. Pure addition: existing serde-serialized" \
  -m "config.toml files using Epub/Txt/Html/Pdf round-trip unchanged." \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 2: TDD — `RenderTarget::Markdown` 变体 + `render_md()` 函数

**Files:**
- Modify: `src/export/render.rs`（3 处：`RenderTarget` enum、`From<ExportFormat>` impl、`render_chapter` body）+ 新增 `render_md` 函数 + 简繁分支 + 5 个新测试

**Interfaces:**
- Consumes: `ExportFormat::Markdown`（Task 1）、`filter_chapter` / `format_chapter`（既有）
- Produces:
  - `pub enum RenderTarget { Txt, Html, Epub, Pdf, Markdown }` —— 新增最后一项
  - `From<ExportFormat> for RenderTarget` 新增 `ExportFormat::Markdown => Self::Markdown`
  - `pub fn render_md(title: &str, p_html: &str) -> String` —— 输出 `## {title}\n\n` + 每段 `trim()` 后 `\n\n`，无 `<p>` 兜底整段 trim 当一段，末尾保留一个 `\n`
  - `maybe_convert_chinese` 新增 `RenderTarget::Markdown => convert_text(&body, ...)` 分支（与 Txt 同路径）

- [ ] **Step 1: 写 5 个失败测试**

打开 `src/export/render.rs`，文件末尾（`render_skips_conversion_when_source_unparseable` 测试之后，`}` 闭合 `mod tests` 之前）追加：

```rust
    // ---------- Markdown ----------

    /// 闭合 `<p>` 抽段 → 输出 `## 标题` + 段落 + `## 标题` 行单独。
    #[test]
    fn render_md_extracts_paragraphs_with_h2_heading() {
        let raw = Chapter {
            url: "https://x/c.html".into(),
            title: "第1章 起航".into(),
            content: "<p>段一</p><p>段二</p>".into(),
            order: 1,
        };
        let (title, body) = render(&raw, &rule_closed_with_ad(), RenderTarget::Markdown);
        assert_eq!(title, "第1章 起航");
        // 首行是 H2 标题
        assert!(
            body.starts_with("## 第1章 起航\n\n"),
            "body should start with H2 title, got: {body}"
        );
        // 段一、段二 各占段，段之间 `\n\n`
        assert!(body.contains("段一"));
        assert!(body.contains("段二"));
    }

    /// 空 `<p>` 跳过（与 TXT 行为一致：matched=true 但 inner 为空时不 push）。
    #[test]
    fn render_md_skips_empty_paragraphs() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: "第1章".into(),
            content: "<p>a</p><p>   </p><p>b</p>".into(),
            order: 1,
        };
        let (_t, body) = render(&raw, &rule_closed_with_ad(), RenderTarget::Markdown);
        assert!(body.contains("a"));
        assert!(body.contains("b"));
        // 空白段不应作为独立段落出现（H2 标题之外不应有连续空行）
        assert!(!body.contains("a\n\n\nb"), "空 <p> 被错留了一段空白: {body}");
    }

    /// 无 `<p>` 兜底：把整段 HTML trim 当一段。
    #[test]
    fn render_md_falls_back_to_whole_html_when_no_p_tags() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: "第1章".into(),
            content: "<div>裸 HTML 兜底</div>".into(),
            order: 1,
        };
        let (_t, body) = render(&raw, &rule_closed_with_ad(), RenderTarget::Markdown);
        assert!(body.starts_with("## 第1章\n\n"));
        assert!(
            body.contains("裸 HTML 兜底"),
            "expected fallback paragraph, got: {body}"
        );
    }

    /// 末尾保留一个 `\n`（与 `render_txt` 风格一致，便于拼接 TOC / 下一章）。
    #[test]
    fn render_md_trailing_newline() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: "末章".into(),
            content: "<p>完</p>".into(),
            order: 1,
        };
        let (_t, body) = render(&raw, &rule_closed_with_ad(), RenderTarget::Markdown);
        assert!(body.ends_with('\n'), "expected trailing \\n, got: {body:?}");
    }

    /// Markdown 走 `convert_text` 路径（与 TXT 同），不走 `convert_html_body`。
    #[test]
    fn render_md_converts_simplified_to_traditional_tw() {
        let raw = Chapter {
            url: "https://x/".into(),
            title: "头发".into(),
            content: "<p>头发的颜色</p>".into(),
            order: 1,
        };
        let (_t, body) = render_chapter(
            &raw,
            &RuleChapter::default(),
            RenderTarget::Markdown,
            "zh_CN",
            LangType::ZhTw,
        );
        assert!(body.contains("頭髮"), "got: {body}");
        assert!(body.contains("顏色"), "got: {body}");
    }
```

- [ ] **Step 2: 跑测试，确认它们因找不到 `RenderTarget::Markdown` 编译失败**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib export::render::tests::render_md --no-run 2>&1 | tail -20`
Expected: 编译错误 —— `no variant or associated item named Markdown found for enum RenderTarget`。这是想要的红。

- [ ] **Step 3: 增 `RenderTarget::Markdown` 变体**

打开 `src/export/render.rs`，在 line 22-29 的 `RenderTarget` enum 末尾追加：

```rust
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RenderTarget {
    Txt,
    Html,
    Epub,
    /// 章节文件沿用 Html 模板写到 `chapters_dir`，由 `PdfExporter` 读取并合成 PDF。
    Pdf,
    /// Markdown 单文件输出（章节文件 = 标题 `##` + 段落 + `chapter-N` 锚点）。
    Markdown,
}
```

`From<ExportFormat> for RenderTarget`（line 31-40）末尾追加：

```rust
impl From<ExportFormat> for RenderTarget {
    fn from(f: ExportFormat) -> Self {
        match f {
            ExportFormat::Txt => Self::Txt,
            ExportFormat::Html => Self::Html,
            ExportFormat::Epub => Self::Epub,
            ExportFormat::Pdf => Self::Pdf,
            ExportFormat::Markdown => Self::Markdown,
        }
    }
}
```

`render_chapter` 内部 `match target`（line 62-74）末尾追加（在 `RenderTarget::Epub` 分支之后）：

```rust
        RenderTarget::Markdown => render_md(&filtered.title, &formatted_html),
```

`maybe_convert_chinese` body 分支（line 96-102）末尾追加（注意：Markdown 是纯文本，与 Txt 同走 `convert_text`，不走 `convert_html_body`）：

```rust
        RenderTarget::Txt => convert_text(&body, &target_lang),
        RenderTarget::Markdown => convert_text(&body, &target_lang),
        RenderTarget::Html | RenderTarget::Epub | RenderTarget::Pdf => {
            convert_html_body(&body, &target_lang)
        }
```

- [ ] **Step 4: 实现 `render_md()` 函数**

插入位置：`render_txt` 函数（line 108-151）**之后**、`const TITLE_PLACEHOLDER`（line 157）**之前**。两份都是模块级自由函数，互不引用，顺序随意；选这里是为读源码的人从"输入解析 → TXT → MD → 模板"按渲染器复杂度递增顺序看下去。

```rust
/// Markdown：从 `<p>...</p>` 中抽段落文字，标题前缀 `##`，段落间双换行。
///
/// 输出形态：
/// ```text
/// ## {title}
///
/// 段一
///
/// 段二
///
/// ```
/// 末尾保留一个 `\n`，便于与 TOC / 下一章拼接。
fn render_md(title: &str, p_html: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    /// 编译期确定的正则：用 match 走 panic 路径以避免 `clippy::expect_used`。
    /// panic IS the design：源码字面量写错就是程序员错误。
    #[allow(
        clippy::panic,
        reason = "static regex literal must compile; failure = programmer error"
    )]
    fn compile_static_re(pattern: &'static str) -> Regex {
        match Regex::new(pattern) {
            Ok(re) => re,
            Err(e) => panic!("static regex `{pattern}` should compile: {e}"),
        }
    }

    static P_RE: LazyLock<Regex> = LazyLock::new(|| compile_static_re(r"(?s)<p>(.*?)</p>"));

    let mut sb = String::with_capacity(p_html.len());
    // H2 标题（front matter 之外的"书标题"由 merge 阶段单独生成 H1）。
    sb.push_str("## ");
    sb.push_str(title);
    sb.push_str("\n\n");

    let mut matched = false;
    for cap in P_RE.captures_iter(p_html) {
        matched = true;
        let inner = cap.get(1).map_or("", |m| m.as_str());
        let trimmed = inner.trim();
        if trimmed.is_empty() {
            // 空段跳过：避免连续空行影响下游 merge 的 `\n\n` 计数
            continue;
        }
        sb.push_str(trimmed);
        sb.push_str("\n\n");
    }
    if !matched {
        // 无 <p> 时直接把整段当一段（极端兜底）
        let s = p_html.trim();
        if !s.is_empty() {
            sb.push_str(s);
            sb.push_str("\n\n");
        }
    }
    // 末尾保留一个尾随 \n（与 render_txt 一致）
    sb.push('\n');
    sb
}
```

- [ ] **Step 5: 跑测试，5 个新测全过、18 个既有测无回归**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib export::render::`
Expected: `test export::render::tests::render_md_extracts_paragraphs_with_h2_heading ... ok` + 4 others ✓；既有 `render_txt_extracts_paragraphs_with_indent` 等 18 个测试不变通过。

- [ ] **Step 6: 提交**

Run:

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add src/export/render.rs && \
git commit -m "feat(export): add RenderTarget::Markdown + render_md() per spec" \
  -m "Outputs '## title\\n\\n' + paragraph blocks separated by '\\n\\n'," \
  -m "matching render_txt behavior (empty <p> skipped, no-<p> fallback," \
  -m "trailing \\n kept). zhconv routing matches Txt (convert_text)" \
  -m "since Markdown is plain text — convert_html_body would mangle" \
  -m "raw Chinese." \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 3: TDD — 章节缓存 `.md` 文件名分支

**Files:**
- Modify: `src/export/exporter.rs:104-108`（`write_chapter_files` 的 filename match）+ `:129-133`（`write_single_chapter` 的 filename match）+ 现有 `mod tests` 追加 1 个测试

**Interfaces:**
- Consumes: `ExportFormat::Markdown`（Task 1）
- Produces:
  - `write_chapter_files(..., ExportFormat::Markdown)` 写出 `{order}_{safe_title}.md`
  - `write_single_chapter(..., ExportFormat::Markdown, ...)` 返回 `{dir}/{order}_{safe_title}.md`
  - 测试：`write_chapter_files(chapters, ExportFormat::Markdown)` 产出 `001_起航.md`

- [ ] **Step 1: 写失败测试**

打开 `src/export/exporter.rs`，在文件末尾（`unique_path_handles_filename_without_extension` 测试之后，`}` 闭合 `mod tests` 之前）追加：

```rust
    #[test]
    fn write_chapter_files_creates_titled_filenames_for_md() {
        let dir = tempfile::tempdir().unwrap();
        let rendered = vec![RenderedChapter {
            order: 5,
            title: "起航".into(),
            body: "## 起航\n\n正文\n\n".into(),
        }];
        write_chapter_files(dir.path(), &rendered, ExportFormat::Markdown).unwrap();
        assert!(dir.path().join("005_起航.md").exists());
    }

    #[test]
    fn write_single_chapter_uses_md_extension() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_single_chapter(dir.path(), 3, "楔子", "body", ExportFormat::Markdown, 10).unwrap();
        assert_eq!(path, dir.path().join("003_楔子.md"));
    }
```

- [ ] **Step 2: 跑测试，确认因"match 不穷尽"编译失败**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib export::exporter::tests::write_chapter_files_creates_titled_filenames_for_md --no-run 2>&1 | tail -20`
Expected: 编译错误 —— `match arms not exhaustive, consider adding Markdown`. 这是想要的红。

- [ ] **Step 3: 在两个 filename match 末尾追加 `Markdown` 分支**

`write_chapter_files`（line 102-108）末尾追加：

```rust
        let filename = match format {
            ExportFormat::Html | ExportFormat::Pdf => format!("{order}_.html"),
            ExportFormat::Txt => format!("{order}_{safe_title}.txt"),
            ExportFormat::Epub => format!("{order}_{safe_title}.html"),
            ExportFormat::Markdown => format!("{order}_{safe_title}.md"),
        };
```

`write_single_chapter`（line 129-133）末尾追加：

```rust
        let filename = match format {
            ExportFormat::Html | ExportFormat::Pdf => format!("{order_str}_.html"),
            ExportFormat::Txt => format!("{order_str}_{safe_title}.txt"),
            ExportFormat::Epub => format!("{order_str}_{safe_title}.html"),
            ExportFormat::Markdown => format!("{order_str}_{safe_title}.md"),
        };
```

- [ ] **Step 4: 跑测试，新 2 个测通过、既有 12 个测无回归**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib export::exporter::`
Expected: `test exporter::tests::write_chapter_files_creates_titled_filenames_for_md ... ok` + 12 others ✓。

- [ ] **Step 5: 提交**

Run:

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add src/export/exporter.rs && \
git commit -m "feat(export): write_chapter_files emits .md under Markdown format" \
  -m "Mirrors the txt branch filename '{order}_{safe_title}.md' so a" \
  -m "chapter file alone is readable. write_single_chapter's same" \
  -m "branch ensures streaming chapter writers agree on the layout." \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 4: 创建 `MdExporter` + 模块注册 + 工厂分支

**Files:**
- Create: `src/export/md.rs`（`MdExporter` impl + 测试 + `chapter_title_from_path` 辅助函数）
- Modify: `src/export/mod.rs`（`pub mod md;`）
- Modify: `src/export/exporter.rs:72-79`（`exporter_for` 加 `Markdown` 分支）

**Interfaces:**
- Consumes: `Exporter` trait + `sort_chapter_files` / `strip_html_tags` / `unique_path`（既有 helper）+ `Book` / `Path`（既有）+ `sanitize_filename`（既有）
- Produces:
  - `pub struct MdExporter;` —— 与 `HtmlExporter` 同形态，无字段
  - `impl Exporter for MdExporter`：ext = `"md"`、`merge()` 输出 YAML front matter + H1 书名 + `## 目录` TOC + 每章正文前置 `<a id="chapter-N"></a>` 锚点
  - `pub(crate) fn chapter_title_from_path(path: &Path) -> String` —— 从 `001_第1章 起航.md` 抽 `第1章 起航`
  - 模块注册：`pub mod md;` + 工厂 `ExportFormat::Markdown => Box::new(super::md::MdExporter)`

- [ ] **Step 1: 写 7 个失败测试（创建空 `md.rs` 让编译红）**

新建 `src/export/md.rs`（内容仅模块 doc + 测试 —— 故意不实现 `MdExporter`，让下面整块测试因找不到类型而编译失败）：

```rust
//! Markdown 导出。对应单文件 `.md` 合并。
//!
//! 与 TXT 同形态：合并 `chapters_dir` 下每章 `.md` → 单文件。
//! 多出两点：
//! - YAML front matter（Hugo/Jekyll 风格）
//! - 章节锚点 TOC（`- [标题](#chapter-N)`）

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use std::path::PathBuf;

    use crate::config::ExportFormat;
    use crate::export::exporter::{
        ExportError, RenderedChapter, exporter_for, write_chapter_files,
    };
    use crate::models::Book;

    use super::super::md::MdExporter;

    fn sample_book() -> Book {
        Book {
            url: "https://x/".into(),
            book_name: "起航".into(),
            author: "苹果".into(),
            intro: Some("<p>这是&nbsp;简介</p>".into()),
            ..Book::default()
        }
    }

    fn sample_chapters() -> Vec<RenderedChapter> {
        vec![
            RenderedChapter {
                order: 1,
                title: "第1章 楔子".into(),
                body: "## 第1章 楔子\n\n\u{3000}\u{3000}正文一\n\n\u{3000}\u{3000}正文二\n".into(),
            },
            RenderedChapter {
                order: 2,
                title: "第2章 启程".into(),
                body: "## 第2章 启程\n\n\u{3000}\u{3000}正文三\n".into(),
            },
        ]
    }

    fn write_and_merge() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Markdown).unwrap();
        let exp = MdExporter;
        let p = exp.merge(&sample_book(), &chapters, &out).unwrap();
        (dir, p)
    }

    /// 工厂函数能找到 `MdExporter`（通过 `Exporter::ext()` 区分）。
    #[test]
    fn exporter_for_markdown_returns_md_exporter() {
        let exp = exporter_for(ExportFormat::Markdown, "UTF-8");
        assert_eq!(exp.ext(), "md");
    }

    /// 输出文件名 `<book_name>(<author>).md`，且包含 front matter 三件套。
    #[test]
    fn merge_writes_yaml_front_matter() {
        let (_dir, p) = write_and_merge();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(p.file_name().unwrap().to_str().unwrap().ends_with("(苹果).md"));
        let header_end = s.find("\n---\n").expect("should contain closing ---");
        let header = &s[..header_end];
        assert!(header.starts_with("---\n"));
        assert!(header.contains("title: 起航"));
        assert!(header.contains("author: 苹果"));
        // intro 已剥离 HTML 标签和实体后作为 description: |
        assert!(header.contains("description: |"));
        assert!(header.contains("  这是简介"));
    }

    /// 空 intro 时 front matter 不含 `description:`。
    #[test]
    fn merge_omits_description_when_intro_empty() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        write_chapter_files(&chapters, &sample_chapters(), ExportFormat::Markdown).unwrap();
        let book = Book {
            book_name: "无简介".into(),
            author: "无名".into(),
            intro: None,
            ..Book::default()
        };
        let p = MdExporter.merge(&book, &chapters, &out).unwrap();
        let s = std::fs::read_to_string(&p).unwrap();
        let header_end = s.find("\n---\n").unwrap();
        let header = &s[..header_end];
        assert!(!header.contains("description:"));
    }

    /// 顶部 H1 + 目录区段 + 每行 `- [标题](#chapter-N)`。
    #[test]
    fn merge_writes_h1_toc_and_anchor_links() {
        let (_dir, p) = write_and_merge();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("# 起航\n\n"), "缺少书名 H1");
        assert!(s.contains("## 目录\n\n"));
        assert!(s.contains("- [第1章 楔子](#chapter-1)"));
        assert!(s.contains("- [第2章 启程](#chapter-2)"));
    }

    /// 每章正文前嵌入 `<a id="chapter-N"></a>` HTML 锚点。
    #[test]
    fn merge_embeds_html_anchor_before_each_chapter() {
        let (_dir, p) = write_and_merge();
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("<a id=\"chapter-1\"></a>"));
        assert!(s.contains("<a id=\"chapter-2\"></a>"));
    }

    /// `chapter_title_from_path` 从 `001_第1章 起航.md` 抽 `第1章 起航`。
    #[test]
    fn chapter_title_from_path_strips_order_prefix() {
        use super::super::md::chapter_title_from_path;
        let s = chapter_title_from_path(std::path::Path::new("001_第1章 起航.md"));
        assert_eq!(s, "第1章 起航");
        // 没下划线时回退整 stem
        let s2 = chapter_title_from_path(std::path::Path::new("无名.md"));
        assert_eq!(s2, "无名");
    }

    /// 章节目录为空 → `EmptyChaptersDir` 错误（与 TXT 一致）。
    #[test]
    fn empty_chapters_dir_returns_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        let chapters = dir.path().join("chapters");
        let out = dir.path().join("out");
        std::fs::create_dir_all(&chapters).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        let err = MdExporter.merge(&sample_book(), &chapters, &out).unwrap_err();
        assert!(
            matches!(err, ExportError::EmptyChaptersDir(_)),
            "expected EmptyChaptersDir, got {err:?}"
        );
    }
}
```

- [ ] **Step 2: 模块注册 —— `src/export/mod.rs` 加 `pub mod md;`**

打开 `src/export/mod.rs`，按字母顺序在 `html` 和 `pdf` 之间插入：

```rust
pub mod html;
pub mod md;
pub mod pdf;
```

- [ ] **Step 3: 跑测试，确认 7 个测试因"模块未存在"编译失败**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib export::md::tests --no-run 2>&1 | tail -20`
Expected: 编译错误 —— `module 'md' not found in crate 'export'` 或 `cannot find type MdExporter`。这是想要的红。

- [ ] **Step 4: 实现 `MdExporter` —— 把 impl 添加到 `src/export/md.rs` 测试块上方**

打开 `src/export/md.rs`，Step 1 已建好文件但只有 `mod tests`。**保持 `mod tests` 块原封不动**；在它**之前**（文件顶部、`//!` 模块 doc 之后）插入以下完整实现代码：

```rust
//! Markdown 导出。对应单文件 `.md` 合并。
//!
//! 与 TXT 同形态：合并 `chapters_dir` 下每章 `.md` → 单文件。
//! 多出两点：
//! - YAML front matter（Hugo/Jekyll 风格）
//! - 章节锚点 TOC（`- [标题](#chapter-N)`）

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::export::exporter::{
    ExportError, Exporter, sort_chapter_files, strip_html_tags, unique_path,
};
use crate::models::Book;
use crate::utils::fs::sanitize_filename;

pub struct MdExporter;

impl Exporter for MdExporter {
    fn ext(&self) -> &'static str {
        "md"
    }

    fn merge(
        &self,
        book: &Book,
        chapters_dir: &Path,
        out_dir: &Path,
    ) -> Result<PathBuf, ExportError> {
        let files: Vec<PathBuf> = sort_chapter_files(chapters_dir)?
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| !s.starts_with("0_"))
            })
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("md"))
            })
            .collect();
        if files.is_empty() {
            return Err(ExportError::EmptyChaptersDir(chapters_dir.to_path_buf()));
        }

        // 单遍预读 (title, body)：避免后面写 TOC + 正文时两次 IO 打开同一文件。
        // 章节文件本身已驻留在磁盘，Vec 持有也算可接受（典型每章几 KB ~ 几百 KB）。
        let mut chapters: Vec<(String, String)> = Vec::with_capacity(files.len());
        for path in &files {
            let title = chapter_title_from_path(path);
            let body = std::fs::read_to_string(path)?;
            chapters.push((title, body));
        }

        std::fs::create_dir_all(out_dir)?;
        let out_name = sanitize_filename(&format!("{}({}).md", book.book_name, book.author));
        let out_path = unique_path(out_dir, &out_name);

        // BufWriter 流式写入：避免一次性把整本书拼成大 String 占用堆。
        // Markdown 是纯 UTF-8，无编码转换需求。
        // 注：用 `write_all(format!(...).as_bytes())?` 而非 `writeln!(w, ...)`，因为
        // `writeln!` 返回 `fmt::Result` 而函数签名要 `Result<_, ExportError>`，而
        // `ExportError` 只 derive 了 `From<std::io::Error>`，没 `From<fmt::Error>`。
        // `write_all` 返回 `io::Result`，`?` 自动走 `ExportError::Io(#[from])`。
        let file = std::fs::File::create(&out_path)?;
        let mut w = BufWriter::new(file);

        // 1) YAML front matter（Hugo/Jekyll 风格）
        w.write_all(b"---\n")?;
        w.write_all(format!("title: {}\n", book.book_name).as_bytes())?;
        w.write_all(format!("author: {}\n", book.author).as_bytes())?;
        if let Some(intro) = book.intro.as_deref() {
            let cleaned = strip_html_tags(intro);
            if !cleaned.is_empty() {
                w.write_all(b"description: |\n")?;
                for line in cleaned.lines() {
                    w.write_all(format!("  {line}\n").as_bytes())?;
                }
            }
        }
        w.write_all(b"---\n\n")?;

        // 2) 顶部 H1（书标题，与 front matter title 一致）
        w.write_all(format!("# {}\n\n", book.book_name).as_bytes())?;

        // 3) 章节锚点 TOC
        w.write_all(b"## 目录\n\n")?;
        for (idx, (title, _)) in chapters.iter().enumerate() {
            w.write_all(format!("- [{title}](#chapter-{})\n", idx + 1).as_bytes())?;
        }
        w.write_all(b"\n")?;

        // 4) 每章正文（前置一个 HTML 锚点以兼容 GFM / Obsidian / Hugo）
        for (idx, (_title, body)) in chapters.iter().enumerate() {
            // 章节渲染时已自带 `## 标题` 行；锚点用内联 HTML，GFM/CM 均识别。
            w.write_all(format!("<a id=\"chapter-{}\"></a>\n\n", idx + 1).as_bytes())?;
            w.write_all(body.trim_end().as_bytes())?;
            w.write_all(b"\n\n")?;
        }

        w.flush()?;
        Ok(out_path)
    }
}

/// 从 `001_第1章 起航.md` 抽出 `第1章 起航`。
/// `sanitize_filename` 已把文件系统非法字符替换成 `_`，回看时无需再清洗。
pub(crate) fn chapter_title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            s.split_once('_')
                .map(|(_, t)| t.to_string())
                .unwrap_or_else(|| s.to_string())
        })
        .unwrap_or_default()
}

// 注：`#[cfg(test)] mod tests { ... }` 由 Task 4 Step 1 创建并已包含全部 7 个测试。
// 本步骤不要修改或重写测试块。Step 5 跑 `cargo test --lib export::md::tests` 校验。
```

> **为什么 `chapter_title_from_path` 是 `pub(crate)`**：测试要 import；外部 crate 不应依赖该实现细节。`MdExporter::merge` 在同模块使用无可见性问题。

> **跳过 `0_` 前缀**：`sort_chapter_files` 仍排索引辅助文件在最前（numeric prefix == 0），本过滤器把它们剔除；与 `TxtExporter` 行为一致（spec §10 与现有 txt.rs 测试对齐）。

- [ ] **Step 5: 工厂分支接入**

打开 `src/export/exporter.rs`（line 72-79），`exporter_for` 末尾追加：

```rust
pub fn exporter_for(format: ExportFormat, txt_encoding: &str) -> Box<dyn Exporter + Send + Sync> {
    match format {
        ExportFormat::Txt => Box::new(super::txt::TxtExporter::new(txt_encoding)),
        ExportFormat::Html => Box::new(super::html::HtmlExporter),
        ExportFormat::Epub => Box::new(super::epub::EpubExporter),
        ExportFormat::Pdf => Box::new(super::pdf::PdfExporter),
        ExportFormat::Markdown => Box::new(super::md::MdExporter),
    }
}
```

- [ ] **Step 6: 跑测试，7 个 md 测全部通过，整 export 模块既有测无回归**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib export::`
Expected: `test export::md::tests::* ... ok` ×7；既有 `render_txt_extracts_paragraphs_with_indent`、`write_chapter_files_*`、`merge_dedup_output_filename_on_collision` 等 30+ 测试不变通过。

- [ ] **Step 7: 提交**

Run:

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add src/export/md.rs src/export/mod.rs src/export/exporter.rs && \
git commit -m "feat(export): add MdExporter (front matter + H1 + TOC + chapter anchors)" \
  -m "MdExporter::merge outputs a single .md containing:" \
  -m "- Hugo/Jekyll YAML front matter (title/author + description block" \
  -m "  when intro present)," \
  -m "- H1 book title," \
  -m "- ## 目录 anchor TOC backed by <a id='chapter-N'> markers GFM/Ob" \
  -m "  sidian/Hugo accept without slugify-differential issues." \
  -m "chapter_title_from_path strips the 001_ prefix used by" \
  -m "write_chapter_files. exporter_for routes ExportFormat::Markdown" \
  -m "here. txt_encoding arg is ignored (md is utf-8). Excluded '.'" \
  -m "and 0_ prefix files mirror TxtExporter behavior." \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 5: 桌面 UI — `ext_value` / `ext_from_str` / `ext_options`

**Files:**
- Modify: `src/desktop/pages/settings/fields.rs:216-233`（`ext_value` 加 1 行，`ext_from_str` 加 1 行）
- Modify: `src/desktop/pages/settings/page_general.rs:71-77`（`ext_options` 加 1 行）

**Interfaces:**
- Consumes: `ExportFormat::Markdown`（Task 1）+ 既有 `ext_value` / `ext_from_str` 桥函数 + 既有 `ext_options` dropdown 数据
- Produces:
  - `ext_value(ExportFormat::Markdown) -> "markdown"`
  - `ext_from_str("markdown") -> Some(ExportFormat::Markdown)`
  - `ext_options` 数组追加 `(markdown, md)`

> 这两个文件改动纯字符串映射，无新测试必要（逻辑被既有 round-trip 测试覆盖：`config::tests::round_trip_through_save_and_load` 已验证 `DownloadCfg::ext_name` JSON 序列化一致；如果既有 UI 测试存在则在此跑过；本任务交付后再做 cargo test 验证 desktop 模块无编译错误即可）。

- [ ] **Step 1: `fields.rs` 加 `Markdown` 分支**

打开 `src/desktop/pages/settings/fields.rs`，修改 `ext_value`（line 216-223）：

```rust
pub(super) const fn ext_value(e: ExportFormat) -> &'static str {
    match e {
        ExportFormat::Epub => "epub",
        ExportFormat::Txt => "txt",
        ExportFormat::Html => "html",
        ExportFormat::Pdf => "pdf",
        ExportFormat::Markdown => "markdown",
    }
}
```

修改 `ext_from_str`（line 225-233）：

```rust
pub(super) fn ext_from_str(s: &str) -> Option<ExportFormat> {
    match s {
        "epub" => Some(ExportFormat::Epub),
        "txt" => Some(ExportFormat::Txt),
        "html" => Some(ExportFormat::Html),
        "pdf" => Some(ExportFormat::Pdf),
        "markdown" => Some(ExportFormat::Markdown),
        _ => None,
    }
}
```

- [ ] **Step 2: `page_general.rs` 加 `Markdown` 选项**

打开 `src/desktop/pages/settings/page_general.rs`，修改 `ext_options`（line 71-77）：

```rust
let ext_options: Vec<(SharedString, SharedString)> = vec![
    (ext_value(ExportFormat::Epub).into(), "epub".into()),
    (ext_value(ExportFormat::Txt).into(), "txt".into()),
    (ext_value(ExportFormat::Html).into(), "html".into()),
    (ext_value(ExportFormat::Pdf).into(), "pdf".into()),
    (ext_value(ExportFormat::Markdown).into(), "md".into()),
];
```

label 用 `md`（与既有 `txt` / `html` / `pdf` 行风格一致：值是扩展名后缀）；value 用 `ext_value(ExportFormat::Markdown)` 返回的 `markdown`，通过 `ext_from_str` 反向解析时唯一匹配该变体。

- [ ] **Step 3: 验证 desktop 模块编译无错**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo build -p desktop 2>&1 | tail -30`
Expected: 编译成功；`non-exhaustive match` 错误消失；其它既有 desktop 模块不变。

如果 `pages::settings::fields::tests` 存在或被 `cargo test --lib desktop::` 触发，跑：

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --lib desktop::pages::settings::`
Expected: 既有测试通过；无新增测试需要。

- [ ] **Step 4: 提交**

Run:

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add src/desktop/pages/settings/fields.rs src/desktop/pages/settings/page_general.rs && \
git commit -m "feat(desktop): settings expose ExportFormat::Markdown as 'md' option" \
  -m "ext_value / ext_from_str / ext_options now cover all five formats." \
  -m "Label follows the existing 'ext-only' style (no i18n key needed," \
  -m "per spec — the encoding dropdown below stays enabled even with" \
  -m "md selected; MdExporter ignores txt_encoding by design)." \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 6: Web UI — `FORMAT_OPTIONS` + `ExportFormat` 类型

**Files:**
- Modify: `web-ui/src/routes/settings.tsx:33`（`FORMAT_OPTIONS` 数组追加 `markdown`）
- Modify: `web-ui/src/lib/types.ts:115`（`ExportFormat` 联合追加 `'markdown'`）

**Interfaces:**
- Consumes: 既有 `FORMAT_OPTIONS: ExportFormat[]` + 既有 `normalizeFormat` 回落逻辑（已自动支持未知值回落 `'epub'`）
- Produces:
  - `FORMAT_OPTIONS` 第 5 项 `'markdown'`
  - `ExportFormat` 联合第 5 个成员 `'markdown'`

- [ ] **Step 1: `types.ts` 加 `'markdown'`**

打开 `web-ui/src/lib/types.ts`，修改 `ExportFormat` 类型定义（line 114-115）：

```ts
/** 导出文件格式。对应后端 `config::ExportFormat`（serde 序列化为小写变体名）。 */
export type ExportFormat = 'epub' | 'txt' | 'html' | 'pdf' | 'markdown'
```

- [ ] **Step 2: `settings.tsx` 加 `'markdown'`**

打开 `web-ui/src/routes/settings.tsx`，修改 `FORMAT_OPTIONS`（line 33）：

```ts
const FORMAT_OPTIONS: ExportFormat[] = ['epub', 'txt', 'html', 'pdf', 'markdown']
```

> `normalizeFormat` 已自动处理（line 43-46：unknown → `'epub'` 回落；现在 `'markdown'` 通过 `(FORMAT_OPTIONS as string[]).includes(v)` 检查，自然被接受）。无需改 normalizeFormat 本身。

- [ ] **Step 3: Web UI 编译验证**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs/web-ui && pnpm tsc --noEmit 2>&1 | tail -20`
Expected: 编译成功；如现存组件 tests，跑 `pnpm test` 也应通过。

> 如果项目用 vite/webpack 而非 tsc 直接跑，可以替换为 `pnpm build` 看类型检查通过。等价命令由各人执行现场确认。

- [ ] **Step 4: 提交**

Run:

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add web-ui/src/routes/settings.tsx web-ui/src/lib/types.ts && \
git commit -m "feat(web): add 'markdown' to ExportFormat union + FORMAT_OPTIONS" \
  -m "Mirrors the desktop surface. normalizeFormat's white-list" \
  -m "already covers the new key; no fallback path change required." \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 7: CLI — `value_name` 帮助文本

**Files:**
- Modify: `src/cli/args.rs:127`（`format` 参数 `value_name`）

**Interfaces:**
- Consumes: 既有 `Download { format: Option<String> }` + `ExportFormat::parse`（已接受 `"markdown"`，Task 1）
- Produces: `--format` 帮助文本展示允许值含 `markdown`

- [ ] **Step 1: 改 `value_name`**

打开 `src/cli/args.rs`，修改 `format` 参数（line 126-128）：

```rust
        /// 覆盖 config.toml 的输出格式（epub / txt / html / pdf / markdown）
        #[arg(long, value_name = "epub|txt|html|pdf|markdown")]
        format: Option<String>,
```

> 这一行不仅是注释，clap 还会拿 `value_name` 当 bash completion 提示词。所以 `markdown` 必须出现。

- [ ] **Step 2: CLI 编译 + 行为验证**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo build --bin so-novel-rs 2>&1 | tail -10 && cargo run --bin so-novel-rs -- download --help 2>&1 | grep -E "(format|markdown)"
Expected: 编译成功；`--help` 中 `--format <epub|txt|html|pdf|markdown>` 行可见。

如果 `parse("markdown")` 已被 Task 1 验证，CLI 自动接受 `--format markdown`，无需额外功能代码。

- [ ] **Step 3: 提交**

Run:

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git add src/cli/args.rs && \
git commit -m "feat(cli): document --format accepts markdown in help" \
  -m "Pure help-text update; ExportFormat::parse already routes the" \
  -m "value through Task 1's new arm, so --format markdown works" \
  -m "end-to-end without further code change." \
  -m "Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 8: 端到端 smoke + 全部回归

**Files:**
- No new files
- No source modifications

**目的**：把 Task 1-7 的所有改动放回一个 `cargo test --workspace` / 手动 CLI smoke，确认全栈通。

- [ ] **Step 1: 全量回归测试**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo test --workspace 2>&1 | tail -30`
Expected: 全部测试通过 —— `config::tests::*` + `export::render::tests::*` + `export::exporter::tests::*` + `export::md::tests::*` + 任何 desktop module 测；零 fail / zero skip.

- [ ] **Step 2: Lint 检查**

Run: `cd C:/Users/pc/Documents/GitHub/so-novel-rs && cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -20`
Expected: 0 warning。`#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]` 已经覆盖所有新测试；如果 `render_md` 的 `LazyLock` 触发 clippy 警告，按 spec 已知路径加 `#[allow(...)]` 在该 static 块。

- [ ] **Step 3: 手动 smoke（CLI 端到端，写完后只用一处即足够）**

Run:

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
mkdir -p /tmp/md-smoke && \
cargo run --quiet --bin so-novel-rs -- --help 2>&1 | head -5 && \
echo "--- sanity: --format accepts markdown via ExportFormat::parse ---" && \
cargo test --quiet --lib config::tests::export_format_parse_accepts_markdown_lowercase
```

如果仓库里有真实书源 fixture 可跑（`fixtures/` 或 tests/integration 测试用 URL），加一步：

```bash
# 可选；如果项目里有 fixture 或 mock 书源，触发真实的导出 → 输出 .md
cargo run --quiet --bin so-novel-rs -- download <fixture-url> --format markdown -o /tmp/md-smoke
ls /tmp/md-smoke/ && head -30 /tmp/md-smoke/*.md
```

> 这一步若 fixture 不可得，跳过即可 —— `cargo test --workspace` 已经在 spec §9 覆盖了所有关键路径（front matter / TOC / 锚点 / EmptyChaptersDir / skip `0_`）。

Expected: 第一行 `---`，其后 `title:`、`author:`、`---`，再后 `# 起航`、`## 目录`、每个章节含 `<a id="chapter-N"></a>`。

- [ ] **Step 4: 最终提交（仅当 Step 1-3 修了东西）**

若 Step 1-3 触发了调整：

Run:

```bash
cd C:/Users/pc/Documents/GitHub/so-novel-rs && \
git status && git diff --stat
```

按动的内容再次提交（commit message 描述具体修复）。无修改则跳过本步；本计划所有任务视为完成。

- [ ] **Step 5: 任务收尾**

- 报告任务完成：8 个任务全部勾选；git log 显示至少 7 条独立 conventional commit。
- 不强制推送到 origin；用户自行决定 push 与否。
- 提示：在下次编辑器打开 Settings / 启动 CLI 时，markdown 选项已可直接选用；无需额外迁移。
