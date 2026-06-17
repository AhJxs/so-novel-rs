# 项目 lessons

## 不要在 `cx.spawn` 内部用 tokio 原语

**症状**：`panic at ...: there is no reactor running, must be called from the context of a Tokio 1.x runtime` + `STATUS_STACK_BUFFER_OVERRUN`（Windows 上 panic 跨 FFI 边界 unwind 时退化成 abort）。

**根因**：`gpui::Context::spawn` / `cx.spawn(async move |...| {...})` 把 future 交给 gpui 自己的 executor —— 这个 executor 是基于 **smol**（不是 tokio）实现的（`gpui-0.2.2/src/executor.rs` 顶层 `spawn_local_with_source_location`，底层 `async-task` + smol 风格的 `Runnable`）。

future 内部 await 的 timer / channel 必须用 smol 系：
- 定时器：`async_cx.background_executor().timer(Duration).await` —— 走 smol reactor。
- 通道：`smol::channel::bounded<T>(cap)` —— 内部基于 `async-channel`，收发都是 `async/await` 友好的。
- `try_recv()` 返回 `Result<T, smol::channel::TryRecvError>`，`TryRecvError` 有 `Empty` / `Closed` 两个变体（`Closed` 表示 sender 已 drop → 任务退出）。

**不要用**：
- `tokio::time::sleep` — 没有 tokio reactor。
- `tokio::sync::mpsc::channel` — 同上，且 `tokio::sync::mpsc::Sender::try_send` 的语义跟 smol 不同。

**为什么不用在项目里另起一个 tokio runtime**：项目顶层只有 gpui 一个 runtime；加 tokio runtime 会导致两套 executor 并存，背景任务跨线程调度难以追踪。代价：`tokio = "1"` 已经在 `Cargo.toml` 里（早期为 rfd 的 tokio feature 拉的），现在仅供 rfd 内部用 + 测试代码用。

**正确 pattern（参考）**：
- `src/app/events.rs:97-119` —— `spawn_drain_loop` 用 `async_cx.background_executor().timer(...)` 做 100 ms 心跳。
- `gpui-component-0.5.1/src/theme/registry.rs:171-206` —— `ThemeRegistry::_watch_themes_dir` 用 `smol::channel::bounded(100)` 给 `notify::recommended_watcher` 回调通信。

**Cargo.toml 需要的直接依赖**：`smol = "2"`。项目目前只有间接依赖（gpui-component 0.5.1 → smol 2）。

## `tokio::sync::mpsc::Sender` 不能跨 smol ↔ tokio

跟上一条相关：即便在普通 Rust 函数（非 `cx.spawn` 内部）里建 `tokio::sync::mpsc::Sender`，如果 sender 的对端 receiver 在 `cx.spawn` future 里 `await tokio::sync::mpsc::Receiver::recv()`，同样 panic。始终用 smol 通道。

## `select!` 宏跨 runtime 不可用

smol 没有自己的 `select!` 宏，tokio 的 `tokio::select!` 在 smol future 里也用不上。两种替代：
- 轮询：`loop { timer.await; try_recv().for_each(...) }`（结构化差但简单够用）。
- 真正的 select：`futures::future::FutureExt` 提供 `select`，但 `futures` crate 不在直接依赖里。需要时再加。
