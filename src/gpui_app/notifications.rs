//! `UIEvent` → `gpui_component::Notification` 翻译层。
//!
//! `app/` 是 UI 框架解耦的, 把意图 (要弹什么 toast) 以 plain enum 推到
//! `AppModel::pending_notifications`; UI 层 `RootView::render` 拿到 `&mut Window` 后
//! 把每个 `UIEvent` 翻译成 `Notification` 再 `window.push_notification(...)`。
//!
//! 翻译层放 `gpui_app/`: `Notification::on_click` / `cx.open_url` 是 UI 框架 API,
//! 跨过去就破坏"app/ 零 GUI 依赖"。`OpenLink` 变体的 `on_click` 在这里挂。

use gpui_component::notification::Notification;

use crate::app::UIEvent;

/// 把 `UIEvent` 翻译为 `gpui_component::Notification`, 准备 `window.push_notification(...)`。
///
/// `OpenLink` 变体的 `on_click` 在这里挂 `cx.open_url(&url)` —— 这一步只能在拿到
/// `App` 上下文时执行, 所以翻译必须发生在 UI 层。
#[tracing::instrument(name = "notifications::ui_event_to_notification", skip_all)]
pub(super) fn ui_event_to_notification(ev: UIEvent) -> Notification {
    match ev {
        UIEvent::Info(s) => Notification::info(s),
        UIEvent::Success(s) => Notification::success(s),
        UIEvent::Warning(s) => Notification::warning(s),
        UIEvent::Error(s) => Notification::error(s),
        UIEvent::OpenLink { message, url } => Notification::new()
            .message(message)
            .on_click(move |_ev, _window, cx| {
                cx.open_url(&url);
            })
            .autohide(true),
    }
}