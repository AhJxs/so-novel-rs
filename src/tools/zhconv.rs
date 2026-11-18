//! TXT / Markdown 简繁转换：整文件转换，TXT 保持源编码。

use std::path::{Path, PathBuf};

use crate::config::LangType;
use crate::tools::encoding::decode_bytes;
use crate::tools::{ToolError, ToolProgress, ToolStage};
use crate::utils::zhconv::convert_text;

/// 执行简繁转换，返回产物路径。
///
/// # Errors
///
/// 源文件不是 `.txt` / `.md`、读取失败或目标编码无法表示转换结果时返回 [`ToolError`]。
pub fn run_zhconv(
    source: &Path,
    out_dir: &Path,
    target: LangType,
    on_progress: &mut dyn FnMut(ToolProgress),
) -> Result<PathBuf, ToolError> {
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "txt" && extension != "md" {
        return Err(ToolError::UnsupportedFormat(extension));
    }

    on_progress(ToolProgress::new(ToolStage::Loading, 0, 1));
    let bytes = std::fs::read(source)?;
    let (text, detected) = decode_bytes(&bytes, None);
    on_progress(ToolProgress::new(ToolStage::Loading, 1, 1));

    on_progress(ToolProgress::new(ToolStage::Writing, 0, 1));
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
    on_progress(ToolProgress::new(ToolStage::Writing, 1, 1));
    Ok(path)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::run_zhconv;
    use crate::config::LangType;

    #[test]
    fn converts_simplified_txt_to_traditional() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.txt");
        std::fs::write(&source, "国家的书").unwrap();

        let path = run_zhconv(&source, tmp.path(), LangType::ZhTw, &mut |_| {}).unwrap();

        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("(繁体)")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "國家的書");
    }

    #[test]
    fn markdown_output_is_utf8() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.md");
        std::fs::write(&source, "# 国家\n").unwrap();

        let path = run_zhconv(&source, tmp.path(), LangType::ZhHant, &mut |_| {}).unwrap();

        assert_eq!(path.extension().and_then(|ext| ext.to_str()), Some("md"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# 國家\n");
    }

    #[test]
    fn rejects_epub() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("书.epub");
        std::fs::write(&source, b"x").unwrap();

        let result = run_zhconv(&source, tmp.path(), LangType::ZhCn, &mut |_| {});

        assert!(matches!(
            result,
            Err(crate::tools::ToolError::UnsupportedFormat(_))
        ));
    }
}
