//! 阅读器页面：加载状态机 + 滚动 / 翻页两种正文 + 目录抽屉 / 排版面板。
//!
//! 两种模式共用同一份位置（`chapter` + `chapter_ratio`）：滚动模式从滚动偏移采样，
//! 翻页模式由分页表换算，切换模式时按比例恢复。

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable as _, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, Render, ScrollEvent,
    ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled, Window, div, point, px,
};
use rust_i18n::t;

use super::content_view::{self, COLUMN_PADDING};
use super::reader_colors;
use super::search_panel::{self, SearchPanel};
use crate::config::{ReaderConfig, ReadingMode};
use crate::desktop::model::AppModel;
use crate::library::{Book, ReadingProgress};
use crate::reader::layout::{page_for_ratio, ratio_for_page};
use crate::reader::{
    LayoutFingerprint, ParagraphMetrics, ReaderBook, ReaderError, ReaderLayout, load_book, paginate,
};

/// 关闭阅读器：由 `RootView` 注入（需要同时改 `RootView` 状态与全屏标记）。
pub type CloseReaderFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// gpui 的滚动偏移向下为负（`set_offset`：越往下滚偏移越负），而进度要的是
/// 0..=1 的正向比例。全部换算都走这两个函数，别在调用点各写一份。
/// 偏移 → 章内比例；内容不足一屏（滚不动）时返回 `None`。
fn scroll_ratio(offset_y: f32, max_y: f32) -> Option<f32> {
    (max_y > 1.0).then(|| (-offset_y / max_y).clamp(0.0, 1.0))
}

/// 章内比例 → 滚动偏移（负值）。
fn ratio_offset(ratio: f32, max_y: f32) -> f32 {
    -(max_y.max(0.0) * ratio.clamp(0.0, 1.0))
}

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
    /// 打开时的书籍快照；关闭时用于落盘。
    pub(super) book: Book,
    pub(super) content: Option<ReaderBook>,
    pub(super) load_state: LoadState,
    pub(super) mode: ReadingMode,
    pub(super) chapter: usize,
    pub(super) chapter_ratio: f32,
    /// 已到达书末并置位；一旦为 true 不再回落。
    finished: bool,
    /// 滚动模式的滚动位置。
    pub(super) scroll: ScrollHandle,
    /// 待恢复的位置：`max_offset` 要等布局完成，所以恢复放到下一帧。
    pub(super) restore_pending: bool,
    /// 每次滚动自增，只有最新一代的防抖任务会写进度。
    scroll_generation: u64,
    /// 翻页模式：当前页号。
    page_index: usize,
    /// 翻页模式：当前章的分页缓存。
    pub(super) page_cache: Option<PageCache>,
    /// 越界向前翻页时置位：重算分页后落到新章最后一页。
    page_turn_to_last: bool,
    /// 正文视口实测高度（px）；`None` = 还没量到，先按窗口高度估。
    viewport_height: Option<f32>,
    /// 与 `viewport_probe` 共享的最近一次实测值，用来判断要不要通知。
    viewport_measured: Rc<Cell<Option<f32>>>,
    /// 测量异常提示过就不再刷屏。
    layout_warned: bool,
    /// 章节标题快照；加载完成时建一次，抽屉每帧零拷贝共享。
    pub(super) toc_titles: Option<Rc<Vec<String>>>,
    /// 目录顶部的章节过滤框。
    pub(super) toc_filter: Entity<InputState>,
    /// 搜索抽屉状态。
    pub(super) search: SearchPanel,
    pub(super) search_input: Entity<InputState>,
    pub(super) search_open: bool,
    /// 在跑的扫描任务的取消旗标。
    pub(super) search_cancel: Option<Arc<AtomicBool>>,
    pub(super) toc_open: bool,
    pub(super) typography_open: bool,
    immersive: bool,
    generation: u64,
    progress_dirty: bool,
    focus: FocusHandle,
    close_handler: Option<CloseReaderFn>,
}

/// 一章的分页缓存：指纹变了才整章重算。
///
/// `offset` 是每页的起始纵向偏移（px，首项恒为 0），由 `paginate` 产出。
///
/// `pub(super)` 是为了让同级的 `search_panel` 能在跳转命中时作废缓存，字段仍只在本模块读写。
pub(super) struct PageCache {
    fingerprint: LayoutFingerprint,
    /// 分页与渲染共用的行高（已按设备像素取整）。
    line_height_px: f32,
    offset: Vec<f32>,
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
        let toc_filter = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("Reader.toc.filter_placeholder").to_string())
        });
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder(t!("Reader.search.placeholder").to_string())
        });
        let finished = book.progress.finished;
        let page = Self {
            model,
            book_id: book.id.clone(),
            mode: book.progress.mode,
            chapter: book.progress.chapter,
            chapter_ratio: book.progress.chapter_ratio,
            book,
            content: None,
            load_state: LoadState::Loading,
            finished,
            scroll: ScrollHandle::new(),
            restore_pending: true,
            scroll_generation: 0,
            page_index: 0,
            page_cache: None,
            page_turn_to_last: false,
            viewport_height: None,
            viewport_measured: Rc::new(Cell::new(None)),
            layout_warned: false,
            toc_titles: None,
            toc_filter,
            search: SearchPanel::new(),
            search_input,
            search_open: false,
            search_cancel: None,
            toc_open: false,
            typography_open: false,
            immersive: false,
            generation: 1,
            progress_dirty: false,
            focus,
            close_handler: None,
        };
        // 过滤框内容变化要让抽屉重画；不清空的话列表会停在旧结果上。
        cx.subscribe(&page.toc_filter, |_this, _input, event, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        // 回车才开始扫描：边输入边搜会让整书扫描反复重启。
        cx.subscribe(&page.search_input, |this, _input, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.start_search(cx);
            }
        })
        .detach();
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
        self.cancel_search();
        // 滚动防抖可能还没触发：关窗前先按当前滚动位置收一次比例，避免丢掉最后一屏。
        self.capture_scroll_ratio();
        self.record_progress(cx);
        self.progress_dirty = false;
        self.persist_settings(cx);
        self.model.update(cx, |model, _| model.flush_library());
    }

    /// 当前生效的排版参数（已 clamp）。
    pub(super) fn reader_config(&self, cx: &App) -> ReaderConfig {
        self.model.read(cx).config.reader.clamped()
    }

    /// 改一项排版参数：写回配置、保持当前阅读比例，并立刻重画。
    ///
    /// 不在这里落盘 —— 面板关闭 / 关闭阅读器时才写 config.toml，拖动步进不会刷盘。
    pub(super) fn update_reader_config(
        &mut self,
        update: impl FnOnce(&mut ReaderConfig),
        cx: &mut Context<Self>,
    ) {
        // 字号一变内容高度就变，先把「读到哪」按比例记下来，下一帧再按比例恢复。
        self.capture_scroll_ratio();
        self.restore_pending = true;
        self.model.update(cx, |model, _cx| {
            let mut reader = model.config.reader.clamped();
            update(&mut reader);
            model.config.reader = reader.clamped();
        });
        cx.notify();
    }

    fn persist_settings(&self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, _| model.persist_settings());
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
                self.toc_titles = Some(Rc::new(
                    content
                        .chapters
                        .iter()
                        .map(|chapter| chapter.title.clone())
                        .collect(),
                ));
                self.content = Some(content);
                self.load_state = LoadState::Ready;
                self.invalidate_scroll_commit();
                self.restore_pending = true;
                let book_id = self.book_id.clone();
                self.model.update(cx, |model, _| {
                    model.mark_book_opened(&book_id, chapter_count as u32, encoding);
                });
                self.record_progress(cx);
                Self::spawn_autosave(cx);
            }
            Err(error) => self.load_state = LoadState::Error(error),
        }
        cx.notify();
    }

    /// 每 5 s 把脏掉的进度落盘一次；实体被丢弃时循环自然结束。
    fn spawn_autosave(cx: &Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                let alive = this.update(cx, Self::autosave).is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn autosave(&mut self, cx: &mut Context<Self>) {
        // 滚动条拖动不上报 `on_scroll`：按固定节奏补采一次位置，位置变了才算一次进度。
        let previous = self.chapter_ratio;
        self.capture_scroll_ratio();
        // 按位比较而不是 `!=`：这里只关心「位置有没有变」，且 `float_cmp` 是启用的。
        if self.chapter_ratio.to_bits() != previous.to_bits() {
            self.record_progress(cx);
            cx.notify();
        }
        if !self.progress_dirty {
            return;
        }
        self.progress_dirty = false;
        // 只落书架：`flush_library` 会连 config.toml 一起写，每 5 s 刷一次配置没有意义。
        self.model.update(cx, |model, _| model.persist_library());
    }

    /// 把当前滚动位置换算成章内比例（滚不动时保持原值）。
    fn capture_scroll_ratio(&mut self) {
        self.invalidate_scroll_commit();
        // 跳章 / 切模式 / 改排版之后、恢复帧跑之前，滚动句柄里还是**旧**位置；
        // 此刻采样会把刚设好的目标比例覆盖掉，等恢复帧落地再说。
        if self.restore_pending {
            return;
        }
        // 翻页模式没有滚动容器：句柄里留着切模式前那次布局的偏移与上限，
        // 采样会把 `goto_page` 写下的页内比例改回旧位置（autosave 每 5 s 采一次）。
        if matches!(self.mode, ReadingMode::Page) {
            return;
        }
        let max = f32::from(self.scroll.max_offset().y);
        if let Some(ratio) = scroll_ratio(f32::from(self.scroll.offset().y), max) {
            self.chapter_ratio = ratio;
        }
    }

    /// 作废在途的滚动防抖提交（位置已被别的路径改写）。
    pub(super) const fn invalidate_scroll_commit(&mut self) {
        self.scroll_generation = self.scroll_generation.wrapping_add(1);
    }

    pub(super) fn record_progress(&mut self, cx: &mut Context<Self>) {
        let chapter_count = self
            .content
            .as_ref()
            .map_or(0, |content| content.chapters.len());
        // 滚动到最后一章的最后 1%（或翻页到最后一页）即视为读完。
        let at_end =
            chapter_count > 0 && self.chapter + 1 >= chapter_count && self.chapter_ratio >= 0.99;
        self.finished |= at_end;
        let progress = ReadingProgress {
            chapter: self.chapter,
            chapter_ratio: self.chapter_ratio,
            // 百分比与时间戳由 store 统一计算，这里不做第二份实现。
            percent: 0.0,
            mode: self.mode,
            updated_unix: 0,
            finished: self.finished,
        };
        let book_id = self.book_id.clone();
        self.model.update(cx, |model, _| {
            model.record_reading_progress(&book_id, progress);
        });
        self.progress_dirty = true;
    }

    fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        self.capture_scroll_ratio();
        self.mode = match self.mode {
            ReadingMode::Scroll => ReadingMode::Page,
            ReadingMode::Page => ReadingMode::Scroll,
        };
        // 切到翻页模式时丢掉旧分页：页号要按当前 `chapter_ratio` 重新归位，
        // 否则会沿用上次翻页留下的 `page_index`（滚动模式读到的位置被无视，
        // 页脚百分比与页号自相矛盾）。
        if matches!(self.mode, ReadingMode::Page) {
            self.page_cache = None;
        }
        self.restore_pending = true;
        let book_id = self.book_id.clone();
        let mode = self.mode;
        self.model
            .update(cx, |model, _| model.set_reading_mode(&book_id, mode));
        self.record_progress(cx);
        cx.notify();
    }

    /// 跳到第 `index` 章（0-based），章内位置归零。
    pub(super) fn goto_chapter(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(content) = self.content.as_ref() else {
            return;
        };
        if index >= content.chapters.len() {
            return;
        }
        self.chapter = index;
        self.invalidate_scroll_commit();
        self.chapter_ratio = 0.0;
        self.restore_pending = true;
        self.record_progress(cx);
        cx.notify();
    }

    fn step_chapter(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(content) = self.content.as_ref() else {
            return;
        };
        let count = content.chapters.len();
        if count == 0 {
            return;
        }
        let next = self.chapter.saturating_add_signed(delta).min(count - 1);
        if next != self.chapter {
            self.goto_chapter(next, cx);
        }
    }

    /// 按屏幕高度滚动（`direction` 为 -1 / 1）。
    pub(super) fn scroll_by_page(&self, direction: f32) {
        let viewport = f32::from(self.scroll.bounds().size.height);
        self.scroll_relative(direction * (viewport * 0.9).max(1.0));
    }

    /// 小步滚动（方向键）。
    fn scroll_by_step(&self, direction: f32) {
        self.scroll_relative(direction * 40.0);
    }

    /// 滚动到章内某个比例处。
    fn scroll_to_ratio(&self, ratio: f32) {
        let max = f32::from(self.scroll.max_offset().y);
        self.scroll
            .set_offset(point(px(0.0), px(ratio_offset(ratio, max))));
    }

    /// 在当前位置上偏移（正数为向下）；程序化滚动不会触发 `on_scroll`，
    /// 调用方负责补重绘与进度提交。
    pub(super) fn scroll_relative(&self, delta: f32) {
        let max = f32::from(self.scroll.max_offset().y).max(0.0);
        let current = f32::from(self.scroll.offset().y);
        // 偏移越往下越负，所以「向下 delta」= 偏移减去 delta。
        let target = (current - delta).clamp(-max, 0.0);
        self.scroll.set_offset(point(px(0.0), px(target)));
    }

    /// 滚动防抖：200 ms 内不再滚动才写进度，避免每一帧都更新 store。
    pub(super) fn schedule_progress_commit(&mut self, cx: &Context<Self>) {
        self.scroll_generation = self.scroll_generation.wrapping_add(1);
        let generation = self.scroll_generation;
        let handle = self.scroll.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let _ = this.update(cx, |page, cx| {
                page.commit_scroll_progress(generation, &handle, cx);
            });
        })
        .detach();
    }

    fn commit_scroll_progress(
        &mut self,
        generation: u64,
        handle: &ScrollHandle,
        cx: &mut Context<Self>,
    ) {
        if generation != self.scroll_generation {
            return;
        }
        let max = f32::from(handle.max_offset().y);
        if let Some(ratio) = scroll_ratio(f32::from(handle.offset().y), max) {
            self.chapter_ratio = ratio;
        }
        self.record_progress(cx);
        cx.notify();
    }

    /// 关目录抽屉；过滤框还聚焦时把焦点收回正文，
    /// 否则抽屉一卸，`shortcuts_blocked` 恒为真。
    fn close_toc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toc_open = false;
        if self.toc_filter.read(cx).focus_handle(cx).is_focused(window) {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// 把焦点收回正文。抽屉一卸，输入框仍持着焦点，`shortcuts_blocked` 就会一直拦着快捷键。
    pub(super) fn focus_body(&self, window: &mut Window, cx: &mut App) {
        window.focus(&self.focus, cx);
    }

    /// 开 / 关搜索抽屉：开时焦点进输入框，关时停扫描并把焦点收回正文。
    ///
    /// 开合都收在这一个入口：抽屉卸载后若焦点还留在输入框上，除 Esc 外的快捷键全部失灵。
    pub(super) fn set_search_open(
        &mut self,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_open = open;
        if open {
            let handle = self.search_input.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        } else {
            self.cancel_search();
            self.focus_body(window, cx);
        }
    }

    fn toggle_toc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toc_open {
            self.close_toc(window, cx);
        } else {
            // 两个右侧抽屉不共存：开目录就关搜索，正文列宽只按一个抽屉算。
            if self.search_open {
                self.set_search_open(false, window, cx);
            }
            self.toc_open = true;
            self.typography_open = false;
            cx.notify();
        }
    }

    fn shortcuts_blocked(&self, window: &Window, cx: &App) -> bool {
        // 抽屉里的输入框聚焦时按键归输入框；只有 Esc 例外（在 on_key_down 里处理）。
        self.toc_filter.read(cx).focus_handle(cx).is_focused(window)
            || self
                .search_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if self.shortcuts_blocked(window, cx) {
            if key == "escape" {
                self.toc_open = false;
                self.set_search_open(false, window, cx);
                cx.notify();
            }
            return;
        }

        match event.keystroke.key_char.as_deref() {
            Some("+") => {
                self.adjust_font_size(1.0, cx);
                return;
            }
            Some("-") => {
                self.adjust_font_size(-1.0, cx);
                return;
            }
            _ => {}
        }

        let paging = matches!(self.mode, ReadingMode::Page);
        match key {
            "left" | "pageup" | "up" if paging => self.turn_page(-1, cx),
            "right" | "pagedown" | "space" | "down" if paging => self.turn_page(1, cx),
            "home" if paging => self.goto_page(0, cx),
            "end" if paging => {
                let pages = self
                    .page_cache
                    .as_ref()
                    .map_or(0, |cache| cache.offset.len());
                self.goto_page(pages.saturating_sub(1), cx);
            }
            "left" | "pageup" => self.scroll_by_page(-1.0),
            "right" | "pagedown" | "space" => self.scroll_by_page(1.0),
            "up" => self.scroll_by_step(-1.0),
            "down" => self.scroll_by_step(1.0),
            "home" => self.scroll_to_ratio(0.0),
            "end" => self.scroll_to_ratio(1.0),
            // 不要退回 Task 18 之前的写法：关目录必须走 `toggle_toc`（它把焦点从过滤框
            // 收回正文），Esc 关闭必须走 `window.defer`（`cx.listener` 正租着本实体）。
            "t" => self.toggle_toc(window, cx),
            "s" => {
                self.toc_open = false;
                self.typography_open = false;
                let open = !self.search_open;
                self.set_search_open(open, window, cx);
                cx.notify();
            }
            "m" => self.toggle_mode(cx),
            "escape" => {
                if self.typography_open {
                    self.typography_open = false;
                    self.persist_settings(cx);
                    cx.notify();
                } else if self.search_open {
                    self.set_search_open(false, window, cx);
                    cx.notify();
                } else if self.toc_open {
                    self.toc_open = false;
                    cx.notify();
                } else if let Some(handler) = self.close_handler.clone() {
                    window.defer(cx, move |window, cx| handler(window, cx));
                }
            }
            _ => return,
        }

        // 滚动模式的按键只改了滚动位置，进度要在滚动之后补记；
        // 翻页模式的 turn_page / goto_page 已经自己记过进度了。
        if !paging
            && matches!(
                key,
                "left" | "right" | "pageup" | "pagedown" | "space" | "up" | "down" | "home" | "end"
            )
        {
            // `set_offset` 不会自己重绘（Task 18 修的 I3），翻页模式则由 `goto_page` 的
            // `cx.notify()` 负责，所以只在滚动模式下补这一下。
            cx.notify();
            self.schedule_progress_commit(cx);
        }
    }

    fn adjust_font_size(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.update_reader_config(move |reader| reader.font_size += delta, cx);
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let close_handler = self.close_handler.clone();
        let mode_label = match self.mode {
            ReadingMode::Scroll => t!("Reader.mode.page"),
            ReadingMode::Page => t!("Reader.mode.scroll"),
        };

        let mut toc_button = Button::new("reader-toc").label(t!("Reader.action.toc"));
        toc_button = if self.toc_open {
            toc_button.primary()
        } else {
            toc_button.ghost()
        };
        let mut typography_button = Button::new("reader-typography").label("Aa");
        typography_button = if self.typography_open {
            typography_button.primary()
        } else {
            typography_button.ghost()
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
            .child(toc_button.on_click(cx.listener(|this, _event, window, cx| {
                this.toggle_toc(window, cx);
            })))
            .child({
                let mut search_button =
                    Button::new("reader-search").label(t!("Reader.action.search"));
                search_button = if self.search_open {
                    search_button.primary()
                } else {
                    search_button.ghost()
                };
                search_button.on_click(cx.listener(|this, _event, window, cx| {
                    this.toc_open = false;
                    this.typography_open = false;
                    let open = !this.search_open;
                    this.set_search_open(open, window, cx);
                    cx.notify();
                }))
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_center()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(SharedString::from(self.book.title.clone())),
            )
            .child(
                typography_button.on_click(cx.listener(|this, _event, window, cx| {
                    this.typography_open = !this.typography_open;
                    if this.toc_open {
                        // 走 `close_toc`：过滤框聚焦时要先把焦点收回正文，
                        // 否则抽屉一卸，除 Esc 外的快捷键全部失灵。
                        this.close_toc(window, cx);
                    }
                    if !this.typography_open {
                        this.persist_settings(cx);
                    }
                    cx.notify();
                })),
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

    /// 全书百分比：(章节下标 + 章内比例) / 章节数。
    ///
    /// 全书的章节数、章节下标都远小于 2^32；经 u32 转 f64 精确，且不触发
    /// `cast_precision_loss`（该 lint 没有全局放行）。
    fn book_percent(&self, count: usize) -> f64 {
        if count == 0 {
            return 0.0;
        }
        let chapter = f64::from(u32::try_from(self.chapter).unwrap_or(u32::MAX));
        let total = f64::from(u32::try_from(count).unwrap_or(u32::MAX));
        let ratio = f64::from(self.chapter_ratio.clamp(0.0, 1.0));
        ((chapter + ratio) / total * 100.0).clamp(0.0, 100.0)
    }

    fn render_chapter_nav(&self, count: usize, cx: &Context<Self>) -> impl IntoElement {
        let percent = self.book_percent(count);
        div()
            .flex()
            .flex_row()
            .items_center()
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
                        this.step_chapter(-1, cx);
                    })),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} · {percent:.0}%",
                        t!(
                            "Reader.chapter_indicator",
                            current = self.chapter + 1,
                            total = count
                        )
                    )),
            )
            .child(
                Button::new("reader-next-chapter")
                    .label(t!("Reader.action.next_chapter"))
                    .disabled(self.chapter + 1 >= count)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.step_chapter(1, cx);
                    })),
            )
    }

    /// `viewport_probe` 量到正文容器高度后回调进来；变化超过半像素才重排。
    pub(super) fn set_viewport_height(&mut self, height: f32, cx: &mut Context<Self>) {
        if self
            .viewport_height
            .is_some_and(|previous| (previous - height).abs() <= 0.5)
        {
            return;
        }
        self.viewport_height = Some(height);
        cx.notify();
    }

    /// 翻页模式：按当前指纹测一遍本章并分页；指纹没变就直接用缓存。
    fn ensure_pagination(&mut self, window: &Window, cx: &mut Context<Self>) {
        let previous_ratio = self.chapter_ratio;
        let reader = self.reader_config(cx);
        // 目录 / 搜索抽屉都是根 flex row 里的正常子元素，会把正文列压窄；宽度进了指纹，
        // 开合抽屉会自动触发重排。两个抽屉理论上不共存，这里仍按和算，免得将来放开互斥时漏掉。
        let dock = (if self.toc_open {
            super::toc::TOC_WIDTH
        } else {
            0.0
        }) + (if self.search_open {
            search_panel::SEARCH_WIDTH
        } else {
            0.0
        });
        // `mul_add` 而非 `a - 2 * b`：`clippy::suboptimal_flops` 是启用的（无全局放行）。
        // `f32::sub` 需要 `use std::ops::Sub`，改用运算符。
        let content_width =
            (2.0_f32.mul_add(-COLUMN_PADDING, f32::from(window.viewport_size().width)) - dock)
                .min(reader.page_width)
                .max(160.0);
        // 探针量到的就是可排字区域（裁剪盒没有上下留白），直接用它；
        // `- 160` 只是首帧探针还没回报时的保守兜底。
        let viewport_height = self
            .viewport_height
            .unwrap_or_else(|| (f32::from(window.viewport_size().height) - 160.0).max(120.0));
        let layout = ReaderLayout::new(
            reader.font_size,
            reader.line_height,
            reader.paragraph_spacing,
            content_width,
            viewport_height,
            reader.indent,
            reader.font_family.clone(),
        );
        // gpui 渲染文本时会把行高 `pixel_snap` 到设备像素（`elements/text.rs:643`），
        // 分页必须用同一个值，否则页起点逐页漂移、页首那行被裁。
        let line_height_px = f32::from(window.pixel_snap(px(layout.line_height_px())));
        let fingerprint = layout.fingerprint(self.chapter);
        if self.page_cache.as_ref().is_some_and(|cache| {
            cache.fingerprint == fingerprint
                && cache.line_height_px.to_bits() == line_height_px.to_bits()
        }) {
            return;
        }
        let Some(chapter) = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(self.chapter))
        else {
            return;
        };
        // 章标题也算一段（同字号、加粗），否则分页偏移和实际内容高度对不上。
        let texts: Vec<String> = std::iter::once(chapter.title.clone())
            .chain(
                chapter
                    .paragraphs
                    .iter()
                    .map(|paragraph| layout.display_text(paragraph).into_owned()),
            )
            .collect();
        let style = content_view::measure_style(window, &reader);
        let mut title_style = style.clone();
        title_style.font_weight = FontWeight::BOLD;
        let spacing = reader.paragraph_spacing;
        let metrics: Vec<ParagraphMetrics> = texts
            .iter()
            .enumerate()
            .map(|(index, text)| ParagraphMetrics {
                lines: content_view::measure_lines(
                    window,
                    text,
                    reader.font_size,
                    content_width,
                    if index == 0 { &title_style } else { &style },
                ),
                spacing_after: if index == 0 {
                    line_height_px * 0.75
                } else if index + 1 == texts.len() {
                    0.0
                } else {
                    spacing
                },
            })
            .collect();
        let page_offsets = paginate(&metrics, line_height_px, viewport_height);
        let content_height = crate::reader::layout::content_height(&metrics, line_height_px);
        // 防御：`paginate` 不变量下不会命中，留着是为了测量实现将来变化时不静默出错。
        let broken =
            page_offsets.is_empty() || page_offsets.last().copied().unwrap_or(0.0) > content_height;
        if broken {
            // 测量和分页对不上：退回滚动模式，只提示一次（在渲染后再动 model）。
            self.mode = ReadingMode::Scroll;
            self.page_cache = None;
            if !self.layout_warned {
                self.layout_warned = true;
                let model = self.model.clone();
                cx.defer(move |cx| {
                    model.update(cx, |model, _cx| {
                        model.push_warning(t!("Reader.error.layout_fallback").to_string());
                    });
                });
            }
            return;
        }

        let pages = page_offsets.len();
        self.page_index = if self.page_turn_to_last {
            self.page_turn_to_last = false;
            pages.saturating_sub(1)
        } else {
            page_for_ratio(self.chapter_ratio, pages)
        };
        self.chapter_ratio = ratio_for_page(self.page_index, pages);
        self.page_cache = Some(PageCache {
            fingerprint,
            line_height_px,
            offset: page_offsets,
        });
        // 跨章回退时 `goto_chapter` 记下的是 ratio = 0，真正的位置由这里改写；
        // 翻页模式下 `capture_scroll_ratio` 已早退，不在这里补记就再没人刷新它。
        if self.chapter_ratio.to_bits() != previous_ratio.to_bits() {
            self.record_progress(cx);
        }
    }

    fn goto_page(&mut self, page: usize, cx: &mut Context<Self>) {
        let pages = self
            .page_cache
            .as_ref()
            .map_or(0, |cache| cache.offset.len());
        if pages == 0 {
            return;
        }
        self.invalidate_scroll_commit();
        self.page_index = page.min(pages - 1);
        self.chapter_ratio = ratio_for_page(self.page_index, pages);
        self.record_progress(cx);
        cx.notify();
    }

    fn turn_page(&mut self, delta: isize, cx: &mut Context<Self>) {
        let pages = self
            .page_cache
            .as_ref()
            .map_or(0, |cache| cache.offset.len());
        if pages == 0 {
            return;
        }
        self.invalidate_scroll_commit();
        let next = self.page_index.cast_signed() + delta;
        if next < 0 {
            if self.chapter == 0 {
                return;
            }
            self.page_turn_to_last = true;
            let target = self.chapter - 1;
            self.goto_chapter(target, cx);
            return;
        }
        if next as usize >= pages {
            let count = self
                .content
                .as_ref()
                .map_or(0, |content| content.chapters.len());
            if self.chapter + 1 >= count {
                return;
            }
            let target = self.chapter + 1;
            self.goto_chapter(target, cx);
            return;
        }
        self.goto_page(next as usize, cx);
    }

    /// 正文区域：翻页模式用裁剪视口 + 纵向位移，滚动模式用滚动容器。
    fn render_body(&self, cx: &Context<Self>) -> AnyElement {
        match self.mode {
            ReadingMode::Page => self.render_page_body(cx),
            ReadingMode::Scroll => self.render_scroll_body(cx),
        }
    }

    fn render_page_body(&self, cx: &Context<Self>) -> AnyElement {
        let Some((start, pages, line_height)) = self.page_cache.as_ref().map(|cache| {
            (
                cache.offset.get(self.page_index).copied().unwrap_or(0.0),
                cache.offset.len(),
                cache.line_height_px,
            )
        }) else {
            // 还没排好页：给一帧空占位。退回滚动容器会在翻页模式里挂上
            // `on_scroll`，可能把陈旧的滚动偏移写成进度。
            return div().flex_1().into_any_element();
        };
        let Some(content) = self.content.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let Some(chapter) = content.chapters.get(self.chapter) else {
            return div().flex_1().into_any_element();
        };
        let count = content.chapters.len();
        let title = chapter.title.clone();
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, cx);
        let layout = ReaderLayout::new(
            reader.font_size,
            reader.line_height,
            reader.paragraph_spacing,
            reader.page_width,
            0.0,
            reader.indent,
            reader.font_family.clone(),
        );
        let paragraphs: Vec<String> = chapter
            .paragraphs
            .iter()
            .map(|paragraph| layout.display_text(paragraph).into_owned())
            .collect();
        let highlights = self.chapter_highlights(&reader, cx);
        let probe =
            content_view::viewport_probe(cx.entity().downgrade(), self.viewport_measured.clone());

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(
                // 上下留白必须在裁剪区之外：gpui 的 `overflow_hidden` 按 border box 裁剪
                // （`style.rs:646-660` 只在有边框时才内缩），padding 带会一并露出来，
                // 页顶 / 页底就会各露出相邻行的一条。
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .py(px(COLUMN_PADDING))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .overflow_hidden()
                            .px(px(COLUMN_PADDING))
                            .child(probe)
                            .child(
                                div()
                                    .relative()
                                    .top(px(-start))
                                    .w_full()
                                    .flex()
                                    .flex_row()
                                    .justify_center()
                                    .child(content_view::chapter_column(
                                        &title,
                                        &paragraphs,
                                        &reader,
                                        line_height,
                                        highlights.as_deref(),
                                    )),
                            ),
                    ),
            )
            .child(self.render_page_nav(pages, count, cx))
            .into_any_element()
    }

    fn render_scroll_body(&self, cx: &Context<Self>) -> AnyElement {
        let Some(content) = self.content.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let Some(chapter) = content.chapters.get(self.chapter) else {
            return div().flex_1().into_any_element();
        };
        let count = content.chapters.len();
        let title = chapter.title.clone();
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, cx);
        let layout = ReaderLayout::new(
            reader.font_size,
            reader.line_height,
            reader.paragraph_spacing,
            reader.page_width,
            0.0,
            reader.indent,
            reader.font_family.clone(),
        );
        let line_height = layout.line_height_px();
        let paragraphs: Vec<String> = chapter
            .paragraphs
            .iter()
            .map(|paragraph| layout.display_text(paragraph).into_owned())
            .collect();
        let highlights = self.chapter_highlights(&reader, cx);
        let column = content_view::chapter_column(
            &title,
            &paragraphs,
            &reader,
            line_height,
            highlights.as_deref(),
        );

        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(
                div()
                    .id("reader-scroll")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .vertical_scrollbar(&self.scroll)
                    .on_scroll(cx.listener(|this, _event: &ScrollEvent, _window, cx| {
                        this.schedule_progress_commit(cx);
                    }))
                    .child(
                        // 居中靠 flex：gpui 没有 auto margin 样式。
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .justify_center()
                            .px(px(COLUMN_PADDING))
                            .py(px(COLUMN_PADDING))
                            .child(column),
                    ),
            )
            .child(self.render_chapter_nav(count, cx))
            .into_any_element()
    }

    fn render_page_nav(&self, pages: usize, count: usize, cx: &Context<Self>) -> impl IntoElement {
        // 与滚动模式的页脚共用一份：`as f32` 会触发 `cast_precision_loss`（无全局放行），
        // 第三份百分比实现只会让两者漂移。
        let percent = self.book_percent(count);
        let at_end = self.chapter + 1 >= count && self.page_index + 1 >= pages;
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_4()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("reader-prev-page")
                    .label(t!("Reader.action.prev_page"))
                    .disabled(self.chapter == 0 && self.page_index == 0)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.turn_page(-1, cx);
                    })),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} · {percent:.0}%",
                        t!(
                            "Reader.page_indicator",
                            page = self.page_index + 1,
                            pages = pages,
                            chapter = self.chapter + 1,
                            total = count
                        )
                    )),
            )
            .child(
                Button::new("reader-next-page")
                    .label(t!("Reader.action.next_page"))
                    .disabled(at_end)
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.turn_page(1, cx);
                    })),
            )
    }
}

impl Render for ReaderPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, cx);

        // 首帧布局算完才知道 max_offset，所以恢复位置一定发生在下一帧。
        if self.restore_pending && matches!(self.load_state, LoadState::Ready) {
            let handle = self.scroll.clone();
            let ratio = self.chapter_ratio.clamp(0.0, 1.0);
            cx.on_next_frame(window, move |page, window, cx| {
                let max = f32::from(handle.max_offset().y);
                handle.set_offset(point(px(0.0), px(ratio_offset(ratio, max))));
                // 标志要留到偏移真正写进去之后再清：在 render 里清掉的话，
                // 「render 已清、本回调未跑」这段里 autosave 仍会采到旧偏移。
                page.restore_pending = false;
                // 只写偏移不重绘：下一帧会被跳过，恢复值要等下一次真实 prepaint 才生效。
                cx.notify();
                window.refresh();
            });
        }

        // 翻页模式：指纹变了才重算分页；分页结果供下面的正文与进度使用。
        if matches!(self.mode, ReadingMode::Page) && matches!(self.load_state, LoadState::Ready) {
            self.ensure_pagination(window, cx);
        }

        let body: AnyElement = match &self.load_state {
            LoadState::Loading => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(t!("Reader.loading").to_string())
                .into_any_element(),
            LoadState::Error(error) => Self::render_error(error).into_any_element(),
            LoadState::Ready => self.render_body(cx),
        };
        let toc = if self.toc_open {
            Some(self.render_toc(cx))
        } else {
            None
        };
        let search = if self.search_open {
            Some(self.render_search(cx))
        } else {
            None
        };
        let typography = if self.typography_open {
            Some(self.render_typography(cx))
        } else {
            None
        };

        div()
            .relative()
            .track_focus(&self.focus)
            .key_context("Reader")
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.background)
            .text_color(colors.foreground)
            .on_key_down(cx.listener(Self::on_key_down))
            .child(self.render_toolbar(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .children(toc)
                    .child(body)
                    .children(search),
            )
            .children(typography)
    }
}

#[cfg(test)]
mod tests {
    use super::{ratio_offset, scroll_ratio};

    #[test]
    fn scroll_ratio_maps_negative_offsets_to_forward_progress() {
        // gpui 的偏移向下为负：0 是章首，-max 是章尾。
        assert_eq!(scroll_ratio(0.0, 1000.0), Some(0.0));
        assert_eq!(scroll_ratio(-500.0, 1000.0), Some(0.5));
        assert_eq!(scroll_ratio(-1000.0, 1000.0), Some(1.0));
        // resize / 亚像素造成的越界要夹住，不能写出负数或 NaN。
        assert_eq!(scroll_ratio(-1200.0, 1000.0), Some(1.0));
        assert_eq!(scroll_ratio(20.0, 1000.0), Some(0.0));
    }

    #[test]
    fn scroll_ratio_is_none_when_nothing_scrolls() {
        assert_eq!(scroll_ratio(0.0, 0.0), None);
        assert_eq!(scroll_ratio(-0.5, 1.0), None);
        assert_eq!(scroll_ratio(0.0, -3.0), None);
    }

    #[test]
    fn ratio_offset_round_trips_and_stays_inside_content() {
        let max = 800.0;
        for ratio in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(scroll_ratio(ratio_offset(ratio, max), max), Some(ratio));
        }
        assert_eq!(ratio_offset(1.5, max), -800.0);
        assert_eq!(ratio_offset(-1.0, 0.0), 0.0);
    }
}
