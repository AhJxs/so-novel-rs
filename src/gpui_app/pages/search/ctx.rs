//! Search page 子模块共享的"借用视图" ctx。
//!
//! 子模块不直接拿 `Entity<SearchPage>`（避免连环 update 借出 &mut self），
//! 而是从 ctx 里 `&` 借出自己用得到的字段。
//!
//! 跟 `settings::ctx::PageCtx<'a>` 同模式：每个 page 自建一个，不上 generic
//! （字段集每个 page 都不同，generic 徒增 trait bound）。`render` / `content` 函数
//! 需要的 `&mut App` / `&mut Context<SearchPage>` / `&mut Window` 由调用方直接传，
//! 不放 ctx 里（避免 lifetime 嵌套打架）。

use gpui::Entity;

use crate::app::AppModel;
use crate::models::SearchResult;

use super::source_select::SourceSelectItem;

/// `SearchPage` 字段的借用视图，递给各子模块的 `render(ctx, ...)`。
///
/// 当前子模块（toolbar/result_row/detail_dialog/range_dialog）大多直接收 `&Entity<...>`
/// 形式参数，跟本页 spec 的"统一 ctx 借用视图"略有偏差（ctx 在 PR2–PR4 收敛）。
/// 留着类型避免后续 refactor 再造，统一先 `#[allow(dead_code)]`。
#[allow(dead_code)]
pub(super) struct SearchCtx<'a> {
    pub model: &'a Entity<AppModel>,
    pub keyword: &'a Entity<gpui_component::input::InputState>,
    pub source_state: &'a Entity<
        gpui_component::select::SelectState<
            gpui_component::select::SearchableVec<SourceSelectItem>,
        >,
    >,
    pub list_state: &'a Entity<gpui_component::list::ListState<super::SearchDelegate>>,
    /// 选章 Dialog 的起始 / 结束输入框。
    pub range_start_input: &'a Entity<gpui_component::input::InputState>,
    pub range_end_input: &'a Entity<gpui_component::input::InputState>,
    /// 选章 Dialog 当前服务哪条结果。`None` = 没开 Dialog。
    pub range_target: &'a Option<SearchResult>,
    /// 0-based 当前页码。
    pub current_page: usize,
}
