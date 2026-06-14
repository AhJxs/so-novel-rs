//! 顶部状态栏临时消息 (toast) 的类型与方法。

/// Toast 类型，决定导航栏 pill 的配色。
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub enum ToastKind {
    /// 默认 — 通用提示（蓝色 ACCENT）
    #[default]
    Info,
    /// 成功（绿色）
    Success,
    /// 警告（橙色）
    Warn,
    /// 错误（红色）
    Error,
}
