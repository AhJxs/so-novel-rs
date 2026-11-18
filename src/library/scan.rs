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
            .map(|file| {
                file.path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
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
