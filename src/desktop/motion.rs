//! UI 过渡动画的统一参数。
//!
//! 应用层只主动加三处动效：一级页面换场、阅读器进出、小面积内容（阅读器抽屉、
//! 书架卡片 hover）。节奏与曲线全部取自主题的 motion token，组件库调整 token 时
//! 这里跟着变，不在各页面里散落魔法数字。
//!
//! `Presence` / `transition` 内部已经尊重 `cx.reduce_motion()`：系统开启「减少
//! 动态效果」时动画直接瞬时到位，调用方不用再分支。

use gpui_kit::base::motion::{Presence, PresenceSample, Transition, TransitionId};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Window};

/// 大面积换场：一级页面切换、阅读器打开/关闭。180ms，进入曲线。
pub fn enter(cx: &App) -> Transition {
    let tokens = cx.theme().motion_tokens();
    Transition::new(tokens.duration_normal).easing(tokens.easing_enter.clone())
}

/// 小面积内容：阅读器抽屉淡入、书架卡片 hover。120ms，进入曲线。
pub fn fast(cx: &App) -> Transition {
    let tokens = cx.theme().motion_tokens();
    Transition::new(tokens.duration_fast).easing(tokens.easing_enter.clone())
}

/// 采样一次「出现 / 消失」进度。
///
/// 调用方通常只取 `progress` 当透明度，用 `should_render()` 决定入场结束后是否
/// 还要画；退出时若不想承担布局抖动，可以按自己的状态位立刻停止渲染，进度仍然
/// 连续记录在 keyed state 里，下次入场从当前值接着走。
pub fn fade(
    id: impl Into<TransitionId>,
    present: bool,
    transition: Transition,
    window: &mut Window,
    cx: &mut App,
) -> PresenceSample {
    Presence::new(id, present)
        .transition(transition)
        .sample(window, cx)
}
