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

**当前 runtime 现状（已与早期 lessons 更新）**：GUI 模式下两套 executor 并存——gpui 的 smol executor 跑 `cx.spawn`（UI 侧），一个 leaked tokio runtime（`src/app/runtime.rs` 的 `build_shared_runtime`，`Box::leak` 永不 drop）跑网络任务（搜索/下载/封面/健康检查）。tokio↔smol 边界靠 `try_recv()`（runtime 无关）桥接：tokio 侧 spawn 后向 mpsc 发事件，smol 侧的 drain 循环 `try_recv` 排空。**关键约束不变**：`cx.spawn` 内部（smol 侧）绝不能用 `tokio::time::sleep` / `tokio::sync::mpsc::recv().await`，会 panic。CLI 模式各自建临时 tokio runtime（`src/cli.rs`），进程退出即销毁。

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

## 在 `InputEvent::Change` 订阅里调 `set_value` 会死循环，耗尽 Windows 句柄配额崩溃

gpui-component 0.5.1 的 `InputState::set_value` → `replace_text` → `cx.emit(InputEvent::Change)`（state.rs:2009）。所以**订阅 `InputEvent::Change` 后在处理器里无条件 `set_value` 会形成 Change→set_value→Change 死循环**。

症状：日志狂刷 `Error { code: HRESULT(0x80070718), message: "配额不足，无法处理此命令。" }`，进程 `exit code: 0xcfffffff`。`0x80070718` = `ERROR_NOT_ENOUGH_QUOTA`（Windows 桌面堆 / 句柄配额耗尽）——死循环每轮创建句柄，几秒内打满。

**正确 pattern**：Change 处理器里只在 clamp / 规整后的值与当前显示值**不同**时才 `set_value`：
```rust
InputEvent::Change => {
    let cur = input.read(cx).value().to_string();
    let want = normalize(&cur);            // clamp / 格式化
    if want != cur {                        // 相等就跳过，断开重入循环
        input.update(cx, |s, cx| s.set_value(want, window, cx));
        cx.notify();
    }
}
```
`set_value` 写回的值已是规整后的，二次 Change 进来时 `want == cur` 直接跳过，循环立即终止。

**对照**：`search.rs` 关键词输入框在 Change 里只更新 model、不调 `set_value`，所以没事；选章起止输入框要在 Change 里 clamp 写回，踩了这个坑。`NumberInputEvent::Step`（按 +/-）不受影响——`set_value` 只 emit `Change` 不 emit `Step`，不会回环。

**通用教训**：任何「订阅事件 A → 在处理器里调用会再次 emit 事件 A 的 API」都要加去重守卫，否则就是隐式递归。GPUI / gpui-component 里 `set_value` / `set_placeholder` 这类带 emit 的 setter 都适用。
