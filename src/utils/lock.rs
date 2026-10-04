//! 通用锁 poison 防护：把"锁被 poison 时不 panic、返 `Result`"这套模式抽出来，
//! 供 http / gpui 这类长寿命服务统一使用。

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// 拿 `Mutex` 锁。poisoned 时记录 `tracing::error!` 并返 `Err(message)`，
/// 由调用方决定怎么处理（上层模块通常包装成业务 error enum）。
///
/// # Examples
///
/// ```
/// use std::sync::Mutex;
/// use so_novel_rs::utils::lock::mutex_or;
///
/// let m = Mutex::new(42_u32);
/// let g = mutex_or("counter", &m).unwrap();
/// assert_eq!(*g, 42);
/// ```
pub fn mutex_or<'a, T>(label: &str, mtx: &'a Mutex<T>) -> Result<MutexGuard<'a, T>, String> {
    mtx.lock().map_err(|e| {
        tracing::error!("{label}: Mutex poisoned: {e}");
        format!("{label} lock poisoned")
    })
}

/// `RwLock` 读锁版本。
///
/// # Examples
///
/// 用法同 [`mutex_or`]：`rw_read_or("items", &lk)` 返回 `RwLockReadGuard`。
pub fn rw_read_or<'a, T>(label: &str, lk: &'a RwLock<T>) -> Result<RwLockReadGuard<'a, T>, String> {
    lk.read().map_err(|e| {
        tracing::error!("{label}: RwLock read poisoned: {e}");
        format!("{label} read lock poisoned")
    })
}

/// `RwLock` 写锁版本。
///
/// # Examples
///
/// 用法同 [`mutex_or`]：`rw_write_or("counter", &lk)` 返回 `RwLockWriteGuard`。
pub fn rw_write_or<'a, T>(
    label: &str,
    lk: &'a RwLock<T>,
) -> Result<RwLockWriteGuard<'a, T>, String> {
    lk.write().map_err(|e| {
        tracing::error!("{label}: RwLock write poisoned: {e}");
        format!("{label} write lock poisoned")
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn mutex_or_happy_path() {
        let m = Mutex::new(42_u32);
        let v = *mutex_or("test", &m).unwrap();
        assert_eq!(v, 42);
    }

    #[test]
    fn mutex_or_returns_err_on_poison() {
        let m = Arc::new(Mutex::new(0_u32));
        let m2 = Arc::clone(&m);
        // 在持锁线程里 panic, 触发 poison
        let _ = thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("intentional");
        })
        .join();
        let err = mutex_or("test", &m).unwrap_err();
        assert_eq!(err, "test lock poisoned");
    }

    #[test]
    fn rw_read_or_happy_path() {
        let lk = RwLock::new(String::from("hello"));
        {
            let s = rw_read_or("test", &lk).unwrap();
            assert_eq!(s.as_str(), "hello");
            drop(s);
        }
    }

    #[test]
    fn rw_write_or_happy_path() {
        let lk = RwLock::new(0_u32);
        {
            let mut g = rw_write_or("test", &lk).unwrap();
            *g = 7;
        }
        let v = *rw_read_or("test", &lk).unwrap();
        assert_eq!(v, 7);
    }

    #[test]
    fn rw_write_or_returns_err_on_poison() {
        let lk = Arc::new(RwLock::new(0_u32));
        let lk2 = Arc::clone(&lk);
        let _ = thread::spawn(move || {
            let _g = lk2.write().unwrap();
            panic!("intentional");
        })
        .join();
        let err = rw_write_or("test", &lk).unwrap_err();
        assert_eq!(err, "test write lock poisoned");
    }
}
