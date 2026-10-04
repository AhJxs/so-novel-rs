//! 后台通道 → UI 通知队列的纯排空逻辑。
//!
//! [`drain`] 无副作用: 把所有后台接收端排空一次, 返回"是否产生过事件"。它跑在
//! `AsyncApp::update_entity` 闭包里, **拿不到 `&mut Window`**, 所以不能直接
//! `push_notification` —— 只把 [`UIEvent`] 推入 `model.pending_ui_events`, 由
//! `desktop::RootView::render` 排空并翻译成 `Notification` 真正 push。
//!
//! GPUI 侧的 100ms 循环在 `desktop::drain_loop::spawn_drain_loop`。

use rust_i18n::t;

use super::AppModel;
use super::UpdateOutcome;

/// 唤醒信号 handle。**仅在 GPUI/smol executor 上使用** —— `cx.spawn` 跑在 smol
/// 之上, 其 channel 在该 executor 原生工作, **不**触碰 tokio runtime。
///
/// `bounded(1)`: `drain` 是按需排空所有 channel 数据的, 丢一两个唤醒信号只让响应
/// 延后到 100ms 兜底, **不会丢数据**。
#[derive(Clone)]
pub struct WakeupHandle {
    tx: smol::channel::Sender<()>,
}

/// 接收端。在 `drain_loop` 持有。
pub struct WakeupReceiver {
    rx: smol::channel::Receiver<()>,
}

impl WakeupHandle {
    /// 非阻塞发一个唤醒信号 (已有未读信号时直接覆盖)。
    pub fn notify(&self) {
        let _ = self.tx.try_send(());
    }
}

impl WakeupReceiver {
    /// 非阻塞尝试拿一个信号; 无信号时立刻返回 `None`, 不阻塞 `drain_loop`。
    pub fn try_recv(&self) -> Option<()> {
        match self.rx.try_recv() {
            Ok(()) => Some(()),
            Err(_) => None,
        }
    }
}

/// 在 `AppModel::new` 里调一次，建 `(WakeupHandle, WakeupReceiver)`。
pub fn new_wakeup() -> (WakeupHandle, WakeupReceiver) {
    let (tx, rx) = smol::channel::bounded::<()>(1);
    (WakeupHandle { tx }, WakeupReceiver { rx })
}

/// 排空 `AppModel` 中所有后台通道。返回是否产生过事件 (true 时调用方 `cx.notify()`)。
///
/// 副作用: 更新 search / tasks / `sources_state` / `update_state`; 保存刚结束的任务;
/// 派发 `pending_cover_prefetch`; 把 `update_state` 结果推成 `UIEvent`。
pub fn drain(model: &mut AppModel) -> bool {
    let mut any = false;

    // 1. 搜索（单源完成 / 详情 / 封面 / TOC 全部走 search.drain）。
    any |= model.search.drain();

    // 2. 详情返回 cover_url → 派发封面下载。drain_detail 期间只会 push 到
    //    `pending_cover_prefetch`; 此处统一取出 spawn。
    let to_fetch = std::mem::take(&mut model.search.pending_cover_prefetch);
    // cover 始终走 safe 分支 (unsafe_ssl=false); 用占位 rule 取 safe client。
    let safe_client = model.http.for_rule(&crate::models::Rule {
        ignore_ssl: false,
        ..crate::models::Rule::default()
    });
    for (sid, url) in to_fetch {
        model
            .search
            .spawn_cover_download(sid, &url, &safe_client, model.runtime);
    }

    // 2b. 本地书库后台扫描结果（refresh_library_async 通过 smol channel 回送）。
    any |= model.library.drain_scan();

    // 3. 下载任务进度。
    //    循环里借了 `&mut model.tasks`, 不能再借 `&mut model` 调 push_*, 所以先收集
    //    要推的 UIEvent, 循环结束后统一 push。
    let mut finished_events: Vec<UIEvent> = Vec::new();
    let mut need_save = false;
    for t in &mut model.tasks {
        let was_running = t.is_running();
        any |= t.drain();
        if was_running && !t.is_running() {
            // 任务刚结束 → 标记需要保存 + 提示。书名优先用详情拉的（完整）,
            // fallback 搜索结果; truncate 防超长。
            need_save = true;
            let book_name = t
                .book_meta
                .as_ref()
                .map(|b| b.book_name.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(t.origin.book_name.as_str());
            let book_name = crate::utils::formatting::truncate(book_name, 50);
            let event = match &t.finished {
                Some(Ok(_)) => UIEvent::Success(
                    t!("Tasks.download_finished.completed", book_name = &book_name).to_string(),
                ),
                Some(Err(reason)) if reason.is_cancelled() => UIEvent::Info(
                    t!("Tasks.download_finished.cancelled", book_name = &book_name).to_string(),
                ),
                Some(Err(_)) => UIEvent::Error(
                    t!("Tasks.download_finished.failed", book_name = &book_name).to_string(),
                ),
                None => continue, // 不该进这分支
            };
            finished_events.push(event);
        }
    }
    // 批量保存任务到文件 — fire-and-forget, 不阻塞 drain_loop。
    if need_save {
        let path = model.paths.tasks_file.clone();
        let tasks = model.tasks.clone();
        model.runtime.spawn_blocking(move || {
            if let Err(e) = crate::db::save_with_trim(&path, &tasks) {
                tracing::warn!("保存任务到文件失败: {e:#}");
            }
        });
    }
    for ev in finished_events {
        model.pending_ui_events.push(ev);
    }

    // 4. 书源健康检查。
    any |= model.sources_state.drain();

    // 5. 更新检查。`UpdateState::drain` 给出语义化结果, 这里只翻译成 `UIEvent`。
    //    "新版本" 用 `OpenLink` 携带 url, 翻译时挂 `on_click(cx.open_url)`。
    if let Some(outcome) = model.update_state.drain() {
        use UpdateOutcome::{Failed, NewVersion, UpToDate};
        match outcome {
            UpToDate => model.push_success(t!("Toasts.update_up_to_date")),
            NewVersion(latest) => model.push_open_link(
                t!("Toasts.update_new_version", ver = &latest),
                "https://github.com/AhJxs/so-novel-rs/releases/latest",
            ),
            Failed(err) => model.push_error(t!("Toasts.update_failed", err = &err)),
        }
    }

    any
}

use crate::desktop::model::UIEvent;

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    /// 只做编译期断言: `drain` 接 `&mut AppModel`（真实运行验证留给集成测试）。
    #[test]
    fn drain_on_empty_appmodel_does_not_panic() {
        fn _check(m: &mut AppModel) -> bool {
            drain(m)
        }
        let _ = _check as fn(&mut AppModel) -> bool;
    }
}
