//! 书架页：封面卡片网格 + 搜索 / 状态筛选 / 收藏开关 / 排序 / 导入 / 重新扫描。
//!
//! - 数据全部来自 `AppModel.library.store`，每帧用 `library::visible_books` 过滤排序
//!   （书架规模是「个人书库」量级，纯计算过滤比维护缓存简单且不会失效）；
//! - 卡片点击 → `RootView::open_reader`，通过 `set_open_reader_handler` 注入的回调，
//!   页面不持有根视图（避免循环强引用）；
//! - 扫描 / 元数据提取都在后台跑，drain 循环回传后窗口自动重绘。

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, InteractiveElement as _,
    IntoElement, ParentElement, Point, Render, ScrollHandle, StatefulInteractiveElement as _,
    Styled, Window, div,
};
use rust_i18n::t;

use super::card::{BookCard, CardData};
use crate::desktop::components::{EmptyState, PageHeader};
use crate::desktop::model::AppModel;
use crate::library::{ShelfQuery, ShelfSort, ShelfStatusFilter, absolute_dir, visible_books};

/// 书架卡片点击 → 打开阅读器。由 `RootView` 注入。
pub type OpenReaderFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// 每批渲染的卡片数；「加载更多」每次追加一批。
const BATCH: usize = 60;

/// 书架页面 entity。
pub struct LibraryPage {
    /// `dialogs` 子模块也要读写它。
    pub(super) model: Entity<AppModel>,
    /// 搜索框 state 必须由页面持有，否则每次重绘丢焦点与光标。
    search_input: Entity<InputState>,
    scroll: ScrollHandle,
    /// 当前渲染的卡片上限。
    visible_limit: usize,
    /// 打开阅读器的回调（`dialogs` 的「继续阅读」也要用）。
    pub(super) open_reader: Option<OpenReaderFn>,
}

impl LibraryPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 后台扫描 / 元数据 / 阅读进度都会改 `AppModel`：观察到就重绘，
        // 不依赖 gpui 的「渲染期读过该 entity」隐式失效。
        cx.observe(&model, |_this, _model, cx| cx.notify()).detach();

        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("Library.search_placeholder")));
        cx.subscribe_in(&search_input, window, |this, _state, event, _window, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let text = this.search_input.read(cx).value().to_string();
            let changed = this.model.update(cx, |model, _cx| {
                let changed = model.library.query.text != text;
                model.library.query.text = text;
                changed
            });
            if changed {
                this.reset_view(cx);
            }
        })
        .detach();

        Self {
            model,
            search_input,
            scroll: ScrollHandle::new(),
            visible_limit: BATCH,
            open_reader: None,
        }
    }

    /// 由 `RootView` 注入打开阅读器的回调。
    pub fn set_open_reader_handler(&mut self, handler: OpenReaderFn) {
        self.open_reader = Some(handler);
    }

    /// 打开一本书；没有注入回调（理论上不会发生）时静默忽略。
    ///
    /// `pub(super)`：Task 17 的 `dialogs` 子模块里的「继续阅读」也要调它。
    pub(super) fn open_book(&self, book_id: &str, window: &mut Window, cx: &mut App) {
        if let Some(open) = self.open_reader.as_ref() {
            open(book_id, window, cx);
        }
    }

    /// 筛选 / 排序 / 搜索变化后回到顶部并重置批次。
    fn reset_view(&mut self, cx: &mut Context<Self>) {
        self.visible_limit = BATCH;
        self.scroll.set_offset(Point::default());
        cx.notify();
    }

    /// 首次进入 / 下载目录变化时自动扫一次（同一目录只扫一次，靠 `scanned_dir` 去重）。
    fn maybe_auto_scan(&self, cx: &mut Context<Self>) {
        let wanted = absolute_dir(std::path::Path::new(
            &self.model.read(cx).config.download.download_path,
        ));
        if self.model.read(cx).library.scanned_dir.as_ref() != Some(&wanted) {
            self.model
                .update(cx, |model, _cx| model.refresh_library_async());
        }
    }

    /// 重新扫描下载目录。
    fn rescan(&self, cx: &mut Context<Self>) {
        self.model
            .update(cx, |model, _cx| model.refresh_library_async());
        cx.notify();
    }

    /// 打开系统文件对话框导入本地文件（只记录路径，不复制文件）。
    fn pick_and_import(&self, cx: &Context<Self>) {
        let model = self.model.clone();
        let page = cx.entity().downgrade();
        cx.spawn(async move |_weak, async_cx| {
            let files = rfd::AsyncFileDialog::new()
                .add_filter(t!("Library.import.filter_books").as_ref(), &["txt", "epub"])
                .add_filter(t!("Library.import.filter_all").as_ref(), &["*"])
                .set_title(t!("Library.import.dialog_title").as_ref())
                .pick_files()
                .await;
            let Some(files) = files else {
                return;
            };
            let paths: Vec<PathBuf> = files.iter().map(|file| file.path().to_path_buf()).collect();
            let _ = page.update(async_cx, |_page, cx| {
                model.update(cx, |model, _cx| model.import_library_files(&paths));
                cx.notify();
            });
        })
        .detach();
    }

    /// 状态筛选：四个按钮互斥，选中任一状态同时关掉「收藏」开关。
    fn set_status_filter(&mut self, filter: ShelfStatusFilter, cx: &mut Context<Self>) {
        let changed = self.model.update(cx, |model, _cx| {
            let query = &mut model.library.query;
            let changed = query.status != filter || query.favorite_only;
            query.status = filter;
            query.favorite_only = false;
            changed
        });
        if changed {
            self.reset_view(cx);
        }
    }

    /// 「收藏」是独立开关：打开时状态回到「全部」，否则两个筛选叠加会看着像空架。
    fn toggle_favorites(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, _cx| {
            let query = &mut model.library.query;
            query.favorite_only = !query.favorite_only;
            if query.favorite_only {
                query.status = ShelfStatusFilter::All;
            }
        });
        self.reset_view(cx);
    }

    /// 排序循环切换：最近阅读 → 最近添加 → 书名 → 大小 → …
    fn cycle_sort(&mut self, cx: &mut Context<Self>) {
        self.model.update(cx, |model, _cx| {
            let current = model.library.query.sort;
            let index = ShelfSort::ALL
                .iter()
                .position(|sort| *sort == current)
                .unwrap_or(0);
            model.library.query.sort = ShelfSort::ALL[(index + 1) % ShelfSort::ALL.len()];
        });
        self.reset_view(cx);
    }

    /// 清空搜索 + 筛选（空结果页的「清除筛选」按钮）。
    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_input
            .update(cx, |state, cx| state.set_value("", window, cx));
        self.model
            .update(cx, |model, _cx| model.library.query = ShelfQuery::default());
        self.reset_view(cx);
    }

    /// 单个状态筛选按钮：当前生效时用主色，其余 ghost。
    fn status_button(
        id: &'static str,
        filter: ShelfStatusFilter,
        status: ShelfStatusFilter,
        favorite_only: bool,
        cx: &Context<Self>,
    ) -> Button {
        let active = !favorite_only && status == filter;
        let button = Button::new(id).label(t!(filter.label_key()).to_string());
        let button = if active {
            button.primary()
        } else {
            button.ghost()
        };
        button.on_click(cx.listener(move |this, _ev, _window, cx| {
            this.set_status_filter(filter, cx);
        }))
    }

    /// 收藏开关按钮。
    fn favorite_button(favorite_only: bool, cx: &Context<Self>) -> Button {
        let icon = Icon::new(if favorite_only {
            IconName::StarFill
        } else {
            IconName::Star
        });
        let button = Button::new("shelf-filter-favorites")
            .icon(icon)
            .label(t!("Library.filter.favorites").to_string());
        let button = if favorite_only {
            button.primary()
        } else {
            button.ghost()
        };
        button.on_click(cx.listener(|this, _ev, _window, cx| this.toggle_favorites(cx)))
    }

    /// 排序按钮（点一次切下一个）。
    fn sort_button(sort: ShelfSort, cx: &Context<Self>) -> Button {
        Button::new("shelf-sort")
            .ghost()
            .icon(Icon::new(IconName::SortAscending))
            .label(t!(sort.label_key()).to_string())
            .on_click(cx.listener(|this, _ev, _window, cx| this.cycle_sort(cx)))
    }

    /// 搜索框 + 状态筛选 + 收藏开关 + 排序。
    fn render_filters(
        &self,
        sort: ShelfSort,
        status: ShelfStatusFilter,
        favorite_only: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .gap_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.search_input).cleanable(true)),
            )
            .child(Self::status_button(
                "shelf-filter-all",
                ShelfStatusFilter::All,
                status,
                favorite_only,
                cx,
            ))
            .child(Self::status_button(
                "shelf-filter-reading",
                ShelfStatusFilter::Reading,
                status,
                favorite_only,
                cx,
            ))
            .child(Self::status_button(
                "shelf-filter-unread",
                ShelfStatusFilter::Unread,
                status,
                favorite_only,
                cx,
            ))
            .child(Self::status_button(
                "shelf-filter-finished",
                ShelfStatusFilter::Finished,
                status,
                favorite_only,
                cx,
            ))
            .child(Self::favorite_button(favorite_only, cx))
            .child(Self::sort_button(sort, cx))
    }

    /// 空态：整库为空 → 「导入 EPUB / TXT」；筛选无结果 → 「清除筛选」。
    fn render_empty(has_books: bool, cx: &Context<Self>) -> AnyElement {
        let empty = if has_books {
            EmptyState::new(IconName::Search, t!("Library.empty_filtered_title").into())
                .subtitle(t!("Library.empty_filtered_subtitle").into())
        } else {
            EmptyState::new(IconName::BookOpen, t!("Library.empty_title").into())
                .subtitle(t!("Library.empty_subtitle").into())
        };
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(empty)
            .child(if has_books {
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("shelf-empty-import")
                            .primary()
                            .icon(Icon::new(IconName::Plus))
                            .label(t!("Library.action_import"))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.pick_and_import(cx);
                            })),
                    )
                    .child(
                        Button::new("shelf-empty-reset")
                            .ghost()
                            .label(t!("Library.action_clear_filters"))
                            .on_click(cx.listener(|this, _ev, window, cx| {
                                this.clear_filters(window, cx);
                            })),
                    )
            } else {
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("shelf-empty-rescan")
                            .primary()
                            .icon(Icon::new(IconName::Search))
                            .label(t!("Library.action_rescan"))
                            .on_click(cx.listener(|this, _ev, _window, cx| this.rescan(cx))),
                    )
                    .child(
                        Button::new("shelf-empty-import")
                            .ghost()
                            .icon(Icon::new(IconName::Plus))
                            .label(t!("Library.action_import"))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.pick_and_import(cx);
                            })),
                    )
            })
            .into_any_element()
    }

    /// 卡片网格 + 「加载更多」；整块纵向滚动。
    fn render_grid(&self, cards: Vec<CardData>, total: usize, cx: &Context<Self>) -> AnyElement {
        let more = total > cards.len();
        let mut grid = div().flex().flex_wrap().gap_5().w_full().pb_2();
        for data in cards {
            let book_id = data.id.clone();
            let on_open: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> =
                Rc::new(cx.listener(move |this, _ev: &ClickEvent, window, cx| {
                    this.open_book(&book_id, window, cx);
                }));
            let favorite_id = data.id.clone();
            let on_favorite: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> =
                Rc::new(cx.listener(move |this, _ev: &ClickEvent, _window, cx| {
                    // 星标在卡片内部：不阻止冒泡会连带打开阅读器。
                    cx.stop_propagation();
                    this.model.update(cx, |model, _cx| {
                        model.toggle_book_favorite(&favorite_id);
                    });
                    cx.notify();
                }));
            let actions_id = data.id.clone();
            let on_actions: Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)> =
                Rc::new(cx.listener(move |this, _ev: &ClickEvent, window, cx| {
                    // 「…」也在卡片内部：不阻止冒泡会连带打开阅读器。
                    cx.stop_propagation();
                    this.open_actions_dialog(&actions_id, window, cx);
                }));
            grid = grid.child(BookCard::new(data, on_open, on_favorite).with_actions(on_actions));
        }

        div()
            .id("shelf-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(grid)
            .when(more, |this| {
                this.child(
                    div().w_full().flex().justify_center().pt_3().child(
                        Button::new("shelf-load-more")
                            .ghost()
                            .label(t!("Library.action_load_more"))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.visible_limit = this.visible_limit.saturating_add(BATCH);
                                cx.notify();
                            })),
                    ),
                )
            })
            .vertical_scrollbar(&self.scroll)
            .into_any_element()
    }
}

impl Render for LibraryPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.maybe_auto_scan(cx);

        let limit = self.visible_limit;
        let (
            cards,
            total,
            has_books,
            scan_error,
            scan_in_flight,
            download_dir,
            sort,
            status,
            favorite_only,
        ) = self.model.read_with(cx, |model, _cx| {
            let covers_dir = model.paths.covers_dir.clone();
            let visible = visible_books(model.library.store.books(), &model.library.query);
            let total = visible.len();
            let cards: Vec<CardData> = visible
                .iter()
                .take(limit)
                .map(|book| CardData::from_book(book, &covers_dir))
                .collect();
            let has_books = !model.library.store.books().is_empty();
            (
                cards,
                total,
                has_books,
                model.library.last_error.clone(),
                model.library.scan_in_flight,
                model.config.download.download_path.clone(),
                model.library.query.sort,
                model.library.query.status,
                model.library.query.favorite_only,
            )
        });

        let body: AnyElement = if total == 0 {
            Self::render_empty(has_books, cx)
        } else {
            self.render_grid(cards, total, cx)
        };

        v_flex()
            .size_full()
            .p_6()
            .gap_3()
            .child(
                PageHeader::new(t!("Library.page_title").into())
                    .subtitle(
                        t!(
                            "Library.subtitle",
                            count = &total.to_string(),
                            dir = &download_dir
                        )
                        .into(),
                    )
                    .action(
                        Button::new("shelf-import")
                            .primary()
                            .icon(Icon::new(IconName::Plus))
                            .label(t!("Library.action_import"))
                            .on_click(cx.listener(|this, _ev, _window, cx| {
                                this.pick_and_import(cx);
                            })),
                    )
                    .action(
                        Button::new("shelf-rescan")
                            .ghost()
                            .icon(Icon::new(IconName::Redo))
                            .label(t!("Library.action_rescan"))
                            .loading(scan_in_flight)
                            .disabled(scan_in_flight)
                            .on_click(cx.listener(|this, _ev, _window, cx| this.rescan(cx))),
                    ),
            )
            .child(self.render_filters(sort, status, favorite_only, cx))
            .when_some(scan_error, |this, error| {
                this.child(
                    div()
                        .p_3()
                        .rounded_md()
                        .bg(cx.theme().danger)
                        .text_color(cx.theme().danger_foreground)
                        .child(error),
                )
            })
            .child(body)
    }
}
