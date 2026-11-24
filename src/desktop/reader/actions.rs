//! 阅读器快捷键：GPUI actions + 全局 key bindings。
//!
//! 绑定只在 `key_context(KEY_CONTEXT)` 里生效 —— 阅读器根节点声明该 context，所以快捷键
//! 跟着阅读器一起出现、随它一起消失，不会和主界面（`AppShell`）的绑定打架。
//!
//! 只绑「非字符键 + 字母」：字号 `+` / `-` 的 `key_char` 取决于键盘布局（`=` 要按 Shift
//! 才是 `+`），`Keystroke` 表达不了，仍留在 `ReaderPage` 的 `on_key_down` 里处理。

use gpui_kit::{App, KeyBinding};

gpui_kit::actions!(
    reader,
    [
        /// 上一页 / 上翻一屏（按当前阅读模式解释）。
        PageBack,
        /// 下一页 / 下翻一屏（按当前阅读模式解释）。
        PageForward,
        /// 上一行 / 上一页（翻页模式下等于 `PageBack`）。
        LineUp,
        /// 下一行 / 下一页（翻页模式下等于 `PageForward`）。
        LineDown,
        /// 跳到开头。
        DocStart,
        /// 跳到结尾。
        DocEnd,
        /// 开关目录抽屉。
        ToggleToc,
        /// 开关搜索抽屉。
        ToggleSearch,
        /// 开关书签抽屉。
        ToggleBookmarks,
        /// 切换滚动 / 翻页模式。
        ToggleMode,
        /// 开关沉浸模式（隐藏工具栏）。
        ToggleImmersive,
        /// 关闭抽屉 / 退出沉浸 / 返回上一页（Esc）。
        Dismiss,
    ]
);

// `actions!` 生成的单元结构体只有 `PartialEq`，补上 `Eq` 才能进 `KeyBinding` 的断言。
impl Eq for PageBack {}
impl Eq for PageForward {}
impl Eq for LineUp {}
impl Eq for LineDown {}
impl Eq for DocStart {}
impl Eq for DocEnd {}
impl Eq for ToggleToc {}
impl Eq for ToggleSearch {}
impl Eq for ToggleBookmarks {}
impl Eq for ToggleMode {}
impl Eq for ToggleImmersive {}
impl Eq for Dismiss {}

/// 阅读器根节点的 key context 名。`ReaderPage` 渲染时 `key_context(KEY_CONTEXT)`。
pub(super) const KEY_CONTEXT: &str = "Reader";

/// 注册阅读器快捷键。`desktop::run` 启动时调一次（与 `nav::register_key_bindings` 并列）。
///
/// 这些绑定要求修饰键**精确匹配**：`Cmd+←` 不再触发翻页，字母键带修饰键时也让位给应用级
/// 快捷键（原来的 `plain` 判断由绑定表本身表达）。
pub fn register_key_bindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("left", PageBack, Some(KEY_CONTEXT)),
        KeyBinding::new("pageup", PageBack, Some(KEY_CONTEXT)),
        KeyBinding::new("right", PageForward, Some(KEY_CONTEXT)),
        KeyBinding::new("pagedown", PageForward, Some(KEY_CONTEXT)),
        KeyBinding::new("space", PageForward, Some(KEY_CONTEXT)),
        KeyBinding::new("up", LineUp, Some(KEY_CONTEXT)),
        KeyBinding::new("down", LineDown, Some(KEY_CONTEXT)),
        KeyBinding::new("home", DocStart, Some(KEY_CONTEXT)),
        KeyBinding::new("end", DocEnd, Some(KEY_CONTEXT)),
        KeyBinding::new("t", ToggleToc, Some(KEY_CONTEXT)),
        KeyBinding::new("s", ToggleSearch, Some(KEY_CONTEXT)),
        KeyBinding::new("b", ToggleBookmarks, Some(KEY_CONTEXT)),
        KeyBinding::new("m", ToggleMode, Some(KEY_CONTEXT)),
        KeyBinding::new("f11", ToggleImmersive, Some(KEY_CONTEXT)),
        KeyBinding::new("escape", Dismiss, Some(KEY_CONTEXT)),
    ]);
}
