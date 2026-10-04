//! 5 个一级导航页面 (`NavPage`) + 全局 key bindings + GPUI actions 注册。
//!
//! - `NavPage` enum + label/icon/next/prev helpers;
//! - [`register_key_bindings`] 在 `desktop::run` 启动时调一次 (actions 声明在 `desktop` 顶层)。
//!
//! 翻页不用 `Ctrl+Tab`: `gpui_kit::component` 的 `InputState` 把 `tab` / `shift-tab` 绑到
//! 自己的 `IndentInline` / `OutdentInline`, 焦点在 Input 时 Tab 事件被消费, 应用级翻页
//! action 拿不到。改用 `F6` 避开。

use gpui_kit::component::IconName;
use gpui_kit::{App, KeyBinding, SharedString};
use rust_i18n::t;

// `actions!` 宏在 `crate::desktop` (mod.rs) 调用, 生成的 action 类型位于 `desktop::*`,
// 这里只 re-export 给 root.rs 用。
pub(super) use crate::desktop::{
    NextPage, PrevPage, ShowLibrary, ShowSearch, ShowSettings, ShowSources, ShowTasks,
    ToggleSidebar,
};

/// GPUI key context 名。`div().key_context("AppShell")` 时激活。
pub(super) const KEY_CONTEXT: &str = "AppShell";

/// 5 个一级导航页面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavPage {
    #[default]
    Search,
    Tasks,
    Library,
    Sources,
    Settings,
}

impl NavPage {
    /// `NavPage` → i18n key (`t!` 用)。
    pub(super) const fn label_key(self) -> &'static str {
        match self {
            Self::Search => "Nav.search",
            Self::Tasks => "Nav.tasks",
            Self::Library => "Nav.library",
            Self::Sources => "Nav.sources",
            Self::Settings => "Nav.settings",
        }
    }

    /// 当前应用语言下的用户可见 label —— `t!` 走全局 locale (语言切换时由
    /// `gpui_kit::component::set_locale` 同步), 所以这里不需要 `lang` 参数。
    pub(super) fn label(self) -> SharedString {
        t!(self.label_key()).into()
    }

    pub(super) const fn icon(self) -> IconName {
        match self {
            Self::Search => IconName::Search,
            Self::Tasks => IconName::Inbox,
            Self::Library => IconName::BookOpen,
            Self::Sources => IconName::Globe,
            Self::Settings => IconName::Settings,
        }
    }

    pub(super) const ALL: [Self; 5] = [
        Self::Search,
        Self::Tasks,
        Self::Library,
        Self::Sources,
        Self::Settings,
    ];

    /// 下一个 page (循环)。
    pub(super) fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    /// 上一个 page (循环)。
    pub(super) fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|p| *p == self).unwrap_or(0);
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// 全局 key bindings 注册。`desktop::run` 启动时调一次。
///
/// `cmd-1..5` 跳 5 个 page, `F6`/`Shift+F6` 循环翻页, `cmd-b` 折叠 sidebar。
#[tracing::instrument(name = "nav::register_key_bindings", skip_all)]
pub fn register_key_bindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-1", ShowSearch, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-2", ShowTasks, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-3", ShowLibrary, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-4", ShowSources, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-5", ShowSettings, Some(KEY_CONTEXT)),
        KeyBinding::new("f6", NextPage, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f6", PrevPage, Some(KEY_CONTEXT)),
        KeyBinding::new("cmd-b", ToggleSidebar, Some(KEY_CONTEXT)),
    ]);
}
