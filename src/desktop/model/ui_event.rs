//! 业务层 → UI 层的事件枚举。
//!
//! Plain data, **零 GUI 依赖**（不 import `gpui_kit`）—— 让 `crate::desktop::model`
//! 与 UI 框架解耦。
//!
//! 流向: 业务方法 push 到 `AppModel::pending_ui_events` → `desktop::root` 每帧排空,
//! 翻译成 `gpui_kit::component::notification::Notification` 后 `push_notification`。
//!
//! `OpenLink` 承载"可点 toast": `Info` / `Success` / `Warning` / `Error` 都是不可点
//! 纯文本, 表达不了"点一下跳 release 页"这类交互。
#[derive(Debug, Clone)]
pub enum UIEvent {
    /// 普通提示，蓝色 icon。例："已是最新版本"。
    Info(String),
    /// 成功提示，绿色 icon。例："下载完成：凡人修仙传"。
    Success(String),
    /// 警告提示，黄色 icon。例："有新版本 v0.3.0"（伴随 `OpenLink` 用）。
    Warning(String),
    /// 错误提示，红色 icon。例："下载失败：网络超时"。
    Error(String),
    /// 可点击 toast —— 消息用 `message` 渲染, 点击触发 `cx.open_url(url)`
    /// （`on_click` 由 `desktop::root::ui_event_to_notification` 挂）。
    OpenLink { message: String, url: String },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
    use super::*;

    #[test]
    fn info_carries_string() {
        let e = UIEvent::Info("hello".into());
        match e {
            UIEvent::Info(s) => assert_eq!(s, "hello"),
            _ => panic!("expected Info"),
        }
    }

    #[test]
    fn open_link_carries_message_and_url() {
        let e = UIEvent::OpenLink {
            message: "click me".into(),
            url: "https://example.com".into(),
        };
        match e {
            UIEvent::OpenLink { message, url } => {
                assert_eq!(message, "click me");
                assert_eq!(url, "https://example.com");
            }
            _ => panic!("expected OpenLink"),
        }
    }

    #[test]
    fn variants_are_distinct() {
        let a = UIEvent::Success("x".into());
        let b = UIEvent::Warning("x".into());
        assert!(matches!(a, UIEvent::Success(_)));
        assert!(matches!(b, UIEvent::Warning(_)));
        assert_ne!(std::mem::discriminant(&a), std::mem::discriminant(&b));
    }
}
