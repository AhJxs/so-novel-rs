//! 从发布包里解出主程序：Windows 为 `.zip`，其余平台为 `.tar.gz`。
//!
//! 只按文件名匹配主程序，不信任包内路径（不拼接、不解包其余文件），
//! 顺带用 zip / gzip 自带的 CRC 做完整性校验。

use std::fs::File;
use std::io::{BufReader, Read as _};
use std::path::{Path, PathBuf};

use crate::core::update::UpdateError;
use crate::update::BINARY_NAME;

/// 解出主程序并写为 `dest_dir/主程序名`，返回落盘路径。
///
/// # Errors
///
/// 压缩包无法读取、CRC 校验失败、或包内没有主程序时返回 [`UpdateError`]。
pub fn extract_binary(archive: &Path, dest_dir: &Path) -> Result<PathBuf, UpdateError> {
    let name = archive
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| UpdateError("无法识别的安装包名".into()))?;
    let lower = name.to_ascii_lowercase();
    let binary = if Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        extract_zip(archive)?
    } else if lower.ends_with(".tar.gz") {
        extract_tar_gz(archive)?
    } else {
        return Err(UpdateError(format!("不支持的安装包格式: {name}")));
    };

    std::fs::create_dir_all(dest_dir).map_err(|e| UpdateError(format!("创建暂存目录失败: {e}")))?;
    let out = dest_dir.join(BINARY_NAME);
    std::fs::write(&out, binary).map_err(|e| UpdateError(format!("写入主程序失败: {e}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| UpdateError(format!("设置执行权限失败: {e}")))?;
    }
    Ok(out)
}

fn extract_zip(archive: &Path) -> Result<Vec<u8>, UpdateError> {
    let file = File::open(archive).map_err(|e| UpdateError(format!("打开安装包失败: {e}")))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| UpdateError(format!("读取 zip 目录失败: {e}")))?;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|e| UpdateError(format!("读取 zip 条目失败: {e}")))?;
        let is_binary = entry
            .enclosed_name()
            .and_then(|path| path.file_name().map(|n| n == BINARY_NAME))
            .unwrap_or(false);
        if !is_binary {
            continue;
        }
        // 读到条目末尾时 zip 会校验 CRC。
        let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| UpdateError(format!("解压主程序失败: {e}")))?;
        return Ok(bytes);
    }
    Err(UpdateError("安装包里没有主程序".into()))
}

fn extract_tar_gz(archive: &Path) -> Result<Vec<u8>, UpdateError> {
    let file = File::open(archive).map_err(|e| UpdateError(format!("打开安装包失败: {e}")))?;
    let mut decoder = flate2::read::GzDecoder::new(BufReader::new(file));
    let mut found: Option<Vec<u8>> = None;
    {
        let mut tar = tar::Archive::new(&mut decoder);
        let entries = tar
            .entries()
            .map_err(|e| UpdateError(format!("读取 tar 目录失败: {e}")))?;
        for entry in entries {
            let mut entry = entry.map_err(|e| UpdateError(format!("读取 tar 条目失败: {e}")))?;
            let is_binary = entry
                .path()
                .ok()
                .and_then(|path| path.file_name().map(|n| n == BINARY_NAME))
                .unwrap_or(false);
            if is_binary && found.is_none() {
                let mut bytes = Vec::new();
                entry
                    .read_to_end(&mut bytes)
                    .map_err(|e| UpdateError(format!("解压主程序失败: {e}")))?;
                found = Some(bytes);
            }
        }
    }
    // 读到流末尾才会校验 gzip 尾部的 CRC32 与长度。
    std::io::copy(&mut decoder, &mut std::io::sink())
        .map_err(|e| UpdateError(format!("安装包校验失败: {e}")))?;
    found.ok_or_else(|| UpdateError("安装包里没有主程序".into()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use std::io::Write as _;

    use super::*;

    const PAYLOAD: &[u8] = b"#!fake-binary";

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, body) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }

    fn write_tar_gz(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut builder = tar::Builder::new(encoder);
        for (name, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(u64::try_from(body.len()).unwrap());
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *body).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }

    fn nested(name: &str) -> String {
        format!("so-novel-rs-0.7.2/{name}")
    }

    #[test]
    fn extracts_binary_from_zip() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("pkg.zip");
        write_zip(
            &archive,
            &[
                (&nested("rules/site.txt"), b"rule"),
                (&nested(BINARY_NAME), PAYLOAD),
            ],
        );

        let out = extract_binary(&archive, &dir.path().join("stage")).unwrap();

        assert_eq!(out, dir.path().join("stage").join(BINARY_NAME));
        assert_eq!(std::fs::read(&out).unwrap(), PAYLOAD);
        assert!(!dir.path().join("stage").join("rules").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&out).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }

    #[test]
    fn extracts_binary_from_tar_gz() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("pkg.tar.gz");
        write_tar_gz(
            &archive,
            &[
                (&nested("rules/site.txt"), b"rule"),
                (&nested(BINARY_NAME), PAYLOAD),
            ],
        );

        let out = extract_binary(&archive, &dir.path().join("stage")).unwrap();

        assert_eq!(std::fs::read(&out).unwrap(), PAYLOAD);
    }

    #[test]
    fn missing_binary_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("pkg.tar.gz");
        write_tar_gz(&archive, &[(&nested("rules/site.txt"), b"rule")]);

        let err = extract_binary(&archive, &dir.path().join("stage")).unwrap_err();

        assert!(err.to_string().contains("没有主程序"));
    }

    #[test]
    fn unknown_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("pkg.7z");
        std::fs::write(&archive, b"x").unwrap();

        let err = extract_binary(&archive, &dir.path().join("stage")).unwrap_err();

        assert!(err.to_string().contains("不支持"));
    }
}
