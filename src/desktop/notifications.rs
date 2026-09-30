//! `UIEvent` → `gpui_kit::component::Notification` 翻译层。
//!
//! `app/` 与 UI 框架解耦, 只把意图以 plain enum 推到 `AppModel::pending_ui_events`;
//! UI 层 `RootView::render` 拿到 `&mut Window` 后在这里翻译并 `push_notification`。
//! 翻译必须放 UI 层: `Notification::on_click` / `cx.open_url` 是 UI 框架 API,
//! `OpenLink` 变体的 `on_click` 只能在拿得到 `App` 上下文时挂。

use gpui_kit::component::notification::Notification;

use crate::desktop::model::UIEvent;

/// 把 `UIEvent` 翻译为 `gpui_kit::component::Notification`, 准备 `window.push_notification(...)`。
///
/// `OpenLink` 变体在这里挂 `cx.open_url(&url)`, 因此翻译必须发生在 UI 层。
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
