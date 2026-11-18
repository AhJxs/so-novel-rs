//! 下载文件扩展名白名单。
//!
//! 白名单集中一处避免漏改；书架的扫描 / 导入 / 重新定位各自按需引用。

/// 下载文件扩展名白名单（**单一事实来源**）。
/// 新增 / 删除格式只改这一处。
pub const SUPPORTED_LIBRARY_EXTS: &[&str] = &["epub", "txt", "html", "zip", "pdf", "md"];

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

    use super::*;

    #[test]
    fn supported_exts_contains_expected() {
        for ext in ["epub", "txt", "html", "zip", "pdf", "md"] {
            assert!(
                SUPPORTED_LIBRARY_EXTS.contains(&ext),
                "{ext} should be in SUPPORTED_LIBRARY_EXTS"
            );
        }
    }

    #[test]
    fn supported_exts_length_is_six() {
        // 锁死长度 —— 防新加 / 删除扩展名时漏改下游消费方的分页 / 计数逻辑。
        assert_eq!(
            SUPPORTED_LIBRARY_EXTS.len(),
            6,
            "新增/删除扩展名时必须同步下游分页/计数逻辑"
        );
    }
}
