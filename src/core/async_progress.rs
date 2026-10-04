//! 桌面 UI 共用的 "mpsc 接收端排空" helper。
//!
//! 桌面有 6 处 `mpsc::UnboundedReceiver` 用同形 `try_recv` 循环排空，抽到这里后调用方
//! 只需提供"应用 ev"的闭包；`running` / `received` 等被 render 直接读的字段仍由调用方维护。
//!
//! `mem::take` rx 而不是 `as_mut`：循环里既要写 `self` 字段又要继续 `try_recv`，
//! 借出 `&mut Receiver` 会跟 `&mut self` 冲突。关键不变量：`Empty` 必须把 rx 放回 slot，
//! `Disconnected` 不放回（`DownloadTask::drain` 同此约定）。

use tokio::sync::mpsc;

/// [`try_drain_all`] 的结果，调用方据此决定下一步：
/// `NoReceiver`（未 spawn，或 rx 被前次 drain 拿走忘了放回）、`Continue`（排空了一批且 rx 已放回）、
/// `Disconnected`（sender 已 drop，调用方应按需清理，如 `self.running = false`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// `rx_slot.take()` 拿到 `None`：没启动后台任务，或前一次 drain 已清空。
    NoReceiver,
    /// 至少处理了一批事件，rx 已放回 `rx_slot`。下次 drain 还能继续收。
    Continue,
    /// sender 端已 drop —— 调用方通常应清理派生状态（`running = false`、`scan_in_flight = false` 等）。
    Disconnected,
}

/// 把 `rx_slot` 里所有可读事件通过 `on_each` 应用一遍，返回 [`DrainOutcome`]。
///
/// 行为合约（与 [`crate::core::download_task::DownloadTask::drain`] 一致）：`rx_slot == None` →
/// 立刻返回 `NoReceiver` 且不改字段；`Empty` → 放回 rx 返回 `Continue`；`Disconnected` → **不**放回。
pub fn try_drain_all<T, F>(
    rx_slot: &mut Option<mpsc::UnboundedReceiver<T>>,
    mut on_each: F,
) -> DrainOutcome
where
    F: FnMut(T),
{
    let Some(mut rx) = rx_slot.take() else {
        return DrainOutcome::NoReceiver;
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
                return DrainOutcome::Disconnected;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn no_receiver_returns_no_receiver_outcome() {
        let mut slot: Option<mpsc::UnboundedReceiver<i32>> = None;
        let outcome = try_drain_all(&mut slot, |_| panic!("on_each 绝不能被调"));
        assert_eq!(outcome, DrainOutcome::NoReceiver);
        assert!(slot.is_none());
    }

    #[test]
    fn empty_branch_puts_rx_back() {
        // 关键不变量：排空后必须把 rx 放回去，否则下一次 drain 再也读不到事件。
        let (tx, rx) = mpsc::unbounded_channel::<i32>();
        let mut slot: Option<mpsc::UnboundedReceiver<i32>> = Some(rx);

        let outcome = try_drain_all(&mut slot, |_| panic!("不应被调"));
        assert_eq!(outcome, DrainOutcome::Continue);
        assert!(slot.is_some(), "Empty 分支必须把 rx 放回 slot");

        tx.send(42).expect("send");
        let mut received = Vec::new();
        let outcome = try_drain_all(&mut slot, |v| received.push(v));
        assert_eq!(outcome, DrainOutcome::Continue);
        assert_eq!(received, vec![42]);
    }

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

    #[test]
    fn disconnected_does_not_put_rx_back() {
        let (tx, rx) = mpsc::unbounded_channel::<i32>();
        drop(tx);
        let mut slot: Option<mpsc::UnboundedReceiver<i32>> = Some(rx);
        let outcome = try_drain_all(&mut slot, |_| panic!("不应被调"));
        assert_eq!(outcome, DrainOutcome::Disconnected);
        assert!(slot.is_none(), "Disconnected 不应放回 rx");
    }

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

    #[test]
    fn alternating_empty_and_events() {
        // 空 → Continue（rx 放回）；期间发事件，下一次 drain 仍能读到。
        let (tx, rx) = mpsc::unbounded_channel::<i32>();
        let mut slot = Some(rx);

        assert_eq!(
            try_drain_all(&mut slot, |_| panic!("不应被调")),
            DrainOutcome::Continue
        );
        assert!(slot.is_some());

        tx.send(99).expect("send");
        let mut got = 0;
        assert_eq!(
            try_drain_all(&mut slot, |v| got = v),
            DrainOutcome::Continue
        );
        assert_eq!(got, 99);

        tx.send(100).expect("send");
        let mut got2 = 0;
        assert_eq!(
            try_drain_all(&mut slot, |v| got2 = v),
            DrainOutcome::Continue
        );
        assert_eq!(got2, 100);
    }
}
