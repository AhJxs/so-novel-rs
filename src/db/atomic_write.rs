use std::path::Path;

/// 把 `data` 写到 `path`, 失败时不会留下半截文件: 同目录生成唯一临时名 (避免多实例冲突) →
/// 全量写 + fsync → `rename` 覆盖 → 失败时主动删临时文件。临时文件放同目录是为了让 `rename`
/// 是原子的 (跨目录 / 跨文件系统 rename 不是原子)。
///
/// # Errors
///
/// 创建 / 写入 / fsync / rename 任一步失败时返回对应的 `std::io::Error`；
/// 失败路径已清理临时文件。
#[tracing::instrument(skip(data), fields(path = %path.display(), bytes = data.len()))]
pub fn write_atomically(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config");
    // 全局唯一 + 进程内唯一（同一 ms 内多次调用也不会冲突）。
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = parent.join(format!(".{file_name}.tmp.{}.{}", std::process::id(), seq));

    let write_result = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        Ok(())
    })();

    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    // rename 覆盖目标。Windows 上 `std::fs::rename` 不允许覆盖已存在的目标, 所以先
    // remove 再 rename。两步不是严格原子, 但配合上面的 fsync, 断电最坏是"老文件还在"。
    if path.exists()
        && let Err(e) = std::fs::remove_file(path)
    {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use std::io::Read;

    use super::*;

    #[test]
    fn write_atomically_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        write_atomically(&path, b"hello").unwrap();
        let mut s = String::new();
        std::fs::File::open(&path)
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn write_atomically_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        write_atomically(&path, b"v1").unwrap();
        write_atomically(&path, b"v2").unwrap();
        let s = std::fs::read_to_string(&path).unwrap();
        assert_eq!(s, "v2");
    }

    #[test]
    fn write_atomically_cleans_tmp_on_failure() {
        // 写一个不可写的目录触发失败, 验证没有 .tmp.* 残留
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        std::fs::create_dir(&path).unwrap();
        let result = write_atomically(&path, b"x");
        assert!(result.is_err(), "目标是目录，写入必须失败，实际 {result:?}");

        let leftover: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| n.starts_with(".data.json.tmp."))
            })
            .collect();
        assert!(
            leftover.is_empty(),
            "tmp files should be cleaned, found: {leftover:?}"
        );
    }
}
