//! 100ms `events::drain` + `cx.notify()` 的 GPUI 循环。
//!
//! 从 `crate::app::events` 搬过来 —— 它 100% 是 GPUI 桥（`gpui::AsyncApp` /
//! `cx.spawn().detach()` / `background_executor().timer()` / `update_entity`），
//! 不属于"业务层与 UI 框架解耦"的 `crate::app`。原 `events.rs` 现在只保留
//! 纯排空逻辑（drain channels + push UIEvent）。
//!
//! 流程：
//! 1. `gpui_app::run` 启动时调一次（见 `crate::gpui_app::run`）；
//! 2. 循环每 100ms 醒来一次 → `update_entity(&model, |m, ctx| { let any = drain(m); if any { ctx.notify(); } })`；
//! 3. entity 已释放（app 退出）时 `update_entity` 返回 `Err`，`break` 出循环；
//! 4. task detached —— 进程退出时随 executor 终止，不暴露 Task handle。
//!
//! 为什么 100ms：搜索/详情/封面/TOC/下载进度/书源健康检查/更新检查 7 条
//! 后台通道都走这个循环统一推动 UI 重绘；100ms 粒度用户感知不到延迟。

use std::time::Duration;

use gpui::{App, AppContext, Entity};

use crate::app::AppModel;
use crate::app::events::drain;

/// 在 GPUI app 启动时调一次：`spawn` 一个循环任务，每 100ms 排空一次 AppModel。
///
/// 调用上下文：必须在 `Application::run(|cx: &mut App| { ... })` 的闭包内，
/// 在 `open_window` 前后调都可以。
///
/// 任务 detached：返回 `()`，不暴露 Task handle，进程退出时随 executor 终止。
pub fn spawn_drain_loop(model: Entity<AppModel>, cx: &mut App) {
    cx.spawn(async move |async_cx: &mut gpui::AsyncApp| {
        loop {
            // 用 background executor 的 timer：跨 await 友好，不阻塞前台。
            async_cx
                .background_executor()
                .timer(Duration::from_millis(100))
                .await;

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
