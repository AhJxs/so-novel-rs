//! 书架页占位实现：Task 16 用卡片网格整体替换。
//!
//! 存在的意义是让「删除旧条目类型 / 旧 ops」能单独编译通过。占位页只显示页面标题与
//! 下载目录，并保存 `RootView` 注入的打开回调（Task 16 才会真正用到）。

use std::rc::Rc;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{App, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};
use rust_i18n::t;

use crate::desktop::components::PageHeader;
use crate::desktop::model::AppModel;

/// 书架卡片点击 → 打开阅读器。由 `RootView` 注入，避免页面反向持有根视图。
pub type OpenReaderFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// 书架页（占位）。
pub struct LibraryPage {
    model: Entity<AppModel>,
    /// Task 16 之前只存不用；类型走 `mod.rs` 的 re-export，
    /// 保证 `pages::library::OpenReaderFn` 是唯一入口。
    open_reader: Option<super::OpenReaderFn>,
}

impl LibraryPage {
    /// 建页；`RootView` 一次性创建，跨页切换保持状态。
    pub fn new(model: Entity<AppModel>, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            model,
            open_reader: None,
        }
    }

    /// 由 `RootView` 注入打开阅读器的回调。
    pub fn set_open_reader_handler(&mut self, handler: OpenReaderFn) {
        self.open_reader = Some(handler);
    }
}

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let download_path = self.model.read(cx).config.download.download_path.clone();
        // 占位页没有卡片可点；读取一次让字段有读点，Task 16 才接真正的点击。
        let _ = self.open_reader.as_ref();
        div()
            .size_full()
            .p_6()
            .bg(cx.theme().background)
            .child(PageHeader::new(t!("Library.page_title").into()).subtitle(download_path.into()))
    }
}
