//! 时间格式化工具：把 unix 时间戳渲染成本地时区的 `YYYY-MM-DD HH:MM`。
//!
//! 故意不引 `chrono` —— 它的 features / tz database 太重，而 UI 显示精度只到分钟。
//! 自己用 Howard Hinnant 的 `civil_from_days` 算法做日历换算，跨 1970-2100 完全准确。
//! 时区偏移走 `time::UtcOffset::current_local_offset()`（仅启用了 `local-offset` feature）。

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 当前 unix 时间戳（秒）。
///
/// 失败时（系统时钟早于 UNIX_EPOCH 这种罕见情况）返回 0 —— UI 显示"未知"
/// 远比 `panic!` 友好。`DownloadTask::started_at_unix = 0` 在语义上表示"任务还没
/// 开始"，和这个 fallback 自然重合。
pub fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 把 unix 秒数（可能为负或 0）格式化为 `YYYY-MM-DD HH:MM`（本地时区）。
///
/// `unix_secs <= 0` 视为"未知" —— DownloadTask 里 `started_at_unix=0` 表示
/// 任务还没开始；library 里 `modified_unix_secs=0` 表示读不到 mtime。统一返回 "未知"
/// 让 UI 直接显示而不必各自判 0。
pub fn format_unix_local(unix_secs: i64) -> String {
    if unix_secs <= 0 {
        return "未知".to_string();
    }
    let dt = UNIX_EPOCH + Duration::from_secs(unix_secs as u64);
    let (y, mo, d) = local_date(dt);
    let hhmm = local_hhmm(dt);
    format!("{y:04}-{mo:02}-{d:02} {hhmm}")
}

/// 把 `Duration` 渲染为人类可读的"X 秒 / X 分 Y 秒 / X 时 Y 分 / X 天 Y 时"。
pub fn format_duration(d: Duration) -> String {
    let total = d.as_secs();
    if total < 60 {
        return format!("{total} 秒");
    }
    if total < 3600 {
        let m = total / 60;
        let s = total % 60;
        return if s == 0 {
            format!("{m} 分")
        } else {
            format!("{m} 分 {s} 秒")
        };
    }
    if total < 86_400 {
        let h = total / 3600;
        let m = (total % 3600) / 60;
        return if m == 0 {
            format!("{h} 时")
        } else {
            format!("{h} 时 {m} 分")
        };
    }
    let days = total / 86_400;
    let h = (total % 86_400) / 3600;
    if h == 0 {
        format!("{days} 天")
    } else {
        format!("{days} 天 {h} 时")
    }
}

// ---------- 内部：本地时区日历换算 ----------

fn local_date(t: SystemTime) -> (i32, u32, u32) {
    let days = days_from_unix(t);
    civil_from_days(days)
}

fn local_hhmm(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64;
    let offset = local_tz_offset_secs();
    let local_secs = secs + offset;
    let day_secs = local_secs.rem_euclid(86_400);
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    format!("{h:02}:{m:02}")
}

fn days_from_unix(t: SystemTime) -> i64 {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs() as i64;
    let offset = local_tz_offset_secs();
    let local_secs = secs + offset;
    local_secs.div_euclid(86_400)
}

/// Howard Hinnant 的 civil_from_days。输入距 1970-01-01 的天数，输出 (year, month, day)。
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = y + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// 读系统时区偏移（秒）。失败回退 UTC（返回 0），首次失败 warn 一次。
fn local_tz_offset_secs() -> i64 {
    match time::UtcOffset::current_local_offset() {
        Ok(off) => off.whole_seconds() as i64,
        Err(e) => {
            use std::sync::atomic::{AtomicBool, Ordering};
            static WARNED: AtomicBool = AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                tracing::warn!("读系统时区失败 ({e})，UI 时间显示回退到 UTC");
            }
            0
        }
    }
}

// ---------- u64 便捷入口（library 用）----------

/// `u64` 版本，便于 `LibraryEntry::modified_unix_secs` 这种从 fs 拿到的非负时间戳。
/// 内部夹到 `i64::MAX`（实际上 u64 表示的时间到 2554 年才溢出，UI 场景不会发生）。
pub fn format_unix_local_u64(unix_secs: u64) -> String {
    let s = i64::try_from(unix_secs).unwrap_or(i64::MAX);
    format_unix_local(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_unknown() {
        assert_eq!(format_unix_local(0), "未知");
        assert_eq!(format_unix_local(-1), "未知");
        assert_eq!(format_unix_local_u64(0), "未知");
    }

    #[test]
    fn shape_is_fixed_width() {
        // 不能断言具体值（取决于 runner 时区），但格式必须是 YYYY-MM-DD HH:MM = 16 字符。
        let s = format_unix_local(1_767_225_600);
        assert_eq!(s.chars().count(), 16, "got {s:?}");
        assert_eq!(s.chars().nth(4), Some('-'));
        assert_eq!(s.chars().nth(7), Some('-'));
        assert_eq!(s.chars().nth(10), Some(' '));
        assert_eq!(s.chars().nth(13), Some(':'));
    }

    #[test]
    fn duration_units() {
        assert_eq!(format_duration(Duration::from_secs(45)), "45 秒");
        assert_eq!(format_duration(Duration::from_secs(60)), "1 分");
        assert_eq!(format_duration(Duration::from_secs(125)), "2 分 5 秒");
        assert_eq!(format_duration(Duration::from_secs(3600)), "1 时");
        assert_eq!(format_duration(Duration::from_secs(7320)), "2 时 2 分");
        assert_eq!(format_duration(Duration::from_secs(86_400)), "1 天");
        assert_eq!(format_duration(Duration::from_secs(90_000)), "1 天 1 时");
    }
}
