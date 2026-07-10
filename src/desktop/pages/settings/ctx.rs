//! `SettingsPage` 拆分时 owner-cached entity 透传到各 page 模块用。
//!
//! `SettingsPage::new` 在 owner 里建一次 `InputState` / 3× `SelectState` / `SliderState`
//! / 「下载目录」`pick_folder_listener` 闭包并缓存 —— 闭包每帧重建会丢 popup/focus/
//! 选中（详见 settings.rs 老版 `theme_state_static` 注释）。
//!
//! 各 page 模块（`page_general` / `page_crawl` / ...）要复用这些 entity 但不持有
//! `&mut SettingsPage`，所以通过 `&'a PageCtx` 借出。`Entity<T>` 内部是 refcount，
//! `&'a` 借用是 0 拷贝 0 clone（caller 端要 clone 也是 +1 计数）。

use std::rc::Rc;

use gpui::{App, ClickEvent, Entity, SharedString, Window};
use gpui_component::{
    input::InputState,
    select::{SearchableVec, SelectState},
    slider::SliderState,
};

use crate::desktop::model::AppModel;

/// 「下载目录」按钮 click handler 的类型别名（owner-cache 闭包用）。
///
/// `Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>` 在 render 闭包里直接写
/// 太长，用 alias 简化。原 `settings.rs:55` 移到此处 —— 它是 settings 内部用的
/// owner-cache 闭包类型，没必要 expose 到 mod 外。
pub(super) type PickFolderListener = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// 把 owner-cached entity 借给各 page 模块用。
///
/// 8 个字段全是 `&'a` 借出（caller clone 出 owned Entity 进 `SettingField` 闭包，
/// `Entity::clone` 是 refcount 增量，cheap）。`'a` 跟 `SettingsPage` 同生同死。
pub(super) struct PageCtx<'a> {
    pub model: &'a Entity<AppModel>,
    pub theme_state_static: &'a Entity<SelectState<SearchableVec<SharedString>>>,
    pub theme_state_dyn_light: &'a Entity<SelectState<SearchableVec<SharedString>>>,
    pub theme_state_dyn_dark: &'a Entity<SelectState<SearchableVec<SharedString>>>,
    pub font_size_state: &'a Entity<SliderState>,
    pub download_path_input: &'a Entity<InputState>,
    /// 起点 cookie 输入框 — `SettingField::render` 闭包每帧重建会丢
    /// focus / 光标 / 多行 wrap，所以建一次缓存。`multi_line(true).rows(3)`
    /// 配合 `Input::h(px(80.))` 给一块固定高度的 textarea 给用户粘贴整段
    /// `Cookie:` 头。`placeholder("w_tsfp=...")` 提示 cookie 头格式起点。
    pub qidian_cookie_input: &'a Entity<InputState>,
    pub pick_folder_listener: &'a PickFolderListener,
}
