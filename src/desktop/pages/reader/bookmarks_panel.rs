//! 书签抽屉：添加当前书签、按章节升序展示、点击跳转、删除。

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IconName};
use gpui_kit::{
    AnyElement, App, Context, FontWeight, InteractiveElement as _, IntoElement, ParentElement,
    StatefulInteractiveElement as _, Styled, div, px,
};
use rust_i18n::t;

use super::reader_page::ReaderPage;
use crate::library::Bookmark;
use crate::utils::formatting::{format_local_unix_secs, truncate};

/// 书签抽屉宽度（px）。正文列宽要减掉它，与 `toc::TOC_WIDTH` / `search_panel::SEARCH_WIDTH` 同理。
pub(super) const BOOKMARKS_WIDTH: f32 = 300.0;

impl ReaderPage {
    /// 书签列表快照：store 是唯一真相，`self.book` 只是打开时的副本。
    ///
    /// 走 `bookmarks_of` 而不是 `book_snapshot`：抽屉每帧都要读，别为此整本 clone。
    pub(super) fn bookmarks_snapshot(&self, cx: &App) -> Vec<Bookmark> {
        self.model
            .read(cx)
            .bookmarks_of(&self.book_id)
            .unwrap_or_default()
    }

    /// 把当前位置加成书签；同章同位置重复时提示「已存在」。
    ///
    /// 滚动模式下 `chapter_ratio` 只由 `on_scroll` 与 5 s 一次的 autosave 更新，
    /// 而**拖动滚动条不触发 `on_scroll`**：不先按当前偏移校准，书签会落在拖之前的位置。
    pub(super) fn add_bookmark(&mut self, cx: &mut Context<Self>) {
        if !self.is_ready() {
            return;
        }
        self.capture_scroll_ratio();
        self.record_progress(cx);
        let excerpt = self.current_excerpt();
        // 摘录为空说明当前位置落不到正文（章列表还没就绪 / 空章）：不记一枚空白书签。
        if excerpt.is_empty() {
            return;
        }
        let bookmark = Bookmark::new(self.chapter, self.chapter_ratio.clamp(0.0, 1.0), excerpt);
        let book_id = self.book_id.clone();
        let exists = self.model.read(cx).bookmarks_of(&book_id).is_some();
        let added = self.model.update(cx, |model, _cx| {
            model.add_reading_bookmark(&book_id, bookmark)
        });
        if !added {
            self.model.update(cx, |model, _cx| {
                if exists {
                    model.push_warning(t!("Reader.bookmarks.duplicate").to_string());
                } else {
                    model.push_error(t!("Library.error.book_not_found").to_string());
                }
            });
        }
        cx.notify();
    }

    /// 当前位置附近一段的前 40 字，作为书签摘录。
    fn current_excerpt(&self) -> String {
        let Some(chapter) = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(self.chapter))
        else {
            return String::new();
        };
        if chapter.paragraphs.is_empty() {
            return String::new();
        }
        let index =
            crate::reader::layout::paragraph_at_ratio(&chapter.paragraphs, self.chapter_ratio);
        truncate(&chapter.paragraphs[index], 40)
    }

    /// 跳到 `(章, 章内比例)`。书签跳转用它；搜索 / 目录各自有更精确的入口。
    pub(super) fn goto_position(&mut self, chapter: usize, ratio: f32, cx: &mut Context<Self>) {
        let Some(count) = self.content.as_ref().map(|content| content.chapters.len()) else {
            return;
        };
        if count == 0 {
            return;
        }
        // 书签可能在当前章：指纹不变时分页缓存会直接早退、`page_index` 不追随新比例。
        self.invalidate_scroll_commit();
        self.page_cache = None;
        // 显式跳转必须清掉跨章回退的残留标志，否则分页会落到目标章末页。
        self.page_turn_to_last = false;
        self.chapter = chapter.min(count - 1);
        self.chapter_ratio = ratio.clamp(0.0, 1.0);
        self.restore_pending = true;
        self.record_progress(cx);
        cx.notify();
    }

    /// 右侧书签抽屉。
    pub(super) fn render_bookmarks(&self, cx: &Context<Self>) -> AnyElement {
        let bookmarks = self.bookmarks_snapshot(cx);
        let titles = self.toc_titles.clone();
        let mut list = div()
            .id("reader-bookmarks")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.bookmarks_scroll)
            .vertical_scrollbar(&self.bookmarks_scroll)
            .p_2()
            .flex()
            .flex_col()
            .gap_1();
        if bookmarks.is_empty() {
            list = list.child(
                div()
                    .p_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("Reader.bookmarks.empty").to_string()),
            );
        }

        for bookmark in &bookmarks {
            let chapter = bookmark.chapter;
            let ratio = bookmark.chapter_ratio;
            let bookmark_id = bookmark.id.clone();
            let title = titles
                .as_ref()
                .and_then(|titles| titles.get(chapter))
                .cloned()
                .unwrap_or_else(|| t!("Reader.bookmarks.chapter", index = chapter + 1).to_string());
            let time = format_local_unix_secs(
                bookmark.created_unix,
                "Reader.bookmarks.unknown_time",
                "Reader.bookmarks.invalid_time",
                "Reader.bookmarks.time_failed",
            );
            let excerpt = bookmark.excerpt.clone();
            let hover_bg = cx.theme().list_hover;
            let delete_id = bookmark_id.clone();
            list = list.child(
                div()
                    .id(format!("reader-bookmark-{bookmark_id}"))
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_bg))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.goto_position(chapter, ratio, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                Button::new(format!("reader-bookmark-delete-{delete_id}"))
                                    .ghost()
                                    .compact()
                                    .icon(IconName::Delete)
                                    .tooltip(t!("Reader.bookmarks.delete").to_string())
                                    .on_click(cx.listener(move |this, _event, _window, cx| {
                                        // 删完不要顺带触发整行的「跳转」。
                                        cx.stop_propagation();
                                        let book_id = this.book_id.clone();
                                        let id = delete_id.clone();
                                        this.model.update(cx, |model, _cx| {
                                            model.remove_reading_bookmark(&book_id, &id);
                                        });
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(excerpt),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(time),
                    ),
            );
        }

        div()
            .w(px(BOOKMARKS_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            // 抽屉属于应用外壳：底色 / 前景跟应用主题。
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .border_l_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .p_3()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Reader.bookmarks.title").to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new("reader-bookmark-add")
                                    .primary()
                                    .compact()
                                    .icon(IconName::Plus)
                                    .tooltip(t!("Reader.bookmarks.add").to_string())
                                    .disabled(self.content.is_none())
                                    .on_click(cx.listener(|this, _event, _window, cx| {
                                        this.add_bookmark(cx);
                                    })),
                            )
                            .child(
                                Button::new("reader-bookmarks-close")
                                    .ghost()
                                    .compact()
                                    .icon(IconName::Close)
                                    .tooltip(t!("Reader.action.close").to_string())
                                    .on_click(cx.listener(|this, _event, window, cx| {
                                        this.set_bookmarks_open(false, window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(list)
            .into_any_element()
    }
}
