//! GPUI 排空循环：100ms 兜底 + 事件驱动唤醒。
//!
//! `desktop::run` 启动时调一次；每轮等 wakeup 信号或 100ms 兜底 tick（防 producer
//! hang 导致 UI 永不刷新），拿到 `&mut AppModel` 后 `drain`，有数据则 `ctx.notify()`。
//!
//! - entity 已释放（app 退出）时 `WeakEntity::upgrade` 返回 `None` 并 `break`；
//!   必须持 `WeakEntity` 先 upgrade 再 update —— `update_entity` 对已释放 entity 直接 panic。
//! - wakeup 用 `smol::channel::bounded(1)`：`cx.spawn` 跑在 smol executor 上，send/recv
//!   全在 smol 上下文，不触碰 tokio runtime；容量 1 让新信号覆盖旧信号而非堆积。
//! - task detached，不暴露 Task handle。

use std::time::Duration;

use gpui_kit::{App, AppContext, Entity};

use crate::desktop::model::AppModel;
use crate::desktop::model::events::{WakeupReceiver, drain};

/// 在 GPUI app 启动时调一次：`spawn` 一个循环任务，按 100ms tick + wakeup 信号
/// 排空 `AppModel`。
///
/// 调用上下文：必须在 `Application::run(|cx: &mut App| { ... })` 的闭包内。
pub fn spawn_drain_loop(model: &Entity<AppModel>, wakeup: WakeupReceiver, cx: &App) {
    // 只持弱引用：强引用会让 entity 永不释放，循环也就无法感知退出。
    let weak_model = model.downgrade();
    cx.spawn(async move |async_cx: &mut gpui_kit::AsyncApp| {
        loop {
            // 非阻塞 try_recv；拿不到就 100ms 兜底（既保证延迟感知不到，也防 producer hang）。
            if wakeup.try_recv().is_none() {
                async_cx
                    .background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                // timer 期间到达的信号再消费一次，避免堆积。
                let _ = wakeup.try_recv();
            }

            // entity 已释放时 upgrade 返回 None，直接 break。
            let Some(model) = weak_model.upgrade() else {
                break;
            };
            async_cx.update_entity(&model, |m, ctx| {
                let any = drain(m);
                if any {
                    ctx.notify();
                }
            });
        }
    })
    .detach();
}
