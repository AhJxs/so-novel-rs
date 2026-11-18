//! 本地文件 → 导出格式。加载器归一化后逐章渲染，再交给现有导出器合并。

use std::path::{Path, PathBuf};

use crate::config::ExportFormat;
use crate::export::{RenderedChapter, exporter_for};
use crate::tools::encoding::parse_encoding;
use crate::tools::load::{LoadOptions, load_path};
use crate::tools::render::render_chapter;
use crate::tools::{ToolError, ToolProgress, ToolStage};

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
    on_progress: &mut dyn FnMut(ToolProgress),
) -> Result<PathBuf, ToolError> {
    on_progress(ToolProgress::new(ToolStage::Loading, 0, 1));
    let options = LoadOptions {
        source_encoding: parse_encoding(source_encoding)?,
        chapter_regex,
    };
    let book = load_path(source, &options)?;
    on_progress(ToolProgress::new(ToolStage::Loading, 1, 1));

    // 逐章渲染按章回报进度；跨步回报避免几千章刷爆 UI。
    let total = book.chapters.len();
    let stride = (total / 200).max(1);
    let mut rendered: Vec<RenderedChapter> = Vec::with_capacity(total);
    for (index, chapter) in book.chapters.iter().enumerate() {
        rendered.push(RenderedChapter {
            order: u32::try_from(index + 1).unwrap_or(u32::MAX),
            title: chapter.title.clone(),
            body: render_chapter(chapter, target),
        });
        let done = index + 1;
        if done == total || done % stride == 0 {
            on_progress(ToolProgress::new(ToolStage::Rendering, done, total));
        }
    }

    std::fs::create_dir_all(out_dir)?;
    on_progress(ToolProgress::new(ToolStage::Writing, 0, total));
    let exporter = exporter_for(target, txt_encoding);
    let cover = if matches!(target, ExportFormat::Epub) {
        book.cover.as_deref()
    } else {
        None
    };
    // 写盘阶段由导出器逐章回报（EPUB 压缩、PDF 排版都发生在逐章处理里），
    // 同样跨步上报，避免几千章刷爆 UI 通道。
    let path = exporter.merge_with_progress(
        &book.to_book(source),
        &rendered,
        out_dir,
        cover,
        &mut |done| {
            if done == total || done % stride == 0 {
                on_progress(ToolProgress::new(ToolStage::Writing, done, total));
            }
        },
    )?;
    on_progress(ToolProgress::new(ToolStage::Writing, total, total));
    Ok(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::run_convert;
    use crate::config::ExportFormat;
    use crate::tools::ToolError;

    #[test]
    fn converts_txt_to_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "第一章 起航\n\n第一段\n\n第二章 到达\n\n第二段\n").unwrap();

        let path = run_convert(
            &source,
            &tmp.path().join("out"),
            ExportFormat::Markdown,
            "UTF-8",
            None,
            None,
            &mut |_| {},
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
            &mut |_| {},
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
            &mut |_| {},
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
            &mut |_| {},
        );

        assert!(matches!(result, Err(ToolError::UnsupportedFormat(_))));
    }
}
