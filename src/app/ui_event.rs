//! 业务层 → UI 层的事件枚举。
//!
//! Plain data，**零 GUI 依赖**（不 import `gpui` / `gpui_component`）——
//! 让 `crate::app` 保持与 UI 框架解耦（详见 `src/lib.rs` 顶部注释）。
//!
//! 流向：
//! 1. 业务方法（`AppModel::push_*` / `events::drain` 内部）push `UIEvent` 到
//!    `AppModel::pending_notifications`；
//! 2. `gpui_app::root::render` 每帧排空该队列，调
//!    `ui_event_to_notification` 翻译成 `gpui_component::notification::Notification`，
//!    再 `window.push_notification(...)` 真正弹 toast。
//!
//! 为什么有 `OpenLink`：旧实现里"有新版本"toast 挂了
//! `on_click(|_,_,cx| cx.open_url("https://github.com/.../releases/latest"))`，
//! 是用户拿到新版本号后一键跳到 release 页的关键交互。`Info`/`Success`/
//! `Warning`/`Error` 都是不可点的纯文本 toast，承载不了这种语义。
//! `OpenLink` 是 "可点 toast" 的通用载体 —— 后续如果有"打开本地文件"/
//! "打开书源主页" 等需求，同一 variant 直接复用。
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
    /// 可点击 toast —— 消息用 `message` 渲染，点击触发 `cx.open_url(url)`。
    /// 翻译层（`gpui_app::root::ui_event_to_notification`）负责挂 `on_click`。
    OpenLink { message: String, url: String },
}

#[cfg(test)]
mod tests {
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
