//! 下载控制选项 (PR #17 拆分, 2026-07-08).
//!
//! `DownloadOptions` 是 `download_*` 函数的入参壳, 包含:
//! - `progress`: 进度事件发送端 (mpsc, 一次构造后可 clone)
//! - `cancel`: 取消令牌 (UI / CLI 共享, 内部 `Arc<AtomicBool>`)
//! - `notify`: 可选 wakeup 回调, 每次 progress.send() 后立即触发,
//!   让 GPUI `drain_loop` 不等 100ms poll 周期

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{Notify, mpsc};

use super::progress::Progress;

/// 控制下载行为的选项 (与 `AppConfig` 的字段在 UI 层做映射)。
///
/// # Examples
///
/// ```ignore
/// let opts = DownloadOptions {
///     progress: tx,
///     cancel: CancelToken::new(),
///     notify: Some(Arc::new(|| wakeup.notify())),
/// };
/// ```
pub struct DownloadOptions {
    /// 进度事件发送端。Clone 后可以多次持有 (mpsc 内部 `Arc`)。
    pub progress: mpsc::UnboundedSender<Progress>,
    /// 取消令牌。在 UI / CLI 侧 clone 一份, set 后下一次检查点会停止。
    pub cancel: CancelToken,
    /// 可选 wakeup 回调: 每次 `progress.send()` 后调用,
    /// 让 GPUI 的 `drain_loop` 立即 pick up 事件, 不必等 100ms poll 周期。
    pub notify: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// 取消令牌: 在 UI / CLI 侧 clone 一份, set 后下一次检查点会停止。
///
/// 内部同时持有 `AtomicBool` (同步检查) 和 `tokio::sync::Notify` (异步唤醒)。
/// `cancel()` 设置 flag 并唤醒所有 `wait_cancelled()` 等待者, 响应 <1ms。
///
/// # Examples
///
/// ```
/// let ct = CancelToken::new();
/// let ct2 = ct.clone();
/// tokio::spawn(async move { ct2.cancel(); });
/// ct.wait_cancelled().await;  // 立即返回
/// ```
#[derive(Clone)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelToken {
    /// 创建未触发的令牌。
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// 触发取消: 设置 flag + 唤醒所有等待者, 响应 <1ms。
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// 同步检查是否已触发。
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// 异步等待取消信号。比 50ms poll 循环快得多 (<1ms 响应)。
    pub async fn wait_cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }
}
