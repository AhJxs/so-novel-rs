//! 5 个一级导航页面 (`NavPage`) + 全局 key bindings + GPUI actions 注册。
//!
//! 主流程 [`RootView`] 在 [`super::root`], 这里只负责:
//! - `NavPage` enum + label/icon/next/prev helpers;
//! - `actions!` 宏声明 8 个 GPUI actions;
//! - [`register_key_bindings`] 在 `desktop::run` 启动时调一次。
//!
//! ## Key bindings 选择
//!
//! 翻页不用 `Ctrl+Tab`: gpui-component `InputState` 把 `tab` / `shift-tab` 绑到自己的
//! `IndentInline` / `OutdentInline` 动作 (多行输入 tab 插入), 焦点在 Input 时 Tab 事件
//! 被 Input 消费 (某些平台连 `ctrl-tab` 也被 keydown handler stop 冒泡), 应用级翻页
//! action 拿不到。改用 `F6` 避开。

use gpui::{App, KeyBinding, SharedString};
use gpui_component::IconName;

use crate::i18n::ts;

// `actions!` 宏在 [`crate::desktop`] 顶层 (mod.rs) 调用, 生成的 8 个 action 类型
// 位于 `desktop::*`。这里只 re-export 给 root.rs 用。
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
    /// `NavPage` → i18n key (`i18n::tr` 用)。
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
    /// `gpui_component::set_locale` 同步), 所以这里不需要 `lang` 参数。
    pub(super) fn label(self) -> SharedString {
        ts(self.label_key())
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

    /// 5 个 page 循环顺序: Search → Tasks → Library → Sources → Settings → Search。
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
