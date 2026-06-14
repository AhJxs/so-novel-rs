//! 当前 unix 时间戳（秒）。失败时返回 0 — UI 显示"未知"远比 panic 友好。

use std::time::SystemTime;

pub fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
