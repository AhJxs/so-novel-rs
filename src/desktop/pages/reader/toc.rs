//! 目录抽屉：章节标题过滤 + 虚拟列表，点条目跳章。

use std::rc::Rc;

use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, div, px, uniform_list,
};
use rust_i18n::t;

use super::reader_page::ReaderPage;

impl ReaderPage {
    /// 目录抽屉。没有内容（加载中 / 失败）时给一句占位而不是空面板。
    pub(super) fn render_toc(&self, cx: &Context<Self>) -> AnyElement {
        let Some(titles) = self.toc_titles.clone() else {
            return div()
                .w(px(300.0))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t!("Reader.toc.empty").to_string())
                .into_any_element();
        };

        // 过滤每帧现算：几千条章节 O(n) 扫描比维护一份过滤索引简单，且只在抽屉打开时发生。
        let needle = self.toc_filter.read(cx).value().trim().to_lowercase();
        let indices: Rc<Vec<usize>> = Rc::new(
            (0..titles.len())
                .filter(|index| {
                    needle.is_empty() || titles[*index].to_lowercase().contains(&needle)
                })
                .collect(),
        );
        let filtered = indices.len();
        let total = titles.len();
        let current = self.chapter;
        let handle = cx.entity().downgrade();
        let list_titles = titles;
        let list_indices = indices;

        let list = uniform_list("reader-toc-list", filtered, move |range, _window, cx| {
            let active_bg = cx.theme().selection;
            let hover_bg = cx.theme().list_hover;
            range
                .map(|slot| {
                    let index = list_indices[slot];
                    let title = list_titles[index].clone();
                    let handle = handle.clone();
                    let is_current = index == current;
                    div()
                        .id(("reader-toc-item", index))
                        .w_full()
                        .h(px(32.0))
                        .px_3()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_sm()
                        .when(is_current, |this| {
                            this.font_weight(FontWeight::SEMIBOLD).bg(active_bg)
                        })
                        .hover(move |style| style.bg(hover_bg))
                        .child(div().truncate().child(SharedString::from(title)))
                        .on_click(move |_event, _window, cx| {
                            let _ = handle.update(cx, |page, cx| page.goto_chapter(index, cx));
                        })
                })
                .collect::<Vec<_>>()
        });

        div()
            .w(px(300.0))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Reader.toc.title").to_string()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        t!("Reader.toc.chapter_count", count = &total.to_string())
                                            .to_string(),
                                    ),
                            ),
                    )
                    .child(Input::new(&self.toc_filter).cleanable(true).small()),
            )
            .child(div().flex_1().min_h_0().child(list))
            .into_any_element()
    }
}
