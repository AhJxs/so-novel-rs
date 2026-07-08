//! 100ms 兜底 + 事件驱动唤醒的 GPUI 排空循环。
//!
//! 从 `crate::app::events` 搬过来 —— 它 100% 是 GPUI 桥（`gpui::AsyncApp` /
//! `cx.spawn().detach()` / `background_executor().timer()` / `update_entity`），
//! 不属于"业务层与 UI 框架解耦"的 `crate::app`。原 `events.rs` 现在只保留
//! 纯排空逻辑（drain channels + push `UIEvent`）。
//!
//! 流程：
//! 1. `gpui_app::run` 启动时调一次（见 `crate::gpui_app::run`）；
//! 2. 循环里 `select!` 风格：要么被 `wakeup` 信号唤醒（`smol::channel`），要么
//!    兜底 100ms tick —— 防止 producer 异常 hang 导致 UI 永远不刷新。
//! 3. 拿到 `&mut AppModel` 后调 `drain`；任何 channel 有数据则 `ctx.notify()`
//!    触发当前 view 重绘。
//! 4. entity 已释放（app 退出）时 `update_entity` 返回 `Err`，`break` 出循环；
//! 5. task detached —— 进程退出时随 executor 终止，不暴露 Task handle。
//!
//! ## 为什么 100ms 兜底
//!
//! 搜索/详情/封面/TOC/下载进度/书源健康检查/更新检查 7 条后台通道都走这个
//! 循环统一推动 UI 重绘。100ms 粒度用户感知不到延迟，**而且**作为兜底防止
//! wakeup 通道丢失信号（比如外部代码忘了 `try_send）时` UI 永远不刷新。
//!
//! ## wakeup 通道类型
//!
//! 用 `smol::channel::bounded(1)` —— `gpui` 的 `cx.spawn` 跑在 smol executor
//! 上，所有 send/recv 都在 smol 上下文，**不**触碰 tokio runtime（避免跨
//! executor 复杂度）。`bounded(1)` 容量让"已有一发未读"时第二次 send 直接
//! 覆盖而不是堆积。

use std::time::Duration;

use gpui::{App, AppContext, Entity};

use crate::app::AppModel;
use crate::app::events::{WakeupReceiver, drain};

/// 在 GPUI app 启动时调一次：`spawn` 一个循环任务，按 100ms tick + wakeup 信号
/// 排空 `AppModel`。
///
/// 调用上下文：必须在 `Application::run(|cx: &mut App| { ... })` 的闭包内，
/// 在 `open_window` 前后调都可以。
///
/// 任务 detached：返回 `()`，不暴露 Task handle，进程退出时随 executor 终止。
pub fn spawn_drain_loop(model: Entity<AppModel>, wakeup: WakeupReceiver, cx: &App) {
    cx.spawn(async move |async_cx: &mut gpui::AsyncApp| {
        loop {
            // 等待：要么被 wakeup 唤醒，要么兜底 100ms tick。
            // 简化做法：先 try_recv（非阻塞），拿不到就 100ms 兜底。
            // 这样比 `or` 组合两个 future 更可读，且 100ms 兜底本身就是
            // 必需的（防 producer hang）。
            if wakeup.try_recv().is_none() {
                async_cx
                    .background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                // timer 醒来后再 try_recv 一次，把上一次 timer 期间的
                // 信号消耗掉（bounded 容量 1，避免堆积）。
                let _ = wakeup.try_recv();
            }

            // 排空一次；entity 已释放（app 退出）时返回 Err，直接 break。
            let result = async_cx.update_entity(&model, |m, ctx| {
                let any = drain(m);
                if any {
                    ctx.notify();
                }
            });
            if result.is_err() {
                break;
            }
        }
    })
    .detach();
}
