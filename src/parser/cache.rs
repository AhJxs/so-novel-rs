//! Per-Rule CSS Selector / Regex 解析结果缓存。分页抓取每页都会重新解析同一条
//! 选择器 / 正则, 几十微秒累加可达秒级, 故缓存。
//!
//! - 按**原始字符串** keyed: 用户编辑 Rule 即产生新字符串, 自动 miss, 无需显式失效;
//! - **失败结果不缓存**: 规则修好后能立即重试编译;
//! - 返回 `Arc<Selector>` / `Arc<Regex>`: `Selector` 内部用 `Rc`, 不是 `Sync`, 但
//!   `Arc<Selector>: Send`。调用方单 task 内借用, **不**跨线程共享 `&Selector`;
//! - 读写都做兜底: 缓存失效只降级为未缓存路径, 不让 `.unwrap()` 炸掉整个调用栈。

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use regex::Regex;
use scraper::Selector;

use crate::parser::dom::SelectError;

static SELECTOR_CACHE: OnceLock<Mutex<HashMap<String, Arc<Selector>>>> = OnceLock::new();
static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Arc<Regex>>>> = OnceLock::new();

/// 加锁并自动从 Mutex 中毒里恢复（保留旧值），不让后续调用一起 panic。
fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// 按字符串缓存 `Selector`。同一字符串重复解析直接命中已编译实例。
/// 解析失败返回 `Err`（**不**缓存失败结果，让用户编辑规则后能重试）。
pub fn cached_selector(sel: &str) -> Result<Arc<Selector>, SelectError> {
    let cache = SELECTOR_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    // 快速路径：try_lock 非阻塞，命中直接返回 Arc::clone。
    if let Some(arc) = cache.try_lock().ok().and_then(|g| g.get(sel).cloned()) {
        return Ok(arc);
    }
    // 拿不到锁 / 已中毒 → 拿 blocking lock 重新查，保证并发下也只 parse 一次。
    {
        let g = lock_or_recover(cache);
        if let Some(arc) = g.get(sel).cloned() {
            return Ok(arc);
        }
    }

    let parsed = match catch_unwind(AssertUnwindSafe(|| Selector::parse(sel))) {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(SelectError::BadSelector(format!("`{sel}`: {e:?}"))),
        Err(_) => {
            return Err(SelectError::BadSelector(format!(
                "`{sel}`: parser panicked"
            )));
        }
    };
    let arc = Arc::new(parsed);
    lock_or_recover(cache).insert(sel.to_string(), Arc::clone(&arc));
    Ok(arc)
}

/// 按字符串缓存 `Regex`。失败结果不缓存，规则修复后能重试编译。
pub fn cached_regex(pat: &str) -> Result<Arc<Regex>, regex::Error> {
    let cache = REGEX_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    if let Some(arc) = cache.try_lock().ok().and_then(|g| g.get(pat).cloned()) {
        return Ok(arc);
    }
    {
        let g = lock_or_recover(cache);
        if let Some(arc) = g.get(pat).cloned() {
            return Ok(arc);
        }
    }

    let parsed = match catch_unwind(AssertUnwindSafe(|| Regex::new(pat))) {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            return Err(regex::Error::Syntax("regex parser panicked".to_string()));
        }
    };
    let arc = Arc::new(parsed);
    lock_or_recover(cache).insert(pat.to_string(), Arc::clone(&arc));
    Ok(arc)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn cached_selector_returns_same_arc_for_same_string() {
        let a = cached_selector("div.book").unwrap();
        let b = cached_selector("div.book").unwrap();
        assert!(Arc::ptr_eq(&a, &b), "重复解析应返回同一 Arc 实例");
    }

    #[test]
    fn cached_selector_distinct_strings_get_distinct_arcs() {
        let a = cached_selector("div.book").unwrap();
        let b = cached_selector("span.title").unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn cached_selector_invalid_returns_error() {
        let err = cached_selector("@@@bogus@@@").unwrap_err();
        let _ = format!("{err:?}");
    }

    #[test]
    fn cached_selector_invalid_does_not_pollute_cache() {
        let _ = cached_selector("###bad###").unwrap_err();
        let ok = cached_selector("p.valid").unwrap();
        let again = cached_selector("p.valid").unwrap();
        assert!(Arc::ptr_eq(&ok, &again));
    }

    #[test]
    fn cached_regex_returns_same_arc_for_same_pattern() {
        let a = cached_regex(r"\d+").unwrap();
        let b = cached_regex(r"\d+").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn cached_regex_invalid_returns_err_and_does_not_cache() {
        // Rust regex 不支持 lookahead；这会触发 Err
        let err = cached_regex(r"(?=foo)").unwrap_err();
        let _ = format!("{err:?}");
        let ok = cached_regex(r"foo").unwrap();
        let again = cached_regex(r"foo").unwrap();
        assert!(Arc::ptr_eq(&ok, &again));
    }

    /// 16 线程并发插入 / 读取，应无 panic。
    #[test]
    fn cached_selector_concurrent_safe() {
        let n_threads = 16;
        let per_thread = 50;
        let barrier = Arc::new(Barrier::new(n_threads));

        let handles: Vec<_> = (0..n_threads)
            .map(|t| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    for i in 0..per_thread {
                        let sel = format!("div.t{t}.i{i}");
                        let arc = cached_selector(&sel).unwrap();
                        let again = cached_selector(&sel).unwrap();
                        assert!(Arc::ptr_eq(&arc, &again));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("线程 join 不应 panic");
        }
    }

    /// 同上，正则版本。
    #[test]
    fn cached_regex_concurrent_safe() {
        let n_threads = 16;
        let per_thread = 50;
        let barrier = Arc::new(Barrier::new(n_threads));

        let handles: Vec<_> = (0..n_threads)
            .map(|t| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    for i in 0..per_thread {
                        let pat = format!(r"^t{t}_i{i}$");
                        let arc = cached_regex(&pat).unwrap();
                        let again = cached_regex(&pat).unwrap();
                        assert!(Arc::ptr_eq(&arc, &again));
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("线程 join 不应 panic");
        }
    }

    /// Mutex 中毒（PoisonError）后 `lock_or_recover` 应能用旧值继续工作。
    #[test]
    fn lock_or_recover_unpoisoned_mutex_keeps_working() {
        let m: Mutex<i32> = Mutex::new(0);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("simulated poison");
        }));
        assert!(m.lock().is_err(), "Mutex 应处于 poisoned 状态");
        {
            let mut g = lock_or_recover(&m);
            *g = 42;
        }
        assert_eq!(*lock_or_recover(&m), 42);
    }
}
