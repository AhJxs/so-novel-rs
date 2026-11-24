//! 目录抽屉：章节标题过滤 + 虚拟列表，点条目跳章。

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement as _, Styled, div, px, rems, uniform_list,
};
use rust_i18n::t;

use super::reader_page::ReaderPage;

/// 目录抽屉宽度（px）。正文列宽要减掉它，否则抽屉打开时分页与实际折行对不上。
pub(super) const TOC_WIDTH: f32 = 320.0;

/// 章节行高（rem；34px @ 16px 基准）。虚拟列表按它铺行，并随主题字号缩放。
const TOC_ROW_HEIGHT: f32 = 34.0 / 16.0;

/// 目录要展示的章节下标（相对完整标题列表）。
///
/// `lower` 是与标题一一对应的小写副本。它缺失或长度对不上说明过滤数据损坏 / 未就绪，
/// 此时退化为「不过滤」并展示全部章节 —— 目录是导航入口，宁可多显示也不能假装它是空的。
/// `needle` 在本函数内 trim + 小写。
pub(super) fn toc_indices(len: usize, lower: Option<&[String]>, needle: &str) -> Vec<usize> {
    let Some(lower) = lower.filter(|lower| lower.len() == len) else {
        return (0..len).collect();
    };
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return (0..len).collect();
    }
    (0..len)
        .filter(|index| lower[*index].contains(&needle))
        .collect()
}

impl ReaderPage {
    /// 目录抽屉。没有内容（加载中 / 失败）时给一句占位而不是空面板。
    pub(super) fn render_toc(&self, cx: &Context<Self>) -> AnyElement {
        // 标题与小写副本在加载时一起建好（`finish_load`）；这里不再逐帧 `to_lowercase`。
        // 过滤仍然每帧现算：几千条章节 O(n) 扫描比维护一份过滤索引简单，且只在抽屉打开时发生。
        // 小写副本缺失或长度对不上时 `toc_indices` 退化成不过滤，目录不会因此变空。
        let Some(titles) = self.toc_titles.clone() else {
            return div()
                .w(px(TOC_WIDTH))
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

        let lower = self.toc_lower.as_ref().map(|lower| lower.as_slice());
        let indices: Rc<Vec<usize>> = Rc::new(toc_indices(
            titles.len(),
            lower,
            &self.toc_filter.read(cx).value(),
        ));
        let filtered = indices.len();
        let total = titles.len();
        let current = self.chapter;
        let handle = cx.entity().downgrade();
        let active_bg = cx.theme().selection;
        let marker = cx.theme().primary;
        let hover_bg = cx.theme().list_hover;
        let muted = cx.theme().muted_foreground;

        let list = uniform_list("reader-toc-list", filtered, move |range, _window, _cx| {
            range
                .map(|slot| {
                    let index = indices[slot];
                    let title = titles[index].clone();
                    let handle = handle.clone();
                    let is_current = index == current;
                    let row = div()
                        .id(("reader-toc-item", index))
                        .w_full()
                        .h(rems(TOC_ROW_HEIGHT))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_3()
                        .border_l_1()
                        .cursor_pointer()
                        .text_sm()
                        .when(is_current, |this| {
                            this.border_color(marker)
                                .bg(active_bg)
                                .font_weight(FontWeight::SEMIBOLD)
                        })
                        .when(!is_current, |this| this.border_color(active_bg))
                        .hover(move |style| style.bg(hover_bg))
                        // 章号右对齐排在标题左边，目录才有「第几章」的参照。
                        .child(
                            div()
                                .w_10()
                                .flex_none()
                                .text_xs()
                                .text_color(muted)
                                .child(SharedString::from((index + 1).to_string())),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(SharedString::from(title)),
                        );
                    row.on_click(move |_event, _window, cx| {
                        let _ = handle.update(cx, |page, cx| page.goto_chapter(index, cx));
                    })
                })
                .collect::<Vec<_>>()
        })
        // `ListSizingBehavior::Auto` 在 flex 容器里量不到高度，列表会整块空白；
        // 必须给它一个确定的高度，铺满标题栏以下的空间。
        .h_full()
        .w_full()
        .track_scroll(&self.toc_scroll);

        let body: AnyElement = if filtered == 0 {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .text_sm()
                .text_color(muted)
                .child(t!("Reader.toc.no_match").to_string())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(list)
                .into_any_element()
        };

        div()
            .w(px(TOC_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            // 抽屉是应用外壳的一部分，用应用主题；只有正文纸面跟随阅读主题。
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
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
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Reader.toc.title").to_string()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div().text_xs().text_color(muted).child(
                                            t!(
                                                "Reader.toc.chapter_count",
                                                count = &total.to_string()
                                            )
                                            .to_string(),
                                        ),
                                    )
                                    .child(
                                        Button::new("reader-toc-close")
                                            .ghost()
                                            .compact()
                                            .icon(IconName::Close)
                                            .tooltip(t!("Reader.action.close").to_string())
                                            .on_click(cx.listener(|this, _event, window, cx| {
                                                this.close_toc(window, cx);
                                            })),
                                    ),
                            ),
                    )
                    .child(Input::new(&self.toc_filter).cleanable(true).small()),
            )
            .child(body)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::toc_indices;

    fn lower_titles() -> Vec<String> {
        vec!["alpha".to_string(), "beta".to_string(), "gamma".to_string()]
    }

    #[test]
    fn toc_indices_filters_by_trimmed_lowercased_needle() {
        let lower = lower_titles();
        // 空 / 纯空白关键词 → 全部章节。
        assert_eq!(toc_indices(3, Some(&lower), ""), vec![0, 1, 2]);
        assert_eq!(toc_indices(3, Some(&lower), "   "), vec![0, 1, 2]);
        // 大小写折叠 + trim 后只留命中的章节。
        assert_eq!(toc_indices(3, Some(&lower), "ET"), vec![1]);
        assert_eq!(toc_indices(3, Some(&lower), "  BETA  "), vec![1]);
        // 三个标题都含 'a'。
        assert_eq!(toc_indices(3, Some(&lower), "A"), vec![0, 1, 2]);
    }

    #[test]
    fn toc_indices_degrades_to_no_filter_when_lower_is_missing_or_mismatched() {
        // 小写副本缺失、或长度与标题对不上 → 不过滤，展示全部。
        assert_eq!(toc_indices(3, None, "beta"), vec![0, 1, 2]);
        let short = vec!["alpha".to_string(), "beta".to_string()];
        assert_eq!(toc_indices(3, Some(&short), "beta"), vec![0, 1, 2]);
        // 空目录无论 Filter 数据如何都返回空。
        assert_eq!(toc_indices(0, None, "beta"), Vec::<usize>::new());
    }
}
