//! PDF 导出 (PR #17 重构, 2026-07-08). 对应 Java `handle.PdfMergeHandler`.
//!
//! # 子模块
//!
//! - [`document`] — `PdfExporter` + `Paginator` + 排版常量 (主流程)
//! - [`chapters`] — HTML → 结构化内容 (extract_chapter_content / html_to_text / wrap_text 等)
//! - [`fonts`] — CJK 字体发现 + `Measurer` 字宽量宽器
//!
//! # 行为
//!
//! - 读 `chapters_dir` 下按文件名升序的章节 HTML 文件 (每章一个, 由 `write_chapter_files`
//!   在 Pdf 模式下写出 `{order}_.html`, body 用 Html 模板)
//! - 用 `pdf_oxide` 的 `DocumentBuilder` 直接构建 PDF (不走 `from_html_css` 的
//!   HTML→DOM→Taffy 管道 — 该管道对中文小说排版问题多: 字号/行距/缩进/分页不受控)
//!
//! # 实现要点
//!
//! - **结构化内容**: 从每章 HTML 抽 `<h1>`(标题) + `<p>`(段落), 剥标签/解码实体得到纯文本
//! - **CJK 字体**: `find_cjk_font` 找系统字体, 找不到时降级到 Base-14 Helvetica (中文 tofu)
//! - **元数据**: `DocumentMetadata` 写入 title/author/subject/keywords
//! - **中文换行**: `wrap_text` 用 `Measurer` 逐字量宽, CJK 按字断行, ASCII 单词不拆
//! - **分页**: 维护 `y` 游标, 触底自动开新页; 封面页 + 每章首页强制分页
//!
//! # 局限
//!
//! - 无 CJK 字体时中文是 tofu (建议安装 Noto Sans CJK)
//! - CJK 粗体需第二个字体文件, 当前只用常规体; 章节标题靠加大字号 + 居中区分

pub mod chapters;
pub mod document;
pub mod fonts;

pub use document::PdfExporter;
