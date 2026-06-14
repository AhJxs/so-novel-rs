//! 章节抓取的重试封装。对应 Java `parse.ChapterParser#retry`。
//!
//! 与 Java 端相同的语义：
//! - 第一次失败后再尝试 `max_attempts` 次（即总共最多执行 `max_attempts + 1` 次）；
//! - 每次失败前 sleep 一段（由调用方提供 sleep_fn，便于单元测试用 zero sleep）；
//! - 任何一次成功立即返回 `Ok`；
//! - 全部失败时返回最后一次的 `Err`。
//!
//! 操作 op 是个返回 future 的 async 闭包；调用方在 `download_book`
//! 里直接喂 `parse_chapter(...)` future（async parser，不再走 spawn_blocking）。

use std::time::Duration;

/// 跑一次操作；失败后按 `max_attempts` 重试。
///
/// `sleep_fn` 在每次重试**之前**调用，参数 `attempt` 是即将开始的重试次数（从 1 起）。
/// 测试时传一个无副作用的闭包；生产用 `tokio::time::sleep`.
pub async fn retry_with_backoff<T, E, Op, OpFut, S, SFut>(
    mut op: Op,
    max_attempts: u32,
    mut sleep_fn: S,
) -> Result<T, E>
where
    Op: FnMut(u32) -> OpFut,
    OpFut: std::future::Future<Output = Result<T, E>>,
    S: FnMut(u32) -> SFut,
    SFut: std::future::Future<Output = ()>,
{
    let mut last_err = None;
    // attempt 0 = 首次尝试；1..=max_attempts = 重试。
    for attempt in 0..=max_attempts {
        if attempt > 0 {
            sleep_fn(attempt).await;
        }
        match op(attempt).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                last_err = Some(e);
            }
        }
    }
    // 上面循环至少跑一次，last_err 一定被赋值。
    Err(last_err.expect("retry loop ran at least once"))
}

/// 配合 `retry_with_backoff` 的标准 sleep：递增间隔。
/// `attempt` 是第几次重试（1-based），返回的 Duration 是这次要等的时间。
///
/// 与 Java `randomInterval(config, true) * attempt` 行为一致：
/// 第 1 次重试等 base，第 2 次等 2×base，等等。
pub fn linear_backoff(base_ms: u64, attempt: u32) -> Duration {
    Duration::from_millis(base_ms.saturating_mul(attempt as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn no_sleep() -> impl FnMut(u32) -> std::future::Ready<()> {
        |_| std::future::ready(())
    }

    #[tokio::test]
    async fn first_attempt_success_no_sleep_called() {
        let sleeps = Rc::new(RefCell::new(0u32));
        let s = sleeps.clone();
        let result: Result<i32, &str> = retry_with_backoff(
            |_attempt| async { Ok(42) },
            3,
            move |_| {
                *s.borrow_mut() += 1;
                std::future::ready(())
            },
        )
        .await;
        assert_eq!(result, Ok(42));
        assert_eq!(*sleeps.borrow(), 0, "no retries → no sleeps");
    }

    #[tokio::test]
    async fn succeeds_on_third_attempt() {
        let count = Rc::new(RefCell::new(0u32));
        let c = count.clone();
        let sleeps = Rc::new(RefCell::new(0u32));
        let s = sleeps.clone();

        let result: Result<i32, &str> = retry_with_backoff(
            move |_attempt| {
                let c = c.clone();
                async move {
                    let mut n = c.borrow_mut();
                    *n += 1;
                    if *n < 3 {
                        Err("transient")
                    } else {
                        Ok(42)
                    }
                }
            },
            5,
            move |_| {
                *s.borrow_mut() += 1;
                std::future::ready(())
            },
        )
        .await;

        assert_eq!(result, Ok(42));
        assert_eq!(*count.borrow(), 3);
        // 失败 2 次 → sleep 2 次（重试前都 sleep）
        assert_eq!(*sleeps.borrow(), 2);
    }

    #[tokio::test]
    async fn exhausts_all_retries_returns_last_error() {
        let count = Rc::new(RefCell::new(0u32));
        let c = count.clone();
        let result: Result<i32, &str> = retry_with_backoff(
            move |_attempt| {
                let c = c.clone();
                async move {
                    *c.borrow_mut() += 1;
                    Err::<i32, &str>("permanent")
                }
            },
            3,
            no_sleep(),
        )
        .await;
        assert_eq!(result, Err("permanent"));
        // 总尝试次数 = 1 + max_attempts
        assert_eq!(*count.borrow(), 4);
    }

    #[tokio::test]
    async fn zero_max_attempts_runs_once() {
        let count = Rc::new(RefCell::new(0u32));
        let c = count.clone();
        let result: Result<i32, &str> = retry_with_backoff(
            move |_attempt| {
                let c = c.clone();
                async move {
                    *c.borrow_mut() += 1;
                    Err::<i32, &str>("nope")
                }
            },
            0,
            no_sleep(),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(*count.borrow(), 1, "max_attempts=0 means try once");
    }

    #[test]
    fn linear_backoff_grows() {
        assert_eq!(linear_backoff(2000, 1), Duration::from_millis(2000));
        assert_eq!(linear_backoff(2000, 2), Duration::from_millis(4000));
        assert_eq!(linear_backoff(2000, 3), Duration::from_millis(6000));
    }

    #[test]
    fn linear_backoff_saturates_on_overflow() {
        // 不应 panic
        let _ = linear_backoff(u64::MAX, 100);
    }
}
