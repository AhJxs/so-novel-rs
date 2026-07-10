//! Library 文件系统 watcher：long-lived task 持有 `notify::RecommendedWatcher`，
//! 监听 `config.download.download_path` 增量（`Create`/`Modify`/`Remove`）。
//!
//! 主循环在 smol executor 上跑：
//! 1. 300 ms `timer` 醒来
//! 2. drain cmd channel（处理 `SetPath` / `Stop`）→ drop 旧 watcher → arm 新路径
//! 3. 看 fs 事件 counter 变化 → 有变就 `refresh_library_async` + `cx.notify()`
//!
//! **不能用** `tokio::time::sleep` / `tokio::sync::mpsc` —— gpui 的 smol-based
//! executor 上没有 tokio reactor，会 panic。通道用 `smol::channel`（基于 `async-channel`），
//! 定时器走 `async_cx.background_executor().timer(...)`。这跟 gpui-component 内部
//! `ThemeRegistry::watch_dir` 的选型一致。
//!
//! **debounce**：300 ms 心跳周期天然就是 debounce —— 一次写文件触发 3~4 个事件
//! （Create + Modify + 2× Rename），间隔 <100 ms，300 ms 内会全部累计进 counter，
//! 下一次心跳一次 rescan 把它们压平。
//!
//! **取消**：`LibraryPage` 析构 → `watcher_cmd_tx: Sender` drop →
//! `watcher_cmd_rx.try_recv()` 返回 `Err(Closed)` → break 退出循环。
//! `notify::Watcher` 随 `_watcher` 局部变量 drop → 释放 OS handle。

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gpui::{AsyncApp, Entity, WeakEntity};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::desktop::model::AppModel;

/// Watcher 任务命令：让任务内部 drop 旧 watcher 并 arm 到新路径上。
///
/// 当前只有 `SetPath` 一个调用方（`maybe_auto_scan` 检测到 `download_path` 变了 → 发）。
#[derive(Debug, Clone)]
pub(super) enum WatcherCmd {
    SetPath(PathBuf),
}

/// Sender 别名（owner 持有 → 析构时 drop → 任务 `try_recv()` 收 Closed → 退出）。
pub(super) type WatcherCmdTx = smol::channel::Sender<WatcherCmd>;

/// 在调用方的 `cx.spawn` future 内部运行 watcher 主循环。
///
/// 必须由 `cx.spawn(async move |_weak, async_cx| { ... })` 包起来 —— `async_cx` 是
/// gpui 提供的 `&mut AsyncApp`，主循环 await 都在它上面完成。函数签名收 `&mut AsyncApp`
/// 是为了让 watcher 主循环跟 `settings::ctx::PickFolderListener` 那样的 owner-cache
/// 模式保持一致：caller 持有 sender，watcher 模块只关心 receiver 那一头。
///
/// 返回 `Pin<Box<dyn Future>>` 而非 `async fn` 以避开 `clippy::future_not_send` ——
/// `AsyncApp` 内部 `Weak<AppCell>` 是 `!Send`，Gpui 的 foreground executor 也不要求
/// `Send`，所以 `!Send` future 是安全的。
pub(super) fn run(
    initial_path: PathBuf,
    page_weak: WeakEntity<super::LibraryPage>,
    model: Entity<AppModel>,
    watcher_cmd_rx: smol::channel::Receiver<WatcherCmd>,
    async_cx: &mut AsyncApp,
) -> Pin<Box<dyn Future<Output = ()> + '_>> {
    Box::pin(async move {
        // 事件计数器（每个 fs 事件 +1）。回调线程写入，主任务读取 → Relaxed 即可。
        let counter = Arc::new(AtomicU64::new(0));
        let mut watcher: Option<RecommendedWatcher> = None;

        // helper：arm 当前路径的 watcher。失败仅 warn，不 panic。
        let arm = |path: PathBuf, counter: Arc<AtomicU64>| -> Option<RecommendedWatcher> {
            let counter_for_cb = counter;
            let mut w =
                match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    // 只对文件增删改计数，忽略 Access / Metadata / CloseWrite 等 inotify 噪音
                    if let Ok(event) = res {
                        match event.kind {
                            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
                            _ => return,
                        }
                    }
                    counter_for_cb.fetch_add(1, Ordering::Relaxed);
                }) {
                    Ok(w) => w,
                    Err(e) => {
                        tracing::warn!("create watcher failed: {e:#}");
                        return None;
                    }
                };
            if let Err(e) = w.watch(&path, RecursiveMode::NonRecursive) {
                tracing::warn!("watch {:?} failed: {e:#}", path);
                return None;
            }
            Some(w)
        };

        // 初始 arm。
        let _ = std::mem::replace(&mut watcher, arm(initial_path, counter.clone()));
        let debounce = Duration::from_millis(300);
        let mut last_seen = 0u64;

        loop {
            // smol-based timer：smol executor 提供的 timer future，
            // 不需要 tokio reactor。在 smol runtime（= gpui 的 `cx.spawn` 内部）
            // 上 await 直接走 smol reactor，没问题。
            // 把 timer 绑定到局部变量再 await —— 让 `async_cx` 的借用立即释放，
            // 避免 `&mut AsyncApp` 跨 await（`AsyncApp` 内部 `Rc/Weak` 非 Send，
            // 跨 await 持有 → future 不满足 `Send`，触发 `clippy::future_not_send`）。
            let timer = async_cx.background_executor().timer(debounce);
            timer.await;

            // 1) drain cmd channel（处理所有待发的 SetPath / Stop）—— try_recv 非阻塞。
            loop {
                match watcher_cmd_rx.try_recv() {
                    Ok(WatcherCmd::SetPath(new_path)) => {
                        // drop 旧 watcher → 释放 OS handle → arm 新 watcher。
                        let _ = watcher.take();
                        let new_w = arm(new_path.clone(), counter.clone());
                        let _ = std::mem::replace(&mut watcher, new_w);
                        // 切路径后立即 rescan（用户在 Settings 切完路径想马上看到新目录内容）。
                        // 改用 async 版本：read_dir / metadata 阻塞 IO 不再卡 UI 帧。
                        let _ = page_weak.update(async_cx, |_p, cx| {
                            model.update(cx, |m, _cx| {
                                m.refresh_library_async();
                            });
                            cx.notify();
                        });
                    }
                    Err(smol::channel::TryRecvError::Empty) => break, // 队列空，跳出内层循环
                    Err(smol::channel::TryRecvError::Closed) => return, // sender drop → 整个任务退出
                }
            }

            // 2) 检查 counter：有新事件 → rescan + notify。
            let now = counter.load(Ordering::Relaxed);
            if now != last_seen {
                last_seen = now;
                // 如果刚发生删除（delete_library_entry 置了 1s skip 窗口），
                // 跳过此次 rescan —— 避免 entries.clear() + 后台 fill 制造的
                // "empty → 重新加载" 闪一下。1s 后窗口过期，正常的 add/modify
                // 事件仍会触发 rescan。
                let skip_due_to_delete = page_weak
                    .update(async_cx, |_p, cx| {
                        model.read(cx).library.watcher_skip_until_unix_ms
                    })
                    .unwrap_or(None)
                    .is_some_and(|until| {
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |d| d.as_millis() as u64);
                        now_ms < until
                    });
                if !skip_due_to_delete {
                    let _ = page_weak.update(async_cx, |_p, cx| {
                        model.update(cx, |m, _cx| {
                            m.refresh_library_async();
                        });
                        cx.notify();
                    });
                }
            }
        }
    })
}
