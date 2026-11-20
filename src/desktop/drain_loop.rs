//! GPUI 排空循环：事件驱动唤醒 + 100ms / 500ms 自适应兜底。
//!
//! `desktop::run` 启动时调一次；每轮等 wakeup 信号或兜底 tick（防 producer
//! hang 导致 UI 永不刷新），拿到 `&mut AppModel` 后 `drain`，有数据则 `ctx.notify()`。
//!
//! - 兜底间隔随空闲退避：有下载任务在跑时固定 100ms（进度条保持 100ms 节奏）；
//!   连续空闲 5 轮后退到 500ms 上限。任一 wakeup 信号或 `drain` 有进展立刻回到
//!   100ms —— 空闲时 timer 唤醒从 600 次/分降到约 120 次/分，任务运行时节奏不变。
//! - entity 已释放（app 退出）时 `WeakEntity::upgrade` 返回 `None` 并 `break`；
//!   必须持 `WeakEntity` 先 upgrade 再 update —— `update_entity` 对已释放 entity 直接 panic。
//! - 判断「是否有任务在跑」同样只短暂 upgrade 读一次，读完立刻释放强引用；把
//!   `Entity` 带过 timer 的 `.await` 会让 entity 在等待期间存活，拖慢 app 退出。
//! - wakeup 用 `smol::channel::bounded(1)`：`cx.spawn` 跑在 smol executor 上，send/recv
//!   全在 smol 上下文，不触碰 tokio runtime；容量 1 让新信号覆盖旧信号而非堆积。
//! - task detached，不暴露 Task handle。

use std::time::Duration;

use gpui_kit::{App, AppContext, AsyncApp, Entity};

use crate::core::DownloadTask;
use crate::desktop::model::AppModel;
use crate::desktop::model::events::{WakeupReceiver, drain};

/// 任务运行中（或尚未深度空闲）时的兜底 tick。
const ACTIVE_TICK_MS: u64 = 100;
/// 连续空闲若干轮后的兜底 tick 上限。
const IDLE_TICK_MS: u64 = 500;
/// 连续空闲多少轮后进入 500ms 兜底。
const IDLE_ROUNDS_BEFORE_BACKOFF: u32 = 5;

/// 本轮等待时长（毫秒）。
///
/// 有任务在跑 → 始终 100ms（进度条节奏）；否则连续空闲 `idle` 轮后从 100ms
/// 退到 500ms。`idle` 由调用方维护：wakeup、任务运行、`drain` 有进展都会先清零。
const fn backoff_ms(idle: u32, task_running: bool) -> u64 {
    if task_running || idle < IDLE_ROUNDS_BEFORE_BACKOFF {
        ACTIVE_TICK_MS
    } else {
        IDLE_TICK_MS
    }
}

/// 在 GPUI app 启动时调一次：`spawn` 一个循环任务，按 wakeup 信号或
/// 100ms / 500ms 自适应兜底 tick 排空 `AppModel`。
///
/// 调用上下文：必须在 `Application::run(|cx: &mut App| { ... })` 的闭包内。
pub fn spawn_drain_loop(model: &Entity<AppModel>, wakeup: WakeupReceiver, cx: &App) {
    // 只持弱引用：强引用会让 entity 永不释放，循环也就无法感知退出。
    let weak_model = model.downgrade();
    cx.spawn(async move |async_cx: &mut AsyncApp| {
        // 连续空闲轮数；到阈值后兜底 tick 退到 500ms。
        let mut idle = 0_u32;
        loop {
            if wakeup.try_recv().is_some() {
                // 事件驱动唤醒：立刻排空，无需 timer；同时重置退避。
                idle = 0;
            } else {
                // 只在等待前短暂 upgrade 读一次「是否有任务在跑」，读完立刻释放强引用：
                // 若把 `Entity` 带过 `.await`，等待期间 entity 不会释放，退出最多被拖 500ms。
                let task_running = {
                    let Some(model) = weak_model.upgrade() else {
                        break;
                    };
                    async_cx
                        .read_entity(&model, |m, _| m.tasks.iter().any(DownloadTask::is_running))
                };
                async_cx
                    .background_executor()
                    .timer(Duration::from_millis(backoff_ms(idle, task_running)))
                    .await;
                // timer 期间到达的信号再消费一次：既避免堆积，也说明有活动 → 重置退避。
                let woken_while_waiting = wakeup.try_recv().is_some();
                idle = if task_running || woken_while_waiting {
                    0
                } else {
                    idle.saturating_add(1)
                };
            }

            // entity 已释放时 upgrade 返回 None，直接 break。
            let Some(model) = weak_model.upgrade() else {
                break;
            };
            let any = async_cx.update_entity(&model, |m, ctx| {
                let any = drain(m);
                if any {
                    ctx.notify();
                }
                any
            });
            if any {
                // drain 有进展（任务进度 / 刚结束任务的落盘等）：回到 100ms 节奏。
                idle = 0;
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn running_task_keeps_100ms_regardless_of_idle() {
        for idle in [0, 4, 5, 100, u32::MAX] {
            assert_eq!(backoff_ms(idle, true), ACTIVE_TICK_MS, "idle={idle}");
        }
    }

    #[test]
    fn idle_backs_off_after_five_rounds() {
        // 前 5 轮空转仍是 100ms（idle = 0..4）。
        for idle in 0..IDLE_ROUNDS_BEFORE_BACKOFF {
            assert_eq!(backoff_ms(idle, false), ACTIVE_TICK_MS, "idle={idle}");
        }
        // 第 6 轮起退到 500ms，且不会继续增长。
        for idle in [IDLE_ROUNDS_BEFORE_BACKOFF, 6, 100, u32::MAX] {
            assert_eq!(backoff_ms(idle, false), IDLE_TICK_MS, "idle={idle}");
        }
    }

    #[test]
    fn wakeup_or_progress_reset_returns_to_100ms() {
        // 重置在循环里是 `idle = 0`；这里验证重置后组出的等待时长。
        assert_eq!(backoff_ms(0, false), ACTIVE_TICK_MS);
        assert_eq!(backoff_ms(0, true), ACTIVE_TICK_MS);
    }
}
