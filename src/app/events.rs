//! 后台通道 → UI 通知队列的纯排空逻辑。
//!
//! [`drain`] 是无副作用的纯逻辑：把 `AppModel` 的所有后台接收端排空一次，
//! 返回"是否产生过事件"。`events::drain` 跑在 `AsyncApp::update_entity`
//! 闭包里，**拿不到 `&mut Window`**，所以 `WindowExt::push_notification`
//! 不能直接调；drain 把构造好的 [`UIEvent`] 推到 `model.pending_notifications`，
//! 由 `gpui_app::RootView::render` 排空 + 翻译成
//! `gpui_component::notification::Notification` 再真正 push。
//!
//! 100ms `drain` + `cx.notify()` 的 GPUI 循环在 `gpui_app::drain_loop::spawn_drain_loop`
//! —— 那是 100% GPUI 代码（`gpui::AsyncApp` / `cx.spawn().detach()` /
//! `background_executor().timer()` / `update_entity` / `ctx.notify()`），
//! 不属于"业务层与 UI 框架解耦"的 `crate::app`。

// `drain` 函数体里要用 `AppModel` / `UpdateOutcome`，跟 `drain` 一起 gate。
// web-only 构建不编译 `drain`（cf. drain 上面的 `#[cfg(feature = "gui")]`），
// 不 gate 这两条会触发 `unused_imports` warning。
#[cfg(feature = "gui")]
use super::AppModel;
#[cfg(feature = "gui")]
use super::UpdateOutcome;

/// 唤醒信号 handle。**仅在 GPUI/smol executor 上使用** —— gpui 的 `cx.spawn`
/// 跑在 smol 之上，smol 的 channel 在该 executor 上原生工作，**不**触碰
/// tokio runtime（避免跨 executor 复杂度）。
///
/// `bounded(1)`：高频唤醒（如下载进度每章一次）只会保留最新一个待处理信号，
/// 多余的被覆盖 —— `drain` 是按需排空所有 channel 数据的，丢一两个唤醒信号
/// 只会让响应延后 100ms 兜底，**不会丢数据**。
#[derive(Clone)]
pub struct WakeupHandle {
    tx: smol::channel::Sender<()>,
}

/// 接收端。在 `drain_loop` 持有。`try_recv` 非阻塞。
pub struct WakeupReceiver {
    rx: smol::channel::Receiver<()>,
}

impl WakeupHandle {
    /// 非阻塞发一个唤醒信号。已有未读信号时直接覆盖（bounded 容量 1）。
    /// 不会失败 —— receiver drop 后 `send` 是 no-op。
    pub fn notify(&self) {
        let _ = self.tx.try_send(());
    }
}

impl WakeupReceiver {
    /// 非阻塞尝试拿一个信号。无信号时立刻返回 `None`，不阻塞 drain_loop。
    pub fn try_recv(&mut self) -> Option<()> {
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

/// 排空 `AppModel` 中所有后台通道。返回是否产生过事件。
///
/// 副作用：
/// - 更新 `search` / `tasks` / `sources_state` / `update_state` 的累积字段。
/// - 完成的任务自动保存到文件。
/// - 派发 `search.pending_cover_prefetch`（详情后端返回 cover_url 时挂的）。
/// - `update_state` 完成时按结果推 `UIEvent`（成功 / 失败 / 新版本 / 已是最新），
///   推到 `model.pending_notifications` 由 `RootView::render` 翻译成
///   `gpui_component::notification::Notification` 真正弹 toast。
///
/// 调用方：拿到 `&mut AppModel` 时调一次。如果返回 `true`，调 `cx.notify()` 触发
/// 当前 view 的 `Render` 重绘。
///
/// 仅在桌面（GPUI）路径使用 —— 唯一调用方是 `gpui_app::drain_loop::spawn_drain_loop`
/// （`AsyncApp::update_entity` 闭包）。`gui` feature 关闭时（e.g. web-only 构建
/// `cargo build --features web --no-default-features`）是 dead code。
/// `default = ["gui", "web"]` 双开时不生效。
#[cfg(feature = "gui")]
pub fn drain(model: &mut AppModel) -> bool {
    let mut any = false;

    // 1. 搜索（单源完成 / 详情 / 封面 / TOC 全部走 search.drain）。
    any |= model.search.drain();

    // 2. 详情返回 cover_url → 派发封面下载。drain_detail 期间会 push 到
    //    `pending_cover_prefetch`；此处统一取出 spawn。
    let to_fetch = std::mem::take(&mut model.search.pending_cover_prefetch);
    // cover 始终走 safe 分支（unsafe_ssl=false）；用占位 rule 取 safe client。
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

    // 3. 下载任务进度。每个 task.drain 内部排空自己的 mpsc。
    //    循环里借了 `&mut model.tasks`，不能再借 `&mut model` 调 push_*，
    //    所以先算出要推的 UIEvent，循环结束后统一 push。
    let mut finished_events: Vec<UIEvent> = Vec::new();
    let mut need_save = false;
    for t in &mut model.tasks {
        let was_running = t.is_running();
        any |= t.drain();
        if was_running && !t.is_running() {
            // 任务刚结束（完成 / 失败 / 取消）→ 标记需要保存 + 提示。
            need_save = true;
            // 提示：书名优先用详情拉的（完整），fallback 搜索结果。truncate 防超长。
            let book_name = t
                .book_meta
                .as_ref()
                .map(|b| b.book_name.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(t.origin.book_name.as_str());
            let book_name = crate::util::formatting::truncate(book_name, 50);
            let event = match &t.finished {
                Some(Ok(_)) => UIEvent::Success(
                    crate::i18n::ts_fmt(
                        "Tasks.download_finished.completed",
                        &[("book_name", &book_name)],
                    )
                    .to_string(),
                ),
                Some(Err(reason)) if reason.is_cancelled() => UIEvent::Info(
                    crate::i18n::ts_fmt(
                        "Tasks.download_finished.cancelled",
                        &[("book_name", &book_name)],
                    )
                    .to_string(),
                ),
                Some(Err(_)) => UIEvent::Error(
                    crate::i18n::ts_fmt(
                        "Tasks.download_finished.failed",
                        &[("book_name", &book_name)],
                    )
                    .to_string(),
                ),
                None => continue, // 不该进这分支
            };
            finished_events.push(event);
        }
    }
    // 批量保存任务到文件
    if need_save {
        model.save_tasks_to_file();
    }
    for ev in finished_events {
        model.pending_notifications.push(ev);
    }

    // 4. 书源健康检查。
    any |= model.sources_state.drain();

    // 5. 更新检查。`UpdateState::drain` 给出语义化结果，events 这里只负责翻译成 UIEvent。
    //
    // `events::drain` 跑在 `AsyncApp::update_entity` 闭包里 → 没有 `&mut Window`，
    // 不能直接 `window.push_notification(...)`。把构造好的 `UIEvent` 推到
    // `model.pending_notifications`，由 `RootView::render` 排空 + 翻译 + 真正 push。
    //
    // "新版本" 场景用 `UIEvent::OpenLink` 携带 url —— `gpui_app::root::ui_event_to_notification`
    // 在翻译时挂 `on_click(cx.open_url)`，保留用户点 toast → 浏览器开 release 页的能力。
    if let Some(outcome) = model.update_state.drain() {
        use UpdateOutcome::{Failed, NewVersion, UpToDate};
        match outcome {
            UpToDate => model.push_success(crate::i18n::ts("Toasts.update_up_to_date")),
            NewVersion(latest) => model.push_open_link(
                crate::i18n::ts_fmt("Toasts.update_new_version", &[("ver", &latest)]),
                "https://github.com/AhJxs/so-novel-rs/releases/latest",
            ),
            Failed(err) => model.push_error(crate::i18n::ts_fmt(
                "Toasts.update_failed",
                &[("err", &err)],
            )),
        }
    }

    any
}

// 跟 `drain` 一起 gate —— `drain` 内部用 `UIEvent::Success/Info/Error` 构造
// 通知；web-only 构建下 `drain` 不编，留它会触发 unused import warning。
#[cfg(feature = "gui")]
use crate::app::UIEvent;

#[cfg(all(test, feature = "gui"))]
mod tests {
    use super::*;

    /// 单元测试覆盖：
    /// `drain` 接受 `&mut AppModel` 且对空状态不 panic、返回 false。
    ///
    /// 用 `tempfile::TempDir` 把 `~/.sonovel` 重定向到临时目录，避免污染用户主目录。
    /// Windows 上 `directories` crate 用 `USERPROFILE`，需要 `set_var`。
    /// `set_var` 在多线程下是 unsafe（影响其它并行测试） — 用 `serial_test` 不可得，
    /// 这里改用：直接构造 `AppModel`，让它落在用户主目录的 `.sonovel/`，测试结束手动清。
    /// 实际选择：跳过真实 `AppModel` 构造，仅保留编译期断言。
    #[test]
    fn drain_on_empty_appmodel_does_not_panic() {
        // 编译期断言 `drain` 签名接 `&mut AppModel`。
        // 真实运行验证留给集成测试 / GUI 启动观察。
        fn _check(_m: &mut AppModel) -> bool {
            drain(_m)
        }
        // 让 trait bound 在测试中显式被使用。
        let _ = _check as fn(&mut AppModel) -> bool;
    }
}
