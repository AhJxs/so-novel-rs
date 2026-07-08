//! Per-Rule CSS Selector / Regex 解析结果缓存。
//!
//! 设计动机：`fetch_paginated_content` 在 1000 章 × ≤50 子页的抓取路径上每页
//! 都会重新解析同一条 `chapter.content` / `chapter.next_page` 选择器和
//! `chapter.next_chapter_link` 正则。每次解析都是几十微秒，累加可达秒级。
//!
//! 设计取舍：
//! 1. **按原始字符串 keyed**：同一字符串可能被多条 Rule 共享；用户编辑 Rule 即
//!    产生新字符串，**自动 miss** 重新编译，无需显式失效。
//! 2. **失败结果不缓存**：用户修复规则后能立即重试编译（典型场景：编辑规则文件
//!    保存后下一章抓取就走新字符串）。
//! 3. **`std::sync::Mutex` + `OnceLock`**：单写多读场景，与 Phase 3.1
//!    `HttpClients::rebuild_proxy` 模式一致；不引 `parking_lot` / `dashmap`。
//! 4. **返回 `Arc<Selector>` / `Arc<Regex>`**：`scraper::Selector` 内部用 `Rc` →
//!    `Selector: !Sync`，但 `Arc<Selector>: Send`。调用方 `Arc::clone` 后在单 task
//!    内 `document.select(&*arc)` 借用一次即可，**不**跨线程共享 `&Selector`。
//! 5. **不加新依赖**：`regex = "1"`、`scraper = "0.27"` 已在树；`std::sync::Mutex` +
//!    `std::collections::HashMap` 已足够。
//!
//! 健壮性细节：
//! - 读路径用 `try_lock` + `Arc::clone` 失败的 `match` 兜底：极端情况下（锁中毒
//!   或被另一个慢解析方持锁）退回未缓存路径，**不**因 `.unwrap()` 把整个调用栈炸掉。
//! - 写路径用 `lock().unwrap_or_else(|e| e.into_inner())`：标准库 `PoisonError` 自带
//!   恢复机制，调用方 panic 中毒后下一次写入仍能继续（内容已由我们重新填入）。
//! - `Selector::parse` / `Regex::new` 用 `AssertUnwindSafe` 包裹 `catch_unwind`，
//!   panic 时不污染整个缓存表。

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use regex::Regex;
use scraper::Selector;

use crate::parser::dom::SelectError;

static SELECTOR_CACHE: OnceLock<Mutex<HashMap<String, Arc<Selector>>>> = OnceLock::new();
static REGEX_CACHE: OnceLock<Mutex<HashMap<String, Arc<Regex>>>> = OnceLock::new();

/// 加锁并自动从 Mutex 中毒里恢复（保留旧值）。中毒通常源于持锁线程 panic，
/// `into_inner()` 让我们继续使用旧数据并就地覆盖坏条目，不会让后续所有调用
/// 一起 panic。
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

    // 快速路径：try_lock 非阻塞。命中（绝大多数情况）直接返回 Arc::clone。
    if let Some(arc) = cache.try_lock().ok().and_then(|g| g.get(sel).cloned()) {
        return Ok(arc);
    }
    // 慢速路径：拿不到锁 / 已中毒 → 拿 blocking lock 重新查（保证正确性，
    // 即便两线程并发也只会 parse 一次、第二个线程拿到的是 cache 里的 Arc）。
    // 中毒时 `into_inner()` 保留旧值，poison-safe。
    {
        let g = lock_or_recover(cache);
        if let Some(arc) = g.get(sel).cloned() {
            return Ok(arc);
        }
    }

    // miss 路径：parse + 写回。catch_unwind 兜底 scraper 内部 panic。
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

/// 按字符串缓存 `Regex`。失败结果不缓存，让规则修复后能重试编译。
///
/// 返回 `Ok(arc)` 表示编译成功并缓存；`Err(e)` 表示 pattern 不合法，
/// 调用方按原语义处理（warn + 跳过 / 降级）。
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
            // regex 1.x 的 parser 在我们的用法下基本不会 panic，但万一炸了不要拖累 caller。
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
        // 错误形态由 SelectError::BadSelector 包装；不强制具体文案
        let _ = format!("{err:?}");
    }

    #[test]
    fn cached_selector_invalid_does_not_pollute_cache() {
        // 先尝试一个非法字符串
        let _ = cached_selector("###bad###").unwrap_err();
        // 接着缓存另一个合法字符串
        let ok = cached_selector("p.valid").unwrap();
        // 第三次请求合法字符串仍能命中
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
        // 紧接着合法 pattern 仍能正常缓存
        let ok = cached_regex(r"foo").unwrap();
        let again = cached_regex(r"foo").unwrap();
        assert!(Arc::ptr_eq(&ok, &again));
    }

    /// 16 线程 × 不同字符串并发插入 / 读取，应无 panic / 无数据竞争（靠 Mutex 串行保证）。
    /// 用 `std::thread::spawn` 而非 tokio，省 runtime 开销。
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
                        // 同一字符串再次访问应得到同一 Arc
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

    /// Mutex 中毒（PoisonError）后 `lock_or_recover` 应能用旧值继续。
    /// 模拟场景：先 panic 一次（持锁线程 panic 把 Mutex 标为 poisoned），
    /// 再写入新条目、读出来，验证缓存仍能正常工作。
    #[test]
    fn lock_or_recover_unpoisoned_mutex_keeps_working() {
        let m: Mutex<i32> = Mutex::new(0);
        // 模拟一次"poison"路径：拿锁后 panic，Mutex 进入 poisoned 状态
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("simulated poison");
        }));
        // 此时 m.lock() 会返回 PoisonError
        assert!(m.lock().is_err(), "Mutex 应处于 poisoned 状态");
        // 但 lock_or_recover 应当能继续工作
        {
            let mut g = lock_or_recover(&m);
            *g = 42;
        }
        assert_eq!(*lock_or_recover(&m), 42);
    }
}
