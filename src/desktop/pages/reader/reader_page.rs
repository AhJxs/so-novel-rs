//! 阅读器页面：加载状态机 + 滚动 / 翻页两种正文 + 目录抽屉 / 排版面板。
//!
//! 两种模式共用同一份位置（`chapter` + `chapter_ratio`）：滚动模式从滚动偏移采样，
//! 翻页模式由分页表换算，切换模式时按比例恢复。

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::select::{SearchableVec, SelectDelegate, SelectEvent, SelectState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IconName, Selectable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Div, Entity, FocusHandle, Focusable as _,
    FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, Render,
    ScrollDelta, ScrollEvent, ScrollHandle, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled, UniformListScrollHandle, Window, div, point, px,
    relative,
};
use rust_i18n::t;

use super::content_view::{self, COLUMN_PADDING};
use super::reader_colors;
use super::search_panel::{self, SearchPanel};
use crate::config::{ReaderConfig, ReadingMode};
use crate::desktop::model::AppModel;
use crate::library::{Book, BookFormat, ReadingProgress};
use crate::reader::layout::{page_for_ratio, page_ratio};
use crate::reader::{
    LayoutFingerprint, ParagraphMetrics, ReaderBook, ReaderError, ReaderLayout, load_book,
    load_book_relaxed, paginate,
};
use crate::utils::system;

/// 解码失败时的候选编码（`encoding_rs` 认识这些标签）。
pub(super) const ENCODING_CHOICES: &[&str] =
    &["UTF-8", "GB18030", "GBK", "Big5", "UTF-16LE", "UTF-16BE"];

/// 关闭阅读器：由 `RootView` 注入（需要同时改 `RootView` 状态与全屏标记）。
pub type CloseReaderFn = Rc<dyn Fn(&mut Window, &mut App)>;

/// 沉浸模式变化通知：`RootView` 用它重画自己的标题栏。
pub type ImmersiveFn = Rc<dyn Fn(&mut App)>;

/// gpui 的滚动偏移向下为负（`set_offset`：越往下滚偏移越负），而进度要的是
/// 0..=1 的正向比例。全部换算都走这两个函数，别在调用点各写一份。
/// 偏移 → 章内比例；内容不足一屏（滚不动）时整章已完全可见，返回 1.0。
// `Option` 是采样接口的既有形状（「这次采样是否可信」），保留它省得调用点与测试
// 跟着改签名；目前所有输入都能给出确定比例，故这一处不做 `unnecessary_wraps` 合并。
#[allow(clippy::unnecessary_wraps)]
fn scroll_ratio(offset_y: f32, max_y: f32) -> Option<f32> {
    if max_y <= 1.0 {
        // 内容不足一屏：整章已完全可见。
        return Some(1.0);
    }
    Some((-offset_y / max_y).clamp(0.0, 1.0))
}

/// 章内比例 → 滚动偏移（负值）。
fn ratio_offset(ratio: f32, max_y: f32) -> f32 {
    -(max_y.max(0.0) * ratio.clamp(0.0, 1.0))
}

/// 偏移 → 恢复位置用的比例。
///
/// 与 `scroll_ratio` 的区别只在「内容不足一屏」：那种章整章可见，**进度的语义**
/// 是 1.0（已读完，见 `scroll_ratio`），但**位置的语义**是章首 0.0 —— 抽屉开合或
/// 窗口缩放让它变得可滚动时，按 1.0 恢复会直接落到章尾。
fn position_ratio(offset_y: f32, max_y: f32) -> f32 {
    if max_y <= 1.0 {
        return 0.0;
    }
    (-offset_y / max_y).clamp(0.0, 1.0)
}

/// 当前章 `display_text` 的缓存键：`(章节号, 排版指纹)`。
///
/// 只收 `display_text` 真正依赖的字段：章节号、缩进，以及渲染 / 折行用到的字号、
/// 字重、行高、字体族；宽度按 `to_bits()` 格式化，避免 `19.0` / `19` 这类浮点噪声。
/// `viewport_height` 与段距不进键 —— `display_text` 不读它们。
fn display_cache_key(chapter: usize, reader: &ReaderConfig, width: f32) -> (u64, String) {
    (
        chapter as u64,
        format!(
            "{}:{:08x}:{}:{}:{}:{}",
            reader.indent,
            width.to_bits(),
            reader.font_size,
            reader.font_weight,
            reader.line_height,
            reader.font_family
        ),
    )
}

/// 打开状态。
pub enum LoadState {
    Loading,
    Ready,
    Error(ReaderError),
}

/// 用户刚选、还没验证成功的编码。
///
/// 不用 `Option<Option<String>>`：两层 `None` 语义太隐晦，且 `clippy::option_option` 是启用的。
enum PendingEncoding {
    /// 显式编码标签。
    Explicit(String),
    /// 改回自动探测。
    Auto,
}

impl PendingEncoding {
    fn into_option(self) -> Option<String> {
        match self {
            Self::Explicit(encoding) => Some(encoding),
            Self::Auto => None,
        }
    }
}

/// 阅读器页面。
pub struct ReaderPage {
    pub(super) model: Entity<AppModel>,
    pub(super) book_id: String,
    /// 打开时的书籍快照；关闭时用于落盘。
    pub(super) book: Book,
    pub(super) content: Option<ReaderBook>,
    /// 用户刚选、还没验证成功的编码。
    ///
    /// 只有解码成功才写回 store：选错不该把错误标签永久留在书架数据里。
    pending_encoding: Option<PendingEncoding>,
    /// 用户点了「仍要打开」：本次加载跳过乱码检查。
    force_decode: bool,
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
    /// 下一帧恢复用的位置比例；None 表示沿用 `chapter_ratio`（跳章 / 切模式）。
    restore_position: Option<f32>,
    /// 每次滚动自增，只有最新一代的防抖任务会写进度。
    scroll_generation: u64,
    /// 上一次自动续章的时间；冷却期内不再接下一章。
    last_chapter_jump: Option<std::time::Instant>,
    /// 翻页模式：当前页号。
    page_index: usize,
    /// 翻页模式：当前章的分页缓存。
    pub(super) page_cache: Option<PageCache>,
    /// 当前章 `display_text` 的结果 + 生成它的指纹；章节 / 排版 / 列宽不变就复用。
    ///
    /// 整章一次性缓存：翻页模式每帧要算两遍（分页测量 + 正文渲染），滚动模式一遍，
    /// 再加 `chapter_column` 的逐段拷贝，省掉每帧数千次字符串分配。
    display_cache: Option<(u64, String, Rc<Vec<String>>)>,
    /// 越界向前翻页时置位：重算分页后落到新章最后一页。
    pub(super) page_turn_to_last: bool,
    /// 正文视口实测高度（px）；`None` = 还没量到，先按窗口高度估。
    viewport_height: Option<f32>,
    /// 与 `viewport_probe` 共享的最近一次实测值，用来判断要不要通知。
    viewport_measured: Rc<Cell<Option<f32>>>,
    /// 上一帧的正文列宽；抽屉开合 / 窗口缩放后据此恢复滚动位置。
    last_content_width: Option<f32>,
    /// 测量异常提示过就不再刷屏。
    layout_warned: bool,
    /// 章节标题快照；加载完成时建一次，抽屉每帧零拷贝共享。
    pub(super) toc_titles: Option<Rc<Vec<String>>>,
    /// 目录顶部的章节过滤框。
    pub(super) toc_filter: Entity<InputState>,
    /// 目录列表的滚动句柄：打开时滚到当前章。
    pub(super) toc_scroll: UniformListScrollHandle,
    /// 搜索抽屉状态。
    pub(super) search: SearchPanel,
    pub(super) search_input: Entity<InputState>,
    pub(super) search_open: bool,
    /// 书签抽屉。
    pub(super) bookmarks_open: bool,
    /// 沉浸模式的悬浮工具栏是否可见。
    toolbar_visible: bool,
    /// 收起工具栏的延时任务代次：鼠标重新进入就作废旧的延时。
    toolbar_hide_gen: u64,
    /// 「更多」抽屉。
    pub(super) more_open: bool,
    /// 书签列表的滚动位置。
    pub(super) bookmarks_scroll: ScrollHandle,
    /// 在跑的扫描任务的取消旗标。
    pub(super) search_cancel: Option<Arc<AtomicBool>>,
    pub(super) toc_open: bool,
    pub(super) typography_open: bool,
    /// 系统字体下拉。首次打开设置面板时才枚举字体并创建（枚举几百个名字不值得每帧做）。
    pub(super) font_select: Option<Entity<SelectState<SearchableVec<SharedString>>>>,
    immersive: bool,
    generation: u64,
    progress_dirty: bool,
    /// 自动保存循环是否已经在跑；换编码 / 重新定位会再成功加载一次，不能各起一条。
    autosave_running: bool,
    focus: FocusHandle,
    immersive_handler: Option<ImmersiveFn>,
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
        // 从未打开过的书（`updated_unix == 0`）用配置里的默认阅读方式；读过的书恢复上次的模式。
        let mode = if book.progress.updated_unix == 0 {
            model.read(cx).config.reader.clamped().default_mode
        } else {
            book.progress.mode
        };
        let page = Self {
            model,
            book_id: book.id.clone(),
            mode,
            chapter: book.progress.chapter,
            chapter_ratio: book.progress.chapter_ratio,
            book,
            content: None,
            pending_encoding: None,
            force_decode: false,
            load_state: LoadState::Loading,
            finished,
            scroll: ScrollHandle::new(),
            restore_pending: true,
            restore_position: None,
            scroll_generation: 0,
            last_chapter_jump: None,
            page_index: 0,
            page_cache: None,
            display_cache: None,
            page_turn_to_last: false,
            viewport_height: None,
            viewport_measured: Rc::new(Cell::new(None)),
            last_content_width: None,
            layout_warned: false,
            toc_titles: None,
            toc_filter,
            toc_scroll: UniformListScrollHandle::new(),
            search: SearchPanel::new(),
            search_input,
            search_open: false,
            bookmarks_open: false,
            toolbar_visible: false,
            toolbar_hide_gen: 0,
            more_open: false,
            bookmarks_scroll: ScrollHandle::new(),
            search_cancel: None,
            toc_open: false,
            typography_open: false,
            font_select: None,
            immersive: false,
            generation: 1,
            progress_dirty: false,
            autosave_running: false,
            focus,
            immersive_handler: None,
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

    /// 注入「沉浸模式变了」的回调（`RootView` 调用）。
    pub fn set_immersive_handler(&mut self, handler: ImmersiveFn) {
        self.immersive_handler = Some(handler);
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
        // 打不开的书不该被记成「在读」：失败态没有可信的位置可写。
        if matches!(self.load_state, LoadState::Ready) {
            self.record_progress(cx);
        }
        self.progress_dirty = false;
        self.persist_settings(cx);
        self.model.update(cx, |model, _| model.flush_library());
    }

    /// 关掉所有抽屉 —— 抽屉之间互斥，开新的之前先调它。
    ///
    /// 收 `window` 是因为 `close_toc` / `set_search_open` / `set_bookmarks_open`
    /// 关抽屉时要把焦点收回正文。
    fn close_drawers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.typography_open {
            self.typography_open = false;
            self.persist_settings(cx);
        }
        if self.toc_open {
            self.close_toc(window, cx);
        }
        if self.search_open {
            self.set_search_open(false, window, cx);
        }
        if self.bookmarks_open {
            self.set_bookmarks_open(false, window, cx);
        }
        self.more_open = false;
        self.cancel_search();
    }

    /// 关阅读设置弹窗：面板里的改动在关闭时才落盘。
    pub(super) fn close_settings(&mut self, cx: &mut Context<Self>) {
        if !self.typography_open {
            return;
        }
        self.typography_open = false;
        self.persist_settings(cx);
        cx.notify();
    }

    /// 首次打开设置面板时枚举系统字体、建可搜索下拉。
    ///
    /// 选「系统默认」存空串（`font-family = ""`）；字体名按字典序去重，
    /// 「系统默认」固定在第一项。字体列表一次枚举后缓存在页面上，改设置不重列。
    fn ensure_font_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.font_select.is_some() {
            return;
        }
        let default_label = t!("Reader.typography.font_default").to_string();
        let mut families = cx.text_system().all_font_names();
        families.sort();
        families.dedup();
        let mut names: Vec<SharedString> = Vec::with_capacity(families.len() + 1);
        names.push(SharedString::from(default_label.clone()));
        names.extend(families.into_iter().map(SharedString::from));

        let current = self.reader_config(cx).font_family;
        let current_label = if current.is_empty() {
            default_label.clone()
        } else {
            current
        };
        let items: SearchableVec<SharedString> = names.into();
        let pos = <SearchableVec<SharedString> as SelectDelegate>::position(
            &items,
            &SharedString::from(current_label),
        );
        let state = cx.new(|cx| SelectState::new(items, pos, window, cx).searchable(true));
        cx.subscribe(&state, move |this, _state, event, cx| {
            if let SelectEvent::Confirm(Some(picked)) = event {
                let picked = picked.to_string();
                let family = if picked == default_label {
                    String::new()
                } else {
                    picked
                };
                this.update_reader_config(move |reader| reader.font_family = family, cx);
            }
        })
        .detach();
        self.font_select = Some(state);
    }

    /// 进出沉浸模式：进 = 隐藏应用内工具栏 + 窗口全屏；出 = 反过来。
    pub(super) fn toggle_immersive(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.immersive = !self.immersive;
        self.toolbar_visible = false;
        // 作废在途的收起任务：否则退出→再进入的 1.5 s 内，旧任务会把新唤出的工具栏关掉。
        self.toolbar_hide_gen = self.toolbar_hide_gen.wrapping_add(1);
        if self.immersive {
            self.close_drawers(window, cx);
        }
        window.toggle_fullscreen();
        if let Some(handler) = self.immersive_handler.clone() {
            handler(cx);
        }
        cx.notify();
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
        // 位置用 `position_ratio` 而不是 `chapter_ratio`：一屏放得下的章进度是 1.0，
        // 改小字号让它变得可滚动时会直接落到章尾。
        self.restore_position = Some(position_ratio(
            f32::from(self.scroll.offset().y),
            f32::from(self.scroll.max_offset().y),
        ));
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
        let force = self.force_decode;
        let book_id = self.book_id.clone();
        let cache_dir = self.model.read(cx).reader_cache_dir();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // 「仍要打开」是跳过乱码检查的有损解码：既不能读缓存（内容口径不同），
                    // 也不能写缓存（否则下次正常打开会吃到放宽后的结果）。
                    if force {
                        return load_book_relaxed(&path, format, encoding.as_deref());
                    }
                    if let Some(dir) = cache_dir.as_deref()
                        && let Some(book) = crate::reader::cache::load(
                            dir,
                            &book_id,
                            &path,
                            format,
                            encoding.as_deref(),
                        )
                    {
                        return Ok(book);
                    }
                    let before = crate::reader::cache::source_fingerprint(&path);
                    let result = load_book(&path, format, encoding.as_deref());
                    if let (Ok(book), Some(dir)) = (&result, cache_dir.as_deref()) {
                        crate::reader::cache::store(
                            dir,
                            &book_id,
                            &path,
                            format,
                            encoding.as_deref(),
                            book,
                            before,
                        );
                    }
                    result
                })
                .await;
            let _ = this.update(cx, |page, cx| page.finish_load(generation, result, cx));
        })
        .detach();
    }

    /// 清掉旧的正文 / 分页 / 搜索状态，从磁盘重新加载。
    ///
    /// **位置由调用方负责**：换编码重试沿用当前位置，重新定位用新书自己的进度。
    /// 这里只置 `restore_pending`，保证新布局跑完后再按 `chapter` / `chapter_ratio` 恢复。
    fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        self.cancel_search();
        // 不能重建面板：`generation` 必须跨重载保留，否则旧书在途的接收循环会在
        // 下一次搜索时重新撞上同一个代次。`cancel_search` 已经先推进过它。
        self.search.reset();
        self.content = None;
        // 同一章号也可能换了一份正文（换编码 / 重新定位 / 重新分章），章节号单独不足以失效。
        self.display_cache = None;
        self.page_cache = None;
        self.toc_titles = None;
        // 位置由调用方决定，一律按 `chapter` / `chapter_ratio` 恢复。
        self.restore_position = None;
        self.restore_pending = true;
        self.load_state = LoadState::Loading;
        self.generation = self.generation.wrapping_add(1);
        self.spawn_load(cx);
        cx.notify();
    }

    /// 换一种编码重新解码。选定值先只在内存里，**解码成功后才写回 store**。
    pub(super) fn retry_with_encoding(
        &mut self,
        encoding: Option<&'static str>,
        cx: &mut Context<Self>,
    ) {
        // 点已经生效的标签不必重读整个文件（失败态 `content` 为空，仍会重试）。
        if self.content.is_some() && self.book.encoding.as_deref() == encoding {
            return;
        }
        self.force_decode = false;
        self.book.encoding = encoding.map(str::to_string);
        self.pending_encoding = Some(encoding.map_or(PendingEncoding::Auto, |label| {
            PendingEncoding::Explicit(label.to_string())
        }));
        self.reload_from_disk(cx);
    }

    /// 文件丢了：选一个新路径，选完交给 `apply_relink`。
    fn pick_relink(&self, cx: &Context<Self>) {
        let format = self.book.format;
        let book_id = self.book_id.clone();
        let page = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            let file = rfd::AsyncFileDialog::new()
                .add_filter(format.label(), &[format.extension()])
                .set_title(t!("Reader.error.action.relink").as_ref())
                .pick_file()
                .await;
            let Some(file) = file else {
                return;
            };
            let path = file.path().to_path_buf();
            let _ = page.update(async_cx, |page, cx| {
                page.apply_relink(&book_id, &path, cx);
            });
        })
        .detach();
    }

    /// 重新定位：换 ID / 路径后按新书记录重载。
    fn apply_relink(&mut self, book_id: &str, path: &Path, cx: &mut Context<Self>) {
        let result = self
            .model
            .update(cx, |model, _cx| model.relink_book(book_id, path));
        match result {
            Ok(new_id) => {
                let Some(book) = self.model.read(cx).book_snapshot(&new_id) else {
                    self.model.update(cx, |model, _cx| {
                        model.push_error(t!("Library.error.book_not_found").to_string());
                    });
                    return;
                };
                self.book_id = new_id;
                self.chapter = book.progress.chapter;
                self.chapter_ratio = book.progress.chapter_ratio;
                self.finished = book.progress.finished;
                self.book = book;
                // 上一本书没验证成功的编码不能带给新书；新书的解码检查也要重新走一遍。
                self.pending_encoding = None;
                self.force_decode = false;
                self.reload_from_disk(cx);
            }
            Err(error) => {
                let message = error.message();
                self.model
                    .update(cx, |model, _cx| model.push_error(message));
                cx.notify();
            }
        }
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
                let forced = self.force_decode;
                self.toc_titles = Some(Rc::new(
                    content
                        .chapters
                        .iter()
                        .map(|chapter| chapter.title.clone())
                        .collect(),
                ));
                self.content = Some(content);
                // 重新加载后同章号指向新正文：旧缓存必须作废（指纹里的章节号看不出这点）。
                self.display_cache = None;
                self.load_state = LoadState::Ready;
                self.invalidate_scroll_commit();
                // 加载完成按书架里的进度（`chapter_ratio`）恢复，不接旧的抽样值。
                self.restore_position = None;
                self.restore_pending = true;
                let book_id = self.book_id.clone();
                // 重新解析（换编码 / 重新定位 / 新分章规则）后章节数可能变少：
                // 越界书签指向不存在的章，先清掉再写进度。必须早于 `mark_book_opened`：
                // 那时 store 里还是上一次的章节数，才能判断出「这次真的变少了」；
                // 「仍要打开」是有损解析，不算章节消失的证据，交给 `trusted` 拦下。
                self.model.update(cx, |model, _| {
                    model.prune_bookmarks_after_load(&book_id, chapter_count, !forced);
                });
                self.model.update(cx, |model, _| {
                    // 「仍要打开」是一次性有损打开：探测结果也不可信，别写回书架，
                    // 否则下次正常打开仍会撞同一个乱码检查。
                    let encoding = if forced { None } else { encoding };
                    model.mark_book_opened(&book_id, chapter_count as u32, encoding);
                });
                if forced {
                    self.pending_encoding = None;
                    self.force_decode = false;
                } else if let Some(pending) = self.pending_encoding.take() {
                    let book_id = self.book_id.clone();
                    self.model.update(cx, |model, _| {
                        model.set_book_encoding(&book_id, pending.into_option());
                    });
                }
                self.record_progress(cx);
                self.start_autosave(cx);
            }
            Err(error) => self.load_state = LoadState::Error(error),
        }
        cx.notify();
    }

    // `cx` 只读：`spawn_autosave` 收 `&Context`；写成 `&mut` 会撞 `needless_pass_by_ref_mut`。
    fn start_autosave(&mut self, cx: &Context<Self>) {
        // 换编码 / 重新定位都会再成功加载一次：只留一个循环，别让它们叠加。
        if self.autosave_running {
            return;
        }
        self.autosave_running = true;
        Self::spawn_autosave(cx);
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

    /// 把当前滚动位置换算成章内比例（滚不动时整章已读，记 1.0）。
    pub(super) fn capture_scroll_ratio(&mut self) {
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

    /// 正文真的可读了：加载完成且内容在手。加载中 / 错误态没有可信的位置可写。
    pub(super) const fn is_ready(&self) -> bool {
        matches!(self.load_state, LoadState::Ready) && self.content.is_some()
    }

    pub(super) fn record_progress(&mut self, cx: &mut Context<Self>) {
        if !self.is_ready() {
            return;
        }
        if matches!(self.mode, ReadingMode::Page)
            && self.page_cache.is_none()
            && matches!(self.load_state, LoadState::Ready)
        {
            // 翻页模式还没排好页：此刻的 chapter_ratio 是旧布局的量化结果，不写。
            return;
        }
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
            model.record_reading_progress(&book_id, &progress);
        });
        self.progress_dirty = true;
    }

    fn toggle_mode(&mut self, cx: &mut Context<Self>) {
        if !self.is_ready() {
            return;
        }
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
            // 进翻页前先丢掉滚动模式留下的旧测量，占位帧带探针会重新量。
            self.viewport_height = None;
            // 探针只在「与上次实测不同」时回报；不清掉共享缓存，重新进翻页模式可能量了也不回调，
            // 正文会一直停在占位帧上。
            self.viewport_measured.set(None);
        }
        // 切模式按 `chapter_ratio` 归位（翻页模式靠它算页号），不接滚动模式的抽样值。
        self.restore_position = None;
        self.restore_pending = true;
        let book_id = self.book_id.clone();
        let mode = self.mode;
        self.model
            .update(cx, |model, _| model.set_reading_mode(&book_id, mode));
        self.record_progress(cx);
        cx.notify();
    }

    /// 换章 / 章内跳转共用：旧分页与旧恢复状态都属于上一处位置。
    pub(super) fn reset_chapter_navigation(&mut self, to_last: bool) {
        self.invalidate_scroll_commit();
        // 分页缓存属于上一处位置：不清掉的话同一帧里的第二次翻页会拿旧书页数继续跳（N → N+2）。
        self.page_cache = None;
        self.page_index = 0;
        // 恢复状态同理：跳章 / 章内跳转都按新的 `chapter_ratio` 归位，
        // 别让宽度变化或改排版留下的抽样值接管这次恢复。
        self.restore_position = None;
        // 跨章回退只是本次翻页的一次性提示：每个跳转入口都要重新决定，
        // 否则残留的 true 会让下一次分页落到目标章末页。
        self.page_turn_to_last = to_last;
        self.restore_pending = true;
    }

    /// 跳到第 `index` 章（0-based），章内位置归零。
    pub(super) fn goto_chapter(&mut self, index: usize, cx: &mut Context<Self>) {
        self.goto_chapter_with(index, false, cx);
    }

    fn goto_chapter_with(&mut self, index: usize, to_last: bool, cx: &mut Context<Self>) {
        let Some(content) = self.content.as_ref() else {
            return;
        };
        if index >= content.chapters.len() {
            return;
        }
        self.chapter = index;
        self.reset_chapter_navigation(to_last);
        self.chapter_ratio = 0.0;
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
    pub(super) fn close_toc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.toc_open = false;
        // 过滤词不跨次保留：下次打开看到的是完整的书，而不是上次的残留结果。
        if !self.toc_filter.read(cx).value().is_empty() {
            self.toc_filter
                .update(cx, |input, cx| input.set_value("", window, cx));
        }
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
            // 书签与搜索是两个右侧抽屉：开搜索前先收掉书签（关它会把焦点还给正文）。
            if self.bookmarks_open {
                self.set_bookmarks_open(false, window, cx);
            }
            self.more_open = false;
            let handle = self.search_input.read(cx).focus_handle(cx);
            window.focus(&handle, cx);
        } else {
            self.cancel_search();
            // 查询词不跨次保留：重开时是「输入关键词」提示，而不是上次的结果状态。
            if !self.search_input.read(cx).value().is_empty() {
                self.search_input
                    .update(cx, |input, cx| input.set_value("", window, cx));
            }
            self.focus_body(window, cx);
        }
    }

    /// 开 / 关书签抽屉：开时关掉另一个右侧抽屉，关时把焦点收回正文。
    ///
    /// 与 `set_search_open` 同理，开合只走这一个入口：抽屉卸掉后焦点若还留在里面，
    /// 除 Esc 外的快捷键会全部失灵（Task 18 / 20 的同类坑）。
    pub(super) fn set_bookmarks_open(
        &mut self,
        open: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bookmarks_open = open;
        if open {
            if self.toc_open {
                self.close_toc(window, cx);
            }
            if self.search_open {
                self.set_search_open(false, window, cx);
            }
            self.more_open = false;
        } else {
            self.focus_body(window, cx);
        }
    }

    fn toggle_toc(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toc_open {
            self.close_toc(window, cx);
        } else {
            // 三个右侧抽屉不共存：开目录先关搜索与书签，正文列宽只按一个抽屉算。
            if self.search_open {
                self.set_search_open(false, window, cx);
            }
            if self.bookmarks_open {
                self.set_bookmarks_open(false, window, cx);
            }
            self.more_open = false;
            self.toc_open = true;
            self.typography_open = false;
            // 打开就停在当前章：382 章的列表从第 1 章开始翻太费手。
            if let Some(titles) = self.toc_titles.as_ref()
                && !titles.is_empty()
            {
                self.toc_scroll
                    .scroll_to_item(self.chapter.min(titles.len() - 1), ScrollStrategy::Center);
            }
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
        // 字母快捷键不抢带修饰键的按键：Cmd/Ctrl+B 是侧边栏，不能让书签抽屉跟着开。
        let plain = !(event.keystroke.modifiers.control
            || event.keystroke.modifiers.alt
            || event.keystroke.modifiers.platform);
        if self.shortcuts_blocked(window, cx) {
            if key == "escape" {
                self.close_drawers(window, cx);
                cx.notify();
            }
            return;
        }
        // 加载中 / 错误态只留 Esc（关抽屉 / 退沉浸 / 返回）：别滚动、切模式或开空书签。
        if !self.is_ready() && key != "escape" {
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
            "t" if plain => self.toggle_toc(window, cx),
            "s" if plain => {
                self.toc_open = false;
                self.typography_open = false;
                let open = !self.search_open;
                self.set_search_open(open, window, cx);
                cx.notify();
            }
            "b" if plain => {
                self.typography_open = false;
                let open = !self.bookmarks_open;
                self.set_bookmarks_open(open, window, cx);
                cx.notify();
            }
            "m" if plain => self.toggle_mode(cx),
            "f11" if !event.is_held => self.toggle_immersive(window, cx),
            "escape" => {
                if self.toc_open
                    || self.search_open
                    || self.bookmarks_open
                    || self.typography_open
                    || self.more_open
                {
                    self.close_drawers(window, cx);
                    cx.notify();
                } else if self.immersive {
                    self.toggle_immersive(window, cx);
                } else if let Some(handler) = self.close_handler.clone() {
                    // `cx.listener` 在回调期间租着本实体，关闭回调要 read/update 它。
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

    /// 工具栏 / 页脚统一的图标按钮：ghost + compact + 提示，激活态由调用方 `.selected()`。
    fn chrome_button(
        id: impl Into<gpui_kit::ElementId>,
        icon: IconName,
        tooltip: String,
    ) -> Button {
        Button::new(id)
            .ghost()
            .compact()
            .icon(icon)
            .tooltip(tooltip)
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let close_handler = self.close_handler.clone();
        // 图标画的是点下去会切到的模式，提示语把动作讲全。
        let (mode_icon, mode_tooltip) = match self.mode {
            ReadingMode::Scroll => (
                IconName::FileText,
                t!("Reader.action.mode_to_page").to_string(),
            ),
            ReadingMode::Page => (
                IconName::ChevronsUpDown,
                t!("Reader.action.mode_to_scroll").to_string(),
            ),
        };
        let immersive_tooltip = if self.immersive {
            t!("Reader.action.exit_immersive").to_string()
        } else {
            t!("Reader.action.immersive").to_string()
        };
        let immersive_icon = if self.immersive {
            IconName::Minimize
        } else {
            IconName::Maximize
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .h(px(48.0))
            .border_b_1()
            .border_color(cx.theme().border)
            // 工具栏用应用主题铺底：阅读主题只负责正文纸面，否则浅色纸面上
            // ghost 按钮的浅色文字会整片看不见。
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                Self::chrome_button(
                    "reader-back",
                    IconName::ArrowLeft,
                    t!("Reader.action.back").to_string(),
                )
                .on_click(move |_event, window, cx| {
                    if let Some(handler) = &close_handler {
                        handler(window, cx);
                    }
                }),
            )
            .child(
                Self::chrome_button(
                    "reader-toc",
                    IconName::Menu,
                    t!("Reader.action.toc").to_string(),
                )
                .selected(self.toc_open)
                .on_click(cx.listener(|this, _event, window, cx| {
                    this.toggle_toc(window, cx);
                })),
            )
            .child(
                Self::chrome_button(
                    "reader-search",
                    IconName::Search,
                    t!("Reader.action.search").to_string(),
                )
                .selected(self.search_open)
                .on_click(cx.listener(|this, _event, window, cx| {
                    this.toc_open = false;
                    this.typography_open = false;
                    let open = !this.search_open;
                    this.set_search_open(open, window, cx);
                    cx.notify();
                })),
            )
            .child(
                Self::chrome_button(
                    "reader-bookmarks-toggle",
                    IconName::Star,
                    t!("Reader.action.bookmarks").to_string(),
                )
                .selected(self.bookmarks_open)
                .on_click(cx.listener(|this, _event, window, cx| {
                    this.typography_open = false;
                    let open = !this.bookmarks_open;
                    this.set_bookmarks_open(open, window, cx);
                    cx.notify();
                })),
            )
            .child(
                Self::chrome_button(
                    "reader-more",
                    IconName::Ellipsis,
                    t!("Reader.action.more").to_string(),
                )
                .selected(self.more_open)
                .on_click(cx.listener(|this, _event, window, cx| {
                    let open = !this.more_open;
                    this.close_drawers(window, cx);
                    this.more_open = open;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .text_center()
                    .font_weight(FontWeight::MEDIUM)
                    .truncate()
                    .child(SharedString::from(self.book.title.clone())),
            )
            .child(
                Self::chrome_button(
                    "reader-settings",
                    IconName::Settings,
                    t!("Reader.action.settings").to_string(),
                )
                .selected(self.typography_open)
                .on_click(cx.listener(|this, _event, window, cx| {
                    let open = !this.typography_open;
                    // 设置弹窗与四个抽屉不共存；关掉正在开的那个时 `close_drawers`
                    // 已经替它 `persist_settings` 了，这里不必再写一次。
                    this.close_drawers(window, cx);
                    if open {
                        this.ensure_font_select(window, cx);
                    }
                    this.typography_open = open;
                    cx.notify();
                })),
            )
            .child(
                Self::chrome_button("reader-mode", mode_icon, mode_tooltip)
                    .on_click(cx.listener(|this, _event, _window, cx| this.toggle_mode(cx))),
            )
            .child(
                Self::chrome_button("reader-immersive-toggle", immersive_icon, immersive_tooltip)
                    .selected(self.immersive)
                    .on_click(cx.listener(|this, _event, window, cx| {
                        this.toggle_immersive(window, cx);
                    })),
            )
    }

    /// 页脚外壳：左「上一」+ 居中进度 + 右「下一」，配色与应用主题一致。
    fn footer_shell(
        prev: impl IntoElement,
        center: impl IntoElement,
        next: impl IntoElement,
        cx: &App,
    ) -> Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(prev)
            .child(div().flex_1().min_w_0().child(center))
            .child(next)
    }

    /// 居中进度：一行文字 + 细进度条。
    fn progress_indicator(text: String, percent: f64, cx: &App) -> Div {
        let ratio = (percent / 100.0).clamp(0.0, 1.0) as f32;
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text),
            )
            .child(
                div()
                    .w(px(220.0))
                    .h(px(3.0))
                    .rounded_full()
                    .overflow_hidden()
                    .bg(cx.theme().muted)
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(cx.theme().primary)
                            .w(relative(ratio)),
                    ),
            )
    }

    /// 出错时的正文区：原因 + 该错误支持的操作按钮。
    fn render_error(&self, error: &ReaderError, cx: &Context<Self>) -> impl IntoElement {
        let close_handler = self.close_handler.clone();
        let mut actions: Vec<AnyElement> = Vec::new();

        if matches!(error, ReaderError::MissingFile | ReaderError::Epub(_)) {
            actions.push(
                Button::new("reader-error-relink")
                    .primary()
                    .compact()
                    .label(t!("Reader.error.action.relink"))
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.pick_relink(cx);
                    }))
                    .into_any_element(),
            );
        }

        let txt = matches!(self.book.format, BookFormat::Txt);
        // `InvalidEncoding` 来自书架数据里的非法标签，换一个编码同样能修好；
        // 编码只对 TXT 有意义，EPUB 上给这排按钮只会写回一个假标签。
        if matches!(error, ReaderError::Decode(BookFormat::Txt, _))
            || (txt && matches!(error, ReaderError::InvalidEncoding(_)))
        {
            for &label in ENCODING_CHOICES {
                actions.push(
                    Button::new(format!("reader-encoding-{label}"))
                        .ghost()
                        .compact()
                        .label(label)
                        .on_click(cx.listener(move |this, _event, _window, cx| {
                            this.retry_with_encoding(Some(label), cx);
                        }))
                        .into_any_element(),
                );
            }
        }

        // 密度判断只是启发式：猜错时给用户一条「仍然打开」的出路。
        if matches!(error, ReaderError::Decode(BookFormat::Txt, _)) {
            actions.push(
                Button::new("reader-error-force-open")
                    .ghost()
                    .compact()
                    .label(t!("Reader.error.action.force_open"))
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.force_decode = true;
                        this.reload_from_disk(cx);
                    }))
                    .into_any_element(),
            );
        }

        if matches!(
            error,
            ReaderError::Epub(_)
                | ReaderError::UnsupportedFormat(_)
                | ReaderError::NoContent
                | ReaderError::Io(_)
        ) {
            let path = self.book.path.clone();
            actions.push(
                Button::new("reader-error-external")
                    .ghost()
                    .compact()
                    .label(t!("Reader.error.action.open_external"))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        if let Err(message) = system::open_path(Path::new(&path)) {
                            this.model
                                .update(cx, |model, _cx| model.push_error(message.to_string()));
                        }
                    }))
                    .into_any_element(),
            );
        }

        actions.push(
            Button::new("reader-error-back")
                .ghost()
                .compact()
                .label(t!("Reader.error.action.back"))
                .on_click(move |_event, window, cx| {
                    if let Some(handler) = &close_handler {
                        handler(window, cx);
                    }
                })
                .into_any_element(),
        );

        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .p_8()
            .child(div().text_lg().child(t!(error.message_key()).to_string()))
            .when_some(error.detail(), |el, detail| {
                el.child(div().text_sm().child(detail))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .children(actions),
            )
    }

    /// 沉浸模式：顶部 8px 热区唤出悬浮工具栏，鼠标移出 1.5 s 后收起。
    fn render_immersive_overlay(&self, cx: &Context<Self>) -> AnyElement {
        if !self.toolbar_visible {
            return div()
                .id("reader-immersive-hotzone")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(8.0))
                .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                    if *hovered {
                        // 唤出也是一次新的可见期，旧的收起任务一律作废。
                        this.toolbar_hide_gen = this.toolbar_hide_gen.wrapping_add(1);
                        this.toolbar_visible = true;
                        cx.notify();
                    }
                }))
                .into_any_element();
        }

        div()
            .id("reader-immersive-toolbar")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                this.toolbar_hide_gen = this.toolbar_hide_gen.wrapping_add(1);
                let generation = this.toolbar_hide_gen;
                if *hovered {
                    this.toolbar_visible = true;
                    cx.notify();
                } else if this.immersive {
                    // 移出后 1.5 s 再收起：从工具栏往正文划时不会闪一下。
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(1500))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            if this.toolbar_hide_gen == generation && this.immersive {
                                this.toolbar_visible = false;
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                }
            }))
            .child(self.render_toolbar(cx))
            .into_any_element()
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
        let prev = Self::chrome_button(
            "reader-prev-chapter",
            IconName::ChevronLeft,
            t!("Reader.action.prev_chapter").to_string(),
        )
        .disabled(self.chapter == 0)
        .on_click(cx.listener(|this, _event, _window, cx| {
            this.step_chapter(-1, cx);
        }));
        let next = Self::chrome_button(
            "reader-next-chapter",
            IconName::ChevronRight,
            t!("Reader.action.next_chapter").to_string(),
        )
        .disabled(self.chapter + 1 >= count)
        .on_click(cx.listener(|this, _event, _window, cx| {
            this.step_chapter(1, cx);
        }));
        let text = format!(
            "{} · {percent:.0}%",
            t!(
                "Reader.chapter_indicator",
                current = self.chapter + 1,
                total = count
            )
        );
        Self::footer_shell(prev, Self::progress_indicator(text, percent, cx), next, cx)
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

    /// 抽屉占掉的宽度（px）。
    ///
    /// 目录 / 搜索 / 书签 / 更多抽屉都是根 flex row 里的正常子元素，会把正文列压窄；
    /// 四者互斥，这里仍按逐个累加写，免得将来放开互斥时漏掉。
    fn dock_width(&self) -> f32 {
        (if self.toc_open {
            super::toc::TOC_WIDTH
        } else {
            0.0
        }) + (if self.search_open {
            search_panel::SEARCH_WIDTH
        } else {
            0.0
        }) + (if self.bookmarks_open {
            super::bookmarks_panel::BOOKMARKS_WIDTH
        } else {
            0.0
        }) + (if self.more_open {
            super::more_panel::MORE_WIDTH
        } else {
            0.0
        })
    }

    /// 正文列宽：窗口宽度扣掉抽屉与两侧留白，再与用户设的页宽取小。
    ///
    /// 分页与渲染都必须走这一个值：页宽调大后开抽屉时，列要跟着收窄而不是
    /// 保持 `page_width` 把抽屉挤出屏幕；两边算得不一样还会让折行与页码对不上。
    fn content_width(&self, viewport_width: f32, cx: &App) -> f32 {
        let page_width = self.reader_config(cx).page_width;
        // `mul_add` 而非 `a - 2 * b`：`clippy::suboptimal_flops` 是启用的（无全局放行）。
        // `f32::sub` 需要 `use std::ops::Sub`，改用运算符。
        (2.0_f32.mul_add(-COLUMN_PADDING, viewport_width) - self.dock_width())
            .min(page_width)
            .max(160.0)
    }

    /// 当前章的 `display_text` 结果；章节 / 排版 / 列宽任一变化才重建。
    ///
    /// 分页测量与两个渲染入口共用这一份 `Rc`，不再每帧整章重新拼缩进前缀、
    /// 也不再在 `chapter_column` 里逐段克隆。
    fn display_paragraphs(&mut self, reader: &ReaderConfig, width: f32) -> Rc<Vec<String>> {
        let (chapter, key) = display_cache_key(self.chapter, reader, width);
        if let Some((cached_chapter, cached_key, texts)) = &self.display_cache
            && *cached_chapter == chapter
            && *cached_key == key
        {
            return texts.clone();
        }
        // `display_text` 只看缩进；其余字段传进来只是让排版参数与两个渲染入口同源。
        let layout = ReaderLayout::new(
            reader.font_size,
            reader.line_height,
            reader.paragraph_spacing,
            width,
            0.0,
            reader.indent,
            reader.font_family.clone(),
            reader.font_weight,
        );
        let texts = Rc::new(
            self.content
                .as_ref()
                .and_then(|content| content.chapters.get(self.chapter))
                .map(|chapter| {
                    chapter
                        .paragraphs
                        .iter()
                        .map(|paragraph| layout.display_text(paragraph).into_owned())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        );
        self.display_cache = Some((chapter, key, texts.clone()));
        texts
    }

    /// 翻页模式：按当前指纹测一遍本章并分页；指纹没变就直接用缓存。
    fn ensure_pagination(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(viewport_height) = self.viewport_height else {
            // 还没量到正文视口：用估算高度分页会让页号在探针回报后二次漂移。
            // 占位视图里带探针，下一帧量到后再排。
            return;
        };
        let previous_ratio = self.chapter_ratio;
        let reader = self.reader_config(cx);
        let content_width = self.content_width(f32::from(window.viewport_size().width), cx);
        let layout = ReaderLayout::new(
            reader.font_size,
            reader.line_height,
            reader.paragraph_spacing,
            content_width,
            viewport_height,
            reader.indent,
            reader.font_family.clone(),
            reader.font_weight,
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
        let Some(title) = self
            .content
            .as_ref()
            .and_then(|content| content.chapters.get(self.chapter))
            .map(|chapter| chapter.title.clone())
        else {
            return;
        };
        // 缓存里是本体的 `display_text`；借出来的切片直接喂测量，不再整章 collect 一份 owned。
        let texts = self.display_paragraphs(&reader, content_width);
        // 章标题也算一段（同字号、加粗），否则分页偏移和实际内容高度对不上。
        let paragraphs = std::iter::once(title.as_str()).chain(texts.iter().map(String::as_str));
        let count = texts.len() + 1;
        let style = content_view::measure_style(window, &reader);
        let mut title_style = style.clone();
        title_style.font_weight = FontWeight::BOLD;
        let spacing = reader.paragraph_spacing;
        let metrics: Vec<ParagraphMetrics> = paragraphs
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
                } else if index + 1 == count {
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
        self.chapter_ratio = page_ratio(self.page_index, pages);
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
        self.chapter_ratio = page_ratio(self.page_index, pages);
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
            let target = self.chapter - 1;
            self.goto_chapter_with(target, true, cx);
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
    fn render_body(&mut self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let width = self.content_width(f32::from(window.viewport_size().width), cx);
        match self.mode {
            ReadingMode::Page => self.render_page_body(width, cx),
            ReadingMode::Scroll => self.render_scroll_body(width, cx),
        }
    }

    fn render_page_body(&mut self, width: f32, cx: &Context<Self>) -> AnyElement {
        let Some((start, pages, line_height)) = self.page_cache.as_ref().map(|cache| {
            (
                cache.offset.get(self.page_index).copied().unwrap_or(0.0),
                cache.offset.len(),
                cache.line_height_px,
            )
        }) else {
            // 还没排好页：给一帧带探针的占位。退回滚动容器会在翻页模式里挂上
            // `on_scroll`，可能把陈旧的滚动偏移写成进度。
            //
            // 结构必须与正式布局一致：探针外面是同样的留白内容盒，下面再挂同高的
            // 页脚占位。少了页脚，探针会把页脚高度也算进正文视口，首排永远偏高、
            // 页号在正式布局出来后还要再跳一次；整章一页放得下的书还会被记成已读完。
            let probe = content_view::viewport_probe(
                cx.entity().downgrade(),
                self.viewport_measured.clone(),
            );
            return div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .py(px(COLUMN_PADDING))
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .overflow_hidden()
                                .px(px(COLUMN_PADDING))
                                .child(probe),
                        ),
                )
                // `invisible()` 是 `Visibility::Hidden`：保留布局、不绘制。
                // 不用 `hidden()` —— 那是 `display: none`，页脚高度会消失。
                .child(div().child(self.render_page_nav(1, 1, cx)).invisible())
                .into_any_element();
        };
        let Some(content) = self.content.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let (title, count) = match content.chapters.get(self.chapter) {
            Some(chapter) => (chapter.title.clone(), content.chapters.len()),
            None => return div().flex_1().into_any_element(),
        };
        // 上面已经把标题和章数拷成值：`content` 的借用到此结束，下面才能 `&mut self` 取缓存。
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, cx);
        let paragraphs = self.display_paragraphs(&reader, width);
        let highlights = self.chapter_highlights(&reader, cx);
        let probe =
            content_view::viewport_probe(cx.entity().downgrade(), self.viewport_measured.clone());

        div()
            .flex_1()
            .min_w_0()
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
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .py(px(COLUMN_PADDING))
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
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
                                        paragraphs.as_slice(),
                                        &reader,
                                        width,
                                        line_height,
                                        highlights.as_deref(),
                                    )),
                            ),
                    ),
            )
            .child(self.render_page_nav(pages, count, cx))
            .into_any_element()
    }

    fn render_scroll_body(&mut self, width: f32, cx: &Context<Self>) -> AnyElement {
        let Some(content) = self.content.as_ref() else {
            return div().flex_1().into_any_element();
        };
        let (title, count) = match content.chapters.get(self.chapter) {
            Some(chapter) => (chapter.title.clone(), content.chapters.len()),
            None => return div().flex_1().into_any_element(),
        };
        // 标题 / 章数已拷成值，`content` 的借用结束；下面取缓存要 `&mut self`。
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, cx);
        // `ReaderLayout::line_height_px()` 就是这两个字段的乘积，不必再构造一份排版。
        let line_height = reader.font_size * reader.line_height;
        let paragraphs = self.display_paragraphs(&reader, width);
        let highlights = self.chapter_highlights(&reader, cx);
        let column = content_view::chapter_column(
            &title,
            paragraphs.as_slice(),
            &reader,
            width,
            line_height,
            highlights.as_deref(),
        );

        div()
            .flex_1()
            .min_w_0()
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
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .vertical_scrollbar(&self.scroll)
                    .on_scroll(cx.listener(|this, event: &ScrollEvent, _window, cx| {
                        this.schedule_progress_commit(cx);
                        this.continue_to_next_chapter(event, cx);
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

    const CONTINUE_COOLDOWN: std::time::Duration = std::time::Duration::from_millis(600);

    /// 距上次自动续章是否已经过了冷却期。
    fn may_continue_after(last: Option<std::time::Instant>, now: std::time::Instant) -> bool {
        last.is_none_or(|last| now.duration_since(last) >= Self::CONTINUE_COOLDOWN)
    }

    /// 滚到章尾后继续向下滚，接上下一章（从章首开始）。
    ///
    /// 只认「已经贴底 + 这一下还在往下」：到章尾就立刻跳会让人看不到最后一屏。
    /// `restore_pending` 期间不接：`goto_chapter` 已置位，滚动句柄要等下一帧才回到章首，
    /// 此时再收到惯性滚动就会连跳两章。
    /// 两次自动续章之间隔 `CONTINUE_COOLDOWN`：触控板惯性在一跳后还会继续送向下事件，
    /// 短章会被一口气连跳好几章；往回滚则立刻解除冷却。
    fn continue_to_next_chapter(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        if !matches!(self.mode, ReadingMode::Scroll) || self.restore_pending {
            return;
        }
        let down = match event.delta {
            ScrollDelta::Pixels(delta) => f32::from(delta.y) < 0.0,
            ScrollDelta::Lines(delta) => delta.y < 0.0,
        };
        if !down {
            // 用户往回滚：立刻解除冷却，下一次触底可以正常续章。
            self.last_chapter_jump = None;
            return;
        }
        if !Self::may_continue_after(self.last_chapter_jump, std::time::Instant::now()) {
            return;
        }
        let max = f32::from(self.scroll.max_offset().y);
        let current = f32::from(self.scroll.offset().y);
        if !Self::at_bottom_and_scrolling_down(down, current, max) {
            return;
        }
        let Some(count) = self.content.as_ref().map(|content| content.chapters.len()) else {
            return;
        };
        if self.chapter + 1 >= count {
            return;
        }
        self.goto_chapter(self.chapter + 1, cx);
        self.last_chapter_jump = Some(std::time::Instant::now());
    }

    /// 「这一下滚动是否该接下一章」：已经贴底，且这一下还在向下。
    ///
    /// 偏移向下为负，贴底就是 `offset == -max`；留 1px 容差吸收亚像素与末帧取整。
    /// 整章一屏放得下（`max == 0`）也算贴底：否则短章滚不动，永远接不上下一章。
    fn at_bottom_and_scrolling_down(down: bool, offset_y: f32, max_offset_y: f32) -> bool {
        down && offset_y <= 1.0 - max_offset_y.max(0.0)
    }

    fn render_page_nav(&self, pages: usize, count: usize, cx: &Context<Self>) -> impl IntoElement {
        // 与滚动模式的页脚共用一份：`as f32` 会触发 `cast_precision_loss`（无全局放行），
        // 第三份百分比实现只会让两者漂移。
        let percent = self.book_percent(count);
        let at_end = self.chapter + 1 >= count && self.page_index + 1 >= pages;
        let prev = Self::chrome_button(
            "reader-prev-page",
            IconName::ChevronLeft,
            t!("Reader.action.prev_page").to_string(),
        )
        .disabled(self.chapter == 0 && self.page_index == 0)
        .on_click(cx.listener(|this, _event, _window, cx| {
            this.turn_page(-1, cx);
        }));
        let next = Self::chrome_button(
            "reader-next-page",
            IconName::ChevronRight,
            t!("Reader.action.next_page").to_string(),
        )
        .disabled(at_end)
        .on_click(cx.listener(|this, _event, _window, cx| {
            this.turn_page(1, cx);
        }));
        let text = format!(
            "{} · {percent:.0}%",
            t!(
                "Reader.page_indicator",
                page = self.page_index + 1,
                pages = pages,
                chapter = self.chapter + 1,
                total = count
            )
        );
        Self::footer_shell(prev, Self::progress_indicator(text, percent, cx), next, cx)
    }
}

impl Render for ReaderPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reader = self.reader_config(cx);
        let colors = reader_colors(reader.theme, cx);

        // 抽屉开合、窗口缩放都会改列宽 → 折行变了 → 同一个绝对偏移指向别的文字。
        // 必须在下面的恢复块之前跑：`capture_scroll_ratio` 刷新 `chapter_ratio`，
        // 恢复块同一帧就要用这个值。
        let width = self.content_width(f32::from(window.viewport_size().width), cx);
        let width_changed = self
            .last_content_width
            .is_some_and(|previous| (previous - width).abs() > 0.5);
        if width_changed
            && matches!(self.mode, ReadingMode::Scroll)
            && matches!(self.load_state, LoadState::Ready)
            && !self.restore_pending
        {
            // 折行变了，绝对偏移会指向另一段文字：先记比例，下一帧按新高度恢复。
            self.capture_scroll_ratio();
            // 位置另按 `position_ratio` 采：一屏放得下的章进度是 1.0，让它变得可滚动后
            // 按 1.0 恢复会落到章尾。两个读取都在旧布局上，正是用户此刻看到的位置。
            self.restore_position = Some(position_ratio(
                f32::from(self.scroll.offset().y),
                f32::from(self.scroll.max_offset().y),
            ));
            self.restore_pending = true;
        }
        // 未命中守卫的帧也要更新，否则下一次比较仍对着旧值。
        self.last_content_width = Some(width);

        // 首帧布局算完才知道 max_offset，所以恢复位置一定发生在下一帧。
        if self.restore_pending && matches!(self.load_state, LoadState::Ready) {
            let handle = self.scroll.clone();
            // 宽度 / 排版变化时用采样到的位置；跳章、切模式、加载走 `chapter_ratio`。
            let ratio = self
                .restore_position
                .take()
                .unwrap_or(self.chapter_ratio)
                .clamp(0.0, 1.0);
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
            LoadState::Error(error) => self.render_error(error, cx).into_any_element(),
            LoadState::Ready => self.render_body(window, cx),
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
        let bookmarks = if self.bookmarks_open {
            Some(self.render_bookmarks(cx))
        } else {
            None
        };
        let more = if self.more_open {
            Some(self.render_more(cx))
        } else {
            None
        };
        let overlay = if self.immersive {
            Some(self.render_immersive_overlay(cx))
        } else {
            None
        };
        let typography = if self.typography_open {
            Some(self.render_typography(cx))
        } else {
            None
        };
        // 放到最后取：Rust 2024 下 `impl IntoElement` 会捕获 `&Context` 的生命周期，
        // 先拿 `toolbar` 再调需要 `&mut Context` 的渲染方法会撞借用冲突。
        // 沉浸模式的工具栏由悬浮层渲染，这里不要再白算一份。
        let toolbar = (!self.immersive).then(|| self.render_toolbar(cx));

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
            .when_some(toolbar, ParentElement::child)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .when_some(toc, ParentElement::child)
                    .child(body)
                    .when_some(search, ParentElement::child)
                    .when_some(bookmarks, ParentElement::child)
                    .when_some(more, ParentElement::child),
            )
            .when_some(typography, ParentElement::child)
            .when_some(overlay, ParentElement::child)
    }
}

#[cfg(test)]
mod tests {
    use super::{ReaderPage, display_cache_key, position_ratio, ratio_offset, scroll_ratio};

    #[test]
    fn display_cache_key_covers_chapter_indent_width_and_typography() {
        type Mutator = fn(&mut crate::config::ReaderConfig);
        let reader = crate::config::ReaderConfig::default();
        let base = display_cache_key(1, &reader, 720.0);

        // 同样的输入必须相等：缓存命中全靠这条。
        assert_eq!(base, display_cache_key(1, &reader, 720.0));

        // 章节变了要重建，否则跳到新章还渲染上一章的文字。
        assert_ne!(base, display_cache_key(2, &reader, 720.0));

        // 缩进只影响 `display_text`，不在别的排版字段里：漏了就会缩进切换后不重绘。
        let mut no_indent = reader.clone();
        no_indent.indent = false;
        assert_ne!(base, display_cache_key(1, &no_indent, 720.0));

        // 列宽变化也要换 key：虽然 `display_text` 本身不读宽度，但键与排版 / 分页同源，
        // 宽度变了整章按新列宽重建一次，省得两份缓存各自失效、口径分叉。
        assert_ne!(base, display_cache_key(1, &reader, 721.0));

        let cases: [(&str, Mutator); 4] = [
            ("font_size", |r| r.font_size = 22.0),
            ("font_weight", |r| r.font_weight = 700),
            ("line_height", |r| r.line_height = 2.0),
            ("font_family", |r| r.font_family = "衬线".to_string()),
        ];
        for (name, mutate) in cases {
            let mut changed = reader.clone();
            mutate(&mut changed);
            assert_ne!(
                base,
                display_cache_key(1, &changed, 720.0),
                "改 {name} 后 key 必须变化"
            );
        }
    }

    #[test]
    fn position_ratio_treats_unscrollable_content_as_the_top() {
        // 一屏放得下的章：进度是 1.0（见 `scroll_ratio`），位置却是章首。
        assert_eq!(position_ratio(0.0, 0.0), 0.0);
        // 同一章变宽 / 换字号后可滚动时，从章首开始，而不是落到章尾。
        assert_eq!(position_ratio(0.0, 100.0), 0.0);
        assert_eq!(position_ratio(-50.0, 100.0), 0.5);
        assert_eq!(position_ratio(-200.0, 100.0), 1.0);
    }

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
    fn a_chapter_that_fits_on_screen_is_fully_read() {
        assert_eq!(super::scroll_ratio(0.0, 0.0), Some(1.0));
        assert_eq!(super::scroll_ratio(0.0, 100.0), Some(0.0));
    }

    #[test]
    fn scroll_ratio_treats_unscrollable_content_as_fully_read() {
        assert_eq!(scroll_ratio(0.0, 0.0), Some(1.0));
        assert_eq!(scroll_ratio(-0.5, 1.0), Some(1.0));
        assert_eq!(scroll_ratio(0.0, -3.0), Some(1.0));
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

    #[test]
    fn continues_to_next_chapter_only_at_the_bottom_scrolling_down() {
        // 贴底（offset == -max）再往下：接。
        assert!(ReaderPage::at_bottom_and_scrolling_down(
            true, -800.0, 800.0
        ));
        // 亚像素没贴严也算贴底。
        assert!(ReaderPage::at_bottom_and_scrolling_down(
            true, -799.5, 800.0
        ));
        // 还在章中间：不接，否则最后一屏看不到。
        assert!(!ReaderPage::at_bottom_and_scrolling_down(
            true, -700.0, 800.0
        ));
        // 向上滚到章首不该跳走。
        assert!(!ReaderPage::at_bottom_and_scrolling_down(
            false, -800.0, 800.0
        ));
        // 整章一屏放得下：本来就没得滚，继续向下就该接下一章。
        assert!(ReaderPage::at_bottom_and_scrolling_down(true, 0.0, 0.0));
    }

    #[test]
    fn short_chapters_need_a_pause_between_jumps() {
        let now = std::time::Instant::now();
        assert!(!ReaderPage::may_continue_after(Some(now), now));
        // `Instant - Duration` 会触发 `clippy::unchecked_time_subtraction`；
        // 这里 `now` 刚取到，减 700ms 一定成立，直接 `unwrap`。
        let earlier = now
            .checked_sub(std::time::Duration::from_millis(700))
            .unwrap();
        assert!(ReaderPage::may_continue_after(Some(earlier), now));
        assert!(ReaderPage::may_continue_after(None, now));
    }
}
