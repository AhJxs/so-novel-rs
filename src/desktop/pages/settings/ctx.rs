//! owner-cached entity 透传到各 page 模块用。
//!
//! `SettingsPage::new` 建一次并缓存 `InputState` / 3× `SelectState` / `SliderState` /
//! `pick_folder_listener` —— 每帧重建会丢 popup / focus / 选中。
//! 各 page 模块要复用这些 entity 但不持有 `&mut SettingsPage`，故用 `&'page` 借出。

use std::rc::Rc;

use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::select::{SearchableVec, SelectState};
use gpui_kit::component::slider::SliderState;
use gpui_kit::{App, ClickEvent, Entity, SharedString, Window};

use crate::desktop::model::AppModel;

/// 「下载目录」按钮 click handler 的类型别名（owner-cache 闭包用），只在本模块内用。
pub(super) type PickFolderListener = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// 把 owner-cached entity 借给各 page 模块用。
///
/// 字段全是 `&'page` 借出（`Entity::clone` 只是 refcount 增量），`'page` 跟 `SettingsPage` 同生同死。
pub(super) struct PageContext<'page> {
    pub model: &'page Entity<AppModel>,
    pub theme_state_static: &'page Entity<SelectState<SearchableVec<SharedString>>>,
    pub theme_state_dyn_light: &'page Entity<SelectState<SearchableVec<SharedString>>>,
    pub theme_state_dyn_dark: &'page Entity<SelectState<SearchableVec<SharedString>>>,
    pub font_size_state: &'page Entity<SliderState>,
    pub download_path_input: &'page Entity<InputState>,
    /// 起点 cookie 输入框 —— 每帧重建会丢 focus / 光标 / 多行 wrap，所以建一次缓存。
    /// 多行由 `TextareaState` 模式本身携带，配合 `.rows(3)` + `.h(px(80.))` 给固定高度。
    pub qidian_cookie_input: &'page Entity<TextareaState>,
    pub pick_folder_listener: &'page PickFolderListener,
}
