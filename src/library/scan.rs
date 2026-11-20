//! 下载目录扫描：只读直接子文件，按扩展名白名单过滤。无状态、无 UI。

use std::path::{Path, PathBuf};

use super::book::{BookFormat, mtime_secs, normalize_path_key};

/// 扫描到的单个文件。
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime: i64,
    /// 规范化路径键，扫描线程算一次；合并时直接用作哈希索引。
    pub key: String,
}

/// 扫描目录（不递归），只保留首版书库支持的扩展名，按修改时间倒序。
///
/// # Errors
///
/// 目录不可读、或枚举目录项本身失败时返回 [`std::io::Error`]；单个条目
/// 读不出元信息时只跳过它（读不到大小/时间的书仍应出现在书架上）。
#[tracing::instrument(name = "library::scan_download_dir", skip_all, fields(dir = %dir.display()))]
pub fn scan_download_dir(dir: &Path) -> std::io::Result<Vec<ScannedFile>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        // 扩展名是纯字符串判断，放在任何 io 之前。
        if BookFormat::from_path(&path).is_none() {
            continue;
        }
        // `DirEntry::metadata()` 不跟随链接，而 `Path::is_file()` 会跟随；
        // 混用会让链接书拿到链接自身的大小/时间。直接用 `file_type()` 一律跳过链接。
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        // 路径键在后台线程算：合并时 UI 线程不再为每个文件做一次 canonicalize。
        let key = normalize_path_key(&path);
        out.push(ScannedFile {
            path,
            size_bytes: metadata.len(),
            mtime: mtime_secs(&metadata),
            key,
        });
    }
    // 修改时间倒序；同一秒内按路径升序，保证同一目录两次扫描顺序一致。
    out.sort_by(|a, b| b.mtime.cmp(&a.mtime).then_with(|| a.path.cmp(&b.path)));
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
        // 名字像书、其实是目录：必须被 `is_file` 挡掉。
        std::fs::create_dir(dir.path().join("fake.txt")).unwrap();

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
        assert!(
            files.windows(2).all(|pair| pair[0].mtime >= pair[1].mtime),
            "必须按修改时间倒序"
        );
    }

    #[test]
    fn scanned_files_carry_normalized_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"x").unwrap();
        let files = scan_download_dir(dir.path()).unwrap();
        assert_eq!(files[0].key, normalize_path_key(&path));
    }

    #[test]
    fn missing_directory_is_an_error() {
        assert!(scan_download_dir(Path::new("Z:/definitely/missing/dir")).is_err());
    }
}
