//! 三端共用的 "mpsc 接收端排空" helper。
//!
//! ## 背景
//!
//! 桌面有 6 处 `mpsc::UnboundedReceiver` 用同形 `try_recv` 循环排空：
//! - `SourcesState::drain`
//! - `SearchState::drain` / `drain_detail` / `drain_cover` / `drain_toc`
//! - `UpdateState::drain`
//!
//! 每处的 `try_recv` 循环模板都一样：
//! ```ignore
//! let Some(rx) = self.rx.as_mut() else { return false; };
//! loop {
//!     match rx.try_recv() {
//!         Ok(ev) => { any = true; /* apply ev */ }
//!         Err(Empty) => break,
//!         Err(Disconnected) => { self.rx = None; break; }
//!     }
//! }
//! ```
//!
//! 抽到 [`try_drain_all`] 后只剩"应用 ev"那部分逻辑。`running` / `expected` /
//! `received` 等被 render 直接读的字段仍由调用方 struct 自己拥有 + 维护（plan 验证过：
//! struct 化等于把这些字段搬到 `core`，改动面更大，收益小）。
//!
//! ## 为什么 `mem::take` rx 而不是 `as_mut`
//!
//! 调用方在循环里既要把事件应用到 `self` 字段，又要用 `rx` 继续 `try_recv` ——
//! `Option<UnboundedReceiver>` 直接 `as_mut()` 借出 `&mut Receiver` 后再写 `self.rx`
//! 字段会跟 `&mut self` 冲突。`mem::take` 把所有权移出，循环内只 `&mut self` 写字段；
//! `Empty` 分支再把 rx 放回去（关键不变量 —— 否则下次 drain 永远收不到事件）。
//!
//! 同样的不变量也是 [`crate::core::download_task::DownloadTask::drain`] 的核心：
//! Phase 3.4 重写时也用了 `mem::take`。

use tokio::sync::mpsc;

/// [`try_drain_all`] 的结果。
///
/// 调用方据此判断下一步：
/// - [`DrainOutcome::NoReceiver`] — 调用方从未 spawn / 已清空 / 通道被前次 drain 拿走
///   但忘了放回去。**不**常见；如果发生说明调用方逻辑漏了。
/// - [`DrainOutcome::Continue`] — 排空了至少一批事件，rx 已放回 `rx_slot`；下次
///   drain 还能继续收。
/// - [`DrainOutcome::Disconnected`] — sender 端已 drop，调用方应按需清理
///   （如 `self.running = false`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// `rx_slot.take()` 拿到 `None`。调用方可能没启动后台任务，或前一次 drain 已
    /// 清空。
    NoReceiver,
    /// 至少处理了一批事件，rx 已放回 `rx_slot`。下次 drain 还能继续收。
    Continue,
    /// sender 端已 drop —— 调用方通常应清理派生状态（`running = false`、`scan_in_flight = false` 等）。
    Disconnected,
}

/// 把 `rx_slot` 里所有可读事件通过 `on_each` 应用一遍，返回 [`DrainOutcome`]。
///
/// ## 行为合约（与 [`crate::core::download_task::DownloadTask::drain`] 一致）
///
/// - `rx_slot == None` → 立刻返回 [`DrainOutcome::NoReceiver`]，不修改任何字段
/// - 拿到 rx 后循环 `try_recv`：
///   - `Ok(ev)` → 调 `on_each(ev)`，继续循环
///   - `Empty` → 把 rx 放回 `rx_slot`，返回 [`DrainOutcome::Continue`]
///   - `Disconnected` → **不**放回 rx（sender 已 drop，留 None 即可），返回
///     [`DrainOutcome::Disconnected`]
///
/// 调用方如果想知道"是否产生过事件"，用 closure-captured bool：
/// ```ignore
/// let mut any = false;
/// let outcome = try_drain_all(&mut self.rx, |ev| {
///     any = true;
///     self.apply(ev);
/// });
/// ```
pub fn try_drain_all<T, F>(
    rx_slot: &mut Option<mpsc::UnboundedReceiver<T>>,
    mut on_each: F,
) -> DrainOutcome
where
    F: FnMut(T),
{
    let mut rx = match rx_slot.take() {
        Some(rx) => rx,
        None => return DrainOutcome::NoReceiver,
    };
    loop {
        match rx.try_recv() {
            Ok(ev) => on_each(ev),
            Err(mpsc::error::TryRecvError::Empty) => {
                // 关键：把 rx 放回去 —— 下次 drain 还能读到新事件。
                *rx_slot = Some(rx);
                return DrainOutcome::Continue;
            }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                // sender 已 drop，留 None 即可；不放回。
                return DrainOutcome::Disconnected;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    // ── rx_slot = None ───────────────────────────────────

    #[test]
    fn no_receiver_returns_no_receiver_outcome() {
        let mut slot: Option<mpsc::UnboundedReceiver<i32>> = None;
        let outcome = try_drain_all(&mut slot, |_| panic!("on_each 绝不能被调"));
        assert_eq!(outcome, DrainOutcome::NoReceiver);
        assert!(slot.is_none());
    }

    // ── Empty 分支：rx 必须被放回去 ───────────────────────

    #[test]
    fn empty_branch_puts_rx_back() {
        // 关键不变量：drain 排空后必须把 rx 放回去，下一次 drain 还能再读。
        let (tx, rx) = mpsc::unbounded_channel::<i32>();
        let mut slot: Option<mpsc::UnboundedReceiver<i32>> = Some(rx);

        // 通道空 → Continue + rx 被放回去
        let outcome = try_drain_all(&mut slot, |_| panic!("不应被调"));
        assert_eq!(outcome, DrainOutcome::Continue);
        assert!(slot.is_some(), "Empty 分支必须把 rx 放回 slot");

        // 第二次 drain 必须能读到新事件
        tx.send(42).expect("send");
        let mut received = Vec::new();
        let outcome = try_drain_all(&mut slot, |v| received.push(v));
        assert_eq!(outcome, DrainOutcome::Continue);
        assert_eq!(received, vec![42]);
    }

    // ── 多事件：全收到 ───────────────────────────────────

    #[test]
    fn drains_multiple_events_in_one_call() {
        let (tx, rx) = mpsc::unbounded_channel::<i32>();
        let mut slot: Option<mpsc::UnboundedReceiver<i32>> = Some(rx);

        for v in 1..=5 {
            tx.send(v).expect("send");
        }
        let mut collected = Vec::new();
        let outcome = try_drain_all(&mut slot, |v| collected.push(v));
        assert_eq!(outcome, DrainOutcome::Continue);
        assert_eq!(collected, vec![1, 2, 3, 4, 5]);
        assert!(slot.is_some(), "Continue 必须把 rx 放回");
    }

    // ── Disconnected：rx 不放回 ──────────────────────────

    #[test]
    fn disconnected_does_not_put_rx_back() {
        // sender drop 后 → Disconnected；slot 应为 None（不放回）
        let (tx, rx) = mpsc::unbounded_channel::<i32>();
        drop(tx);
        let mut slot: Option<mpsc::UnboundedReceiver<i32>> = Some(rx);
        let outcome = try_drain_all(&mut slot, |_| panic!("不应被调"));
        assert_eq!(outcome, DrainOutcome::Disconnected);
        assert!(slot.is_none(), "Disconnected 不应放回 rx");
    }

    // ── 顺序：先发 3 个，再 sender drop ───────────────────

    #[test]
    fn events_then_disconnected() {
        let (tx, rx) = mpsc::unbounded_channel::<i32>();
        tx.send(1).expect("send");
        tx.send(2).expect("send");
        tx.send(3).expect("send");
        drop(tx); // sender drop → 后续 Disconnected

        let mut slot: Option<mpsc::UnboundedReceiver<i32>> = Some(rx);
        let mut collected = Vec::new();
        let outcome = try_drain_all(&mut slot, |v| collected.push(v));
        assert_eq!(outcome, DrainOutcome::Disconnected);
        assert_eq!(collected, vec![1, 2, 3], "发的事件必须全部 drain");
        assert!(slot.is_none());
    }

    // ── on_each 闭包可以写 slot 之外的状态 ────────────────

    #[test]
    fn closure_can_capture_external_state() {
        let (tx, rx) = mpsc::unbounded_channel::<i32>();
        tx.send(10).expect("send");
        tx.send(20).expect("send");

        let mut slot = Some(rx);
        let mut sum = 0_i32;
        let outcome = try_drain_all(&mut slot, |v| sum += v);
        assert_eq!(outcome, DrainOutcome::Continue);
        assert_eq!(sum, 30);
    }

    // ── Empty 后立刻再 Drain 还能读到 ─────────────────────

    #[test]
    fn alternating_empty_and_events() {
        // 第一次：空 → Continue（rx 放回）
        // 中间：发一个事件
        // 第二次：读到事件 → Continue
        // 中间：再发，再读
        let (tx, rx) = mpsc::unbounded_channel::<i32>();
        let mut slot = Some(rx);

        // 空通道
        assert_eq!(
            try_drain_all(&mut slot, |_| panic!("不应被调")),
            DrainOutcome::Continue
        );
        assert!(slot.is_some());

        // 发一个
        tx.send(99).expect("send");
        let mut got = 0;
        assert_eq!(
            try_drain_all(&mut slot, |v| got = v),
            DrainOutcome::Continue
        );
        assert_eq!(got, 99);

        // 再发一个
        tx.send(100).expect("send");
        let mut got2 = 0;
        assert_eq!(
            try_drain_all(&mut slot, |v| got2 = v),
            DrainOutcome::Continue
        );
        assert_eq!(got2, 100);
    }
}
