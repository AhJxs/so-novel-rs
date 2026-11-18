//! 阅读器页面：加载状态机 + 顶栏 + 正文骨架。
//!
//! 滚动 / 翻页的完整实现分别在 Task 18 / 19 里替换 `render_chapter`；
//! 本任务先把「从书架打开 → 加载 → 显示正文 → 关闭落盘」的闭环跑通。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::{
    App, Context, Entity, FocusHandle, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement as _, Styled, Window, div,
};
use rust_i18n::t;

use super::reader_colors;
use crate::config::{ReaderTheme, ReadingMode};
use crate::desktop::model::AppModel;
use crate::library::{Book, ReadingProgress};
use crate::reader::{ReaderBook, ReaderError, load_book};

/// 关闭阅读器：由 `RootView` 注入（需要同时改 `RootView` 状态与全屏标记）。
pub type CloseReaderFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// 打开状态。
pub enum LoadState {
    Loading,
    Ready,
    Error(ReaderError),
}

/// 阅读器页面。
pub struct ReaderPage {
    model: Entity<AppModel>,
    book_id: String,
    book: Book,
    content: Option<ReaderBook>,
    load_state: LoadState,
    mode: ReadingMode,
    chapter: usize,
    chapter_ratio: f32,
    immersive: bool,
    /// 加载代数：结果回来时与当时的代数比对，丢弃过期结果。重开一本新书是换实体，
    /// 所以今天只在 `new()` 里置 1；将来若在同一实体里「重新加载」，必须在这里 +1。
    generation: u64,
    /// 进度有未落盘的改动；Task 18 的滚动提交与关闭路径读它。
    progress_dirty: bool,
    focus: FocusHandle,
    close_handler: Option<CloseReaderFn>,
}

impl ReaderPage {
    /// 打开一本书并启动后台加载。
    pub fn new(
        model: Entity<AppModel>,
        book: Book,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let page = Self {
            model,
            book_id: book.id.clone(),
            mode: book.progress.mode,
            chapter: book.progress.chapter,
            chapter_ratio: book.progress.chapter_ratio,
            book,
            content: None,
            load_state: LoadState::Loading,
            immersive: false,
            generation: 1,
            progress_dirty: false,
            focus,
            close_handler: None,
        };
        page.spawn_load(cx);
        page
    }

    /// 注入关闭回调（`RootView` 调用）。
    pub fn set_close_handler(&mut self, handler: CloseReaderFn) {
        self.close_handler = Some(handler);
    }

    /// 是否处于沉浸模式（`RootView` 据此隐藏 `TitleBar`）。
    #[must_use]
    pub const fn is_immersive(&self) -> bool {
        self.immersive
    }

    /// 落盘进度与阅读设置；随后由 `RootView` 丢弃本实体。
    pub fn close_and_flush(&mut self, cx: &mut Context<Self>) {
        self.record_progress(cx);
        self.progress_dirty = false;
        self.model.update(cx, |model, _| model.flush_library());
    }

    fn spawn_load(&self, cx: &Context<Self>) {
        let generation = self.generation;
        let path = PathBuf::from(&self.book.path);
        let format = self.book.format;
        let encoding = self.book.encoding.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { load_book(&path, format, encoding.as_deref()) })
                .await;
            let _ = this.update(cx, |page, cx| page.finish_load(generation, result, cx));
        })
        .detach();
    }

    fn finish_load(
        &mut self,
        generation: u64,
        result: Result<ReaderBook, ReaderError>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.generation {
            return;
        }
        match result {
            Ok(content) => {
                let chapter_count = content.chapters.len().max(1);
                self.chapter = self.chapter.min(chapter_count - 1);
                self.chapter_ratio = self.chapter_ratio.clamp(0.0, 1.0);
                let encoding = content.encoding.clone();
                self.content = Some(content);
                self.load_state = LoadState::Ready;
                let book_id = self.book_id.clone();
                self.model.update(cx, |model, _| {
                    model.mark_book_opened(&book_id, chapter_count as u32, encoding);
                });
                self.record_progress(cx);
            }
            Err(error) => self.load_state = LoadState::Error(error),
        }
        cx.notify();
    }

    /// 当前章节快照（章节下标已夹在合法范围）。
    fn current_chapter(&self) -> Option<&crate::reader::ReaderChapter> {
        let content = self.content.as_ref()?;
        content.chapters.get(self.chapter)
    }

    fn record_progress(&mut self, cx: &mut Context<Self>) {
        // `percent` 与 `updated_unix` 是占位：`LibraryStore::record_progress` 会用
        // 章节数重算百分比并写入当前时间。
        let progress = ReadingProgress {
            chapter: self.chapter,
            chapter_ratio: self.chapter_ratio,
            percent: 0.0,
            mode: self.mode,
            updated_unix: 0,
            finished: self.book.progress.finished,
        };
        let book_id = self.book_id.clone();
        self.model.update(cx, |model, _| {
            model.record_reading_progress(&book_id, progress);
        });
        self.progress_dirty = true;
    }

    fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        self.mode = match self.mode {
            ReadingMode::Scroll => ReadingMode::Page,
            ReadingMode::Page => ReadingMode::Scroll,
        };
        self.book.progress.mode = self.mode;
        let book_id = self.book_id.clone();
        let mode = self.mode;
        self.model
            .update(cx, |model, _| model.set_reading_mode(&book_id, mode));
        cx.notify();
    }

    fn goto_chapter(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(content) = self.content.as_ref() else {
            return;
        };
        let count = content.chapters.len();
        let next = self.chapter.saturating_add_signed(delta).min(count - 1);
        if next != self.chapter {
            self.chapter = next;
            self.chapter_ratio = 0.0;
            self.record_progress(cx);
            cx.notify();
        }
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let close_handler = self.close_handler.clone();
        let mode_label = match self.mode {
            ReadingMode::Scroll => t!("Reader.mode.page"),
            ReadingMode::Page => t!("Reader.mode.scroll"),
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("reader-back")
                    .label(t!("Reader.action.back"))
                    .on_click(move |_event, window, cx| {
                        if let Some(handler) = &close_handler {
                            handler(window, cx);
                        }
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .text_center()
                    .font_weight(FontWeight::MEDIUM)
                    .child(SharedString::from(self.book.title.clone())),
            )
            .child(
                Button::new("reader-mode")
                    .label(mode_label)
                    .on_click(cx.listener(|this, _event, _window, cx| this.toggle_mode(cx))),
            )
    }

    fn render_error(error: &ReaderError) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .p_8()
            .child(div().text_lg().child(t!(error.message_key()).to_string()))
            .child(div().text_sm().child(error.detail()))
    }

    fn render_chapter(&self, cx: &Context<Self>) -> impl IntoElement {
        let Some(content) = self.content.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let colors = reader_colors(self.book_theme(cx), cx);
        let Some(chapter) = self.current_chapter() else {
            return div().flex_1().into_any_element();
        };
        let title = chapter.title.clone();
        let paragraphs = chapter.paragraphs.clone();
        let count = content.chapters.len();
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(
                div()
                    .id("reader-chapter-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_6()
                    .child(
                        // 居中靠 flex 而不是 `mx_auto`：gpui 没有 auto margin 样式。
                        div().w_full().flex().flex_row().justify_center().child(
                            div()
                                .w_full()
                                .max_w(gpui_kit::px(720.0))
                                .flex()
                                .flex_col()
                                .gap_3()
                                .child(div().text_xl().font_weight(FontWeight::BOLD).child(title))
                                .children(paragraphs.into_iter().map(|line| div().child(line))),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .px_4()
                    .py_2()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("reader-prev-chapter")
                            .label(t!("Reader.action.prev_chapter"))
                            .disabled(self.chapter == 0)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.goto_chapter(-1, cx);
                            })),
                    )
                    .child(div().text_sm().child(t!(
                        "Reader.chapter_indicator",
                        current = self.chapter + 1,
                        total = count
                    )))
                    .child(
                        Button::new("reader-next-chapter")
                            .label(t!("Reader.action.next_chapter"))
                            .disabled(self.chapter + 1 >= count)
                            .on_click(cx.listener(|this, _event, _window, cx| {
                                this.goto_chapter(1, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn book_theme(&self, cx: &Context<Self>) -> ReaderTheme {
        self.model.read(cx).config.reader.theme
    }
}

impl Render for ReaderPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body: gpui_kit::AnyElement = match &self.load_state {
            LoadState::Loading => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(t!("Reader.loading").to_string())
                .into_any_element(),
            LoadState::Error(error) => Self::render_error(error).into_any_element(),
            LoadState::Ready => self.render_chapter(cx).into_any_element(),
        };
        div()
            .track_focus(&self.focus)
            .key_context("Reader")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_toolbar(cx))
            .child(body)
    }
}
