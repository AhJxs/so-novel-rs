//! 书架页：封面卡片网格 + 搜索 / 状态筛选 / 收藏开关 / 排序 / 导入 / 重新扫描。
//!
//! - 数据全部来自 `AppModel.library.store`；`render_cache` 以
//!   `(books_version, query, covers_dir)` 为键缓存可见卡片，数据没变就不重跑
//!   `visible_books` 的过滤排序与 `CardData::from_book`（含 `is_file()` stat）；
//! - 卡片点击 → `RootView::open_reader`，通过 `set_open_reader_handler` 注入的回调，
//!   页面不持有根视图（避免循环强引用）；
//! - 扫描 / 元数据提取都在后台跑，drain 循环回传后窗口自动重绘。

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Icon, IconName, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement, Point, Render, ScrollHandle,
    StatefulInteractiveElement as _, Styled, Window, div, px,
};
use rust_i18n::t;

use super::card::{BookCard, CardData, MenuBuilder};
use crate::desktop::components::{EmptyState, PageHeader};
use crate::desktop::model::AppModel;
use crate::library::{ShelfQuery, ShelfSort, ShelfStatusFilter, absolute_dir, visible_books};
use crate::utils::system;

/// 书架卡片点击 → 打开阅读器。由 `RootView` 注入。
pub type OpenReaderFn = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// 每批渲染的卡片数；「加载更多」每次追加一批。
const BATCH: usize = 60;

/// 书架页渲染缓存：`(books_version, query, covers_dir, cards, total)`。
type RenderCache = (u64, ShelfQuery, Option<PathBuf>, Rc<Vec<CardData>>, usize);

/// 书架页面 entity。
pub struct LibraryPage {
    /// `dialogs` 子模块也要读写它。
    pub(super) model: Entity<AppModel>,
    /// 搜索框 state 必须由页面持有，否则每次重绘丢焦点与光标。
    search_input: Entity<InputState>,
    scroll: ScrollHandle,
    /// 当前渲染的卡片上限。
    visible_limit: usize,
    /// `(books_version, query, covers_dir, cards, total)`：键没变就复用上一帧的卡片。
    ///
    /// 缓存里是**全部**可见书的卡片；`visible_limit` 在渲染时才裁剪，
    /// 所以「加载更多」不会重建缓存。过滤排序与 `is_file()` 磁盘 stat 只在键变化时发生。
    render_cache: Option<RenderCache>,
    /// 打开阅读器的回调（`dialogs` 的「继续阅读」也要用）。
    pub(super) open_reader: Option<OpenReaderFn>,
    /// 网格键盘导航的焦点句柄：整块网格是一个 Tab 停靠点。
    focus: FocusHandle,
    /// 键盘聚焦的卡片在**可见列表**中的索引；`None` 表示还没用方向键选过。
    focused_index: Option<usize>,
    /// 布局期实测的列数（`on_children_prepainted` 写入）：上 / 下键按列数跳。
    grid_columns: Rc<Cell<usize>>,
}

impl LibraryPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 网格整体是一个 Tab 停靠点，方向键在卡片间移动（见 `on_grid_key_down`）。
        let focus = cx.focus_handle().tab_stop(true);

        // 后台扫描 / 元数据 / 阅读进度都会改 `AppModel`：观察到就重绘，
        // 不依赖 gpui 的「渲染期读过该 entity」隐式失效。
        // 自动扫描也挂在这里：只有模型变化（下载目录设置、扫描哨兵、工具产出）时才比较，
        // 渲染路径不再承担副作用。
        cx.observe(&model, |this, _model, cx| {
            this.maybe_auto_scan(cx);
            cx.notify();
        })
        .detach();

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

        let this = Self {
            model,
            search_input,
            scroll: ScrollHandle::new(),
            visible_limit: BATCH,
            render_cache: None,
            open_reader: None,
            focus,
            focused_index: None,
            grid_columns: Rc::new(Cell::new(1)),
        };
        // 构造期扫一次：之后由上面的 observe 在模型变化时比较。
        this.maybe_auto_scan(cx);
        this
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
        // 列表顺序变了，旧的键盘索引会指向另一本书：直接清掉，下次从第一张开始。
        self.focused_index = None;
        self.scroll.set_offset(Point::default());
        cx.notify();
    }

    /// 首次进入 / 下载目录变化时自动扫一次（同一目录只扫一次，靠 `scanned_dir` 去重）。
    ///
    /// 只在模型变化 / 构造期调用，不再是渲染期副作用；比较本身只读模型，不碰磁盘。
    fn maybe_auto_scan(&self, cx: &mut Context<Self>) {
        let (wanted, scanned, in_flight) = self.model.read_with(cx, |model, _cx| {
            (
                absolute_dir(std::path::Path::new(&model.config.download.download_path)),
                model.library.scanned_dir.clone(),
                model.library.scan_in_flight,
            )
        });
        // 扫描在跑时不必再排队：完成后的模型通知会再比较一次。
        if in_flight || scanned.as_ref() == Some(&wanted) {
            return;
        }
        self.model
            .update(cx, |model, _cx| model.refresh_library_async());
    }

    /// 网格键盘导航：方向键在卡片间移动，Enter / Space 打开当前卡片。
    ///
    /// 列数取布局期实测值，窗口宽度或字号变化后自动更新；还没测到时按 1 列兜底。
    /// 只处理无修饰键，避免和输入框 / 系统快捷键冲突。
    fn on_grid_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = &event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        // 只有网格本身持有焦点时才处理：卡片内的收藏星标是独立 Button，
        // 在它上面按 Enter / Space 只应切换收藏，不能冒泡成「打开这本书」。
        if !self.focus.is_focused(window) {
            return;
        }

        let total = self.model.read_with(cx, |model, _cx| {
            visible_books(model.library.store.books(), &model.library.query).len()
        });
        if total == 0 {
            return;
        }

        // 索引可能因过滤结果变短而越界，取当前位置时先夹紧。
        let current = self.focused_index.unwrap_or(0).min(total - 1);
        let step = self.grid_columns.get().max(1);
        let next = match event.keystroke.key.as_str() {
            "left" => current.saturating_sub(1),
            "right" => (current + 1).min(total - 1),
            "up" => current.saturating_sub(step),
            "down" => (current + step).min(total - 1),
            "home" => 0,
            "end" => total - 1,
            "enter" | "space" => {
                // 卡片缓存与本帧列表一致；越界说明缓存还没建好，忽略这次激活。
                let book_id = self
                    .render_cache
                    .as_ref()
                    .and_then(|(_, _, _, cards, _)| cards.get(current).map(|data| data.id.clone()));
                if let Some(book_id) = book_id {
                    self.open_book(&book_id, window, cx);
                }
                cx.stop_propagation();
                return;
            }
            _ => return,
        };

        self.focused_index = Some(next);
        // 键盘移到还没渲染的批次时顺手放宽上限，用户不必再去点「加载更多」。
        if next >= self.visible_limit {
            self.visible_limit = ((next / BATCH) + 1) * BATCH;
        }
        cx.stop_propagation();
        cx.notify();
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

    /// 空态：整库为空 → 「扫描下载目录 + 导入本地文件」；筛选无结果 → 「导入 + 清除筛选」。
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
            .px_6()
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

    /// 单本书的右键菜单：继续阅读 / 从头开始 / 书籍信息 / 在文件夹中显示 /
    /// 用默认程序打开 / 从书架移除 / 删除文件。
    ///
    /// 没有标题行（书名就在卡片上），也不再有「编辑信息」入口。
    fn book_menu(data: &CardData, cx: &Context<Self>) -> MenuBuilder {
        let page = cx.entity().downgrade();
        let id = data.id.clone();
        let missing = data.missing;
        let readable = data.readable;
        let finished = data.finished;
        let path = PathBuf::from(&data.path);
        Rc::new(move |mut menu, _window, _cx| {
            menu = menu.min_w(px(180.0));

            // 「继续阅读」/「重新定位」共用同一个入口：文件缺失时改走重新定位。
            let continue_label = if missing {
                t!("Library.actions.relink")
            } else {
                t!("Library.actions.continue")
            }
            .to_string();
            let continue_page = page.clone();
            let continue_id = id.clone();
            menu = menu.item(
                PopupMenuItem::new(continue_label)
                    .icon(Icon::new(IconName::BookOpen))
                    .disabled(!missing && !readable)
                    .on_click(move |_event, window, cx| {
                        let id = continue_id.clone();
                        let _ = continue_page.update(cx, |page, cx| {
                            if missing {
                                page.pick_relink(&id, cx);
                            } else {
                                page.open_book(&id, window, cx);
                            }
                        });
                    }),
            );

            if finished && !missing {
                let restart_page = page.clone();
                let restart_id = id.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!("Library.actions.restart").to_string())
                        .icon(Icon::new(IconName::Redo))
                        .on_click(move |_event, window, cx| {
                            let id = restart_id.clone();
                            let _ = restart_page.update(cx, |page, cx| {
                                page.model.update(cx, |model, _cx| model.restart_book(&id));
                                page.open_book(&id, window, cx);
                            });
                        }),
                );
            }

            let info_page = page.clone();
            let info_id = id.clone();
            menu = menu.item(
                PopupMenuItem::new(t!("Library.actions.info").to_string())
                    .icon(Icon::new(IconName::FileText))
                    .on_click(move |_event, window, cx| {
                        let id = info_id.clone();
                        let _ =
                            info_page.update(cx, |page, cx| page.open_info_dialog(&id, window, cx));
                    }),
            );

            let reveal_page = page.clone();
            let reveal_path = path.clone();
            menu = menu.item(
                PopupMenuItem::new(t!("Library.actions.reveal").to_string())
                    .icon(Icon::new(IconName::FolderOpen))
                    .on_click(move |_event, _window, cx| {
                        let path = reveal_path.clone();
                        let _ = reveal_page.update(cx, |page, cx| {
                            if let Err(error) = system::reveal_in_folder(&path) {
                                page.model
                                    .update(cx, |model, _cx| model.push_error(error.to_string()));
                            }
                        });
                    }),
            );

            let open_page = page.clone();
            let open_path = path.clone();
            menu = menu.item(
                PopupMenuItem::new(t!("Library.actions.open_external").to_string())
                    .icon(Icon::new(IconName::ExternalLink))
                    .on_click(move |_event, _window, cx| {
                        let path = open_path.clone();
                        let _ = open_page.update(cx, |page, cx| {
                            if let Err(error) = system::open_path(&path) {
                                page.model
                                    .update(cx, |model, _cx| model.push_error(error.to_string()));
                            }
                        });
                    }),
            );

            let hide_page = page.clone();
            let hide_id = id.clone();
            menu = menu.separator().item(
                PopupMenuItem::new(t!("Library.actions.hide").to_string())
                    .icon(Icon::new(IconName::Minus))
                    .on_click(move |_event, window, cx| {
                        let id = hide_id.clone();
                        let _ = hide_page.update(cx, |page, cx| page.hide_book(&id, window, cx));
                    }),
            );

            let delete_page = page.clone();
            let delete_id = id.clone();
            menu.item(
                PopupMenuItem::new(t!("Library.actions.delete_file").to_string())
                    .icon(Icon::new(IconName::Delete))
                    .on_click(move |_event, window, cx| {
                        let id = delete_id.clone();
                        let _ = delete_page.update(cx, |page, cx| {
                            page.prompt_delete_file(&id, window, cx);
                        });
                    }),
            )
        })
    }

    /// 卡片网格 + 「加载更多」；整块纵向滚动。
    ///
    /// `cards` 是本帧要画的那一段（已按 `visible_limit` 裁剪），从缓存借用，
    /// 只有每个可见项的 `CardData` 复制一份交给 `BookCard`（元素不能借用）。
    fn render_grid(
        &self,
        cards: &[CardData],
        total: usize,
        grid_focused: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let more = total > cards.len();
        // Tab 刚进入网格、还没按方向键时，把第一张当作当前项，焦点环立即可见。
        let focused_index =
            grid_focused.then(|| self.focused_index.unwrap_or(0).min(total.saturating_sub(1)));
        let columns_cell = Rc::clone(&self.grid_columns);
        let mut grid = div()
            .flex()
            .flex_wrap()
            .gap_5()
            .w_full()
            // 内边距挂在滚动内容上，滚动条才贴窗口边缘（与页头的 `px_6` 对齐）。
            .px_6()
            .pb_6()
            .track_focus(&self.focus)
            .key_context("LibraryGrid")
            .on_key_down(cx.listener(Self::on_grid_key_down))
            .on_children_prepainted(move |bounds, _window, _cx| {
                // 同一行卡片的 top 相同：数出首行数量就是当前列数。
                // 每次布局都重测，窗口宽度 / 字号变化后方向键仍然按实际列数跳。
                if let Some(first) = bounds.first() {
                    let columns = bounds
                        .iter()
                        .filter(|bounds| bounds.origin.y == first.origin.y)
                        .count();
                    columns_cell.set(columns.max(1));
                }
            });
        for (index, data) in cards.iter().enumerate() {
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
            let menu = Self::book_menu(data, cx);
            grid = grid.child(
                BookCard::new(data.clone(), on_open, on_favorite)
                    .focused(focused_index == Some(index))
                    .with_context_menu(menu),
            );
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
                    div().w_full().flex().justify_center().py_3().child(
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 自动扫描已在构造期与 `cx.observe(&model, ..)` 里触发，渲染路径保持纯声明。
        let limit = self.visible_limit;
        let (
            version,
            query,
            covers,
            has_books,
            scan_error,
            scan_in_flight,
            download_dir,
            sort,
            status,
            favorite_only,
        ) = self.model.read_with(cx, |model, _cx| {
            (
                model.library.books_version,
                model.library.query.clone(),
                model.covers_dir(),
                !model.library.store.books().is_empty(),
                model.library.last_error.clone(),
                model.library.scan_in_flight,
                model.config.download.download_path.clone(),
                model.library.query.sort,
                model.library.query.status,
                model.library.query.favorite_only,
            )
        });

        // 数据没变（版本号 / 查询 / 封面目录都相同）就复用上一帧的卡片：
        // 过滤排序和 `CardData::from_book` 里的 `is_file()` stat 都不再每帧发生。
        let cached = self.render_cache.as_ref().and_then(
            |(cached_version, cached_query, cached_covers, cards, total)| {
                (*cached_version == version && *cached_query == query && cached_covers == &covers)
                    .then(|| (Rc::clone(cards), *total))
            },
        );
        let (cards, total) = if let Some(hit) = cached {
            hit
        } else {
            let (cards, total) = self.model.read_with(cx, |model, _cx| {
                let visible = visible_books(model.library.store.books(), &model.library.query);
                let total = visible.len();
                let cards: Vec<CardData> = visible
                    .iter()
                    .map(|book| CardData::from_book(book, covers.as_deref()))
                    .collect();
                (Rc::new(cards), total)
            });
            self.render_cache = Some((version, query, covers, Rc::clone(&cards), total));
            (cards, total)
        };

        let body: AnyElement = if total == 0 {
            Self::render_empty(has_books, cx)
        } else {
            let shown = &cards[..limit.min(cards.len())];
            let grid_focused = self.focus.is_focused(window);
            self.render_grid(shown, total, grid_focused, cx)
        };

        v_flex()
            .size_full()
            .gap_3()
            // 页头留在滚动区外，内边距自己带；滚动区占满剩余宽度，
            // 滚动条才能贴到窗口右边缘（挂在 `p_6` 里面会内缩 24px）。
            .child(
                v_flex()
                    .px_6()
                    .pt_6()
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
                                    .on_click(cx.listener(|this, _ev, _window, cx| {
                                        this.rescan(cx);
                                    })),
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
                    }),
            )
            .child(body)
    }
}
