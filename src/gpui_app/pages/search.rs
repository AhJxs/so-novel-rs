//! Search 页面：关键词搜索 + 书源过滤 + 结果列表（Stage 10 落地 + 重构版）。
//!
//! 布局（参考 `library.rs` / `sources.rs` 的统一模式）：
//! - PageHeader：title + subtitle（**无** 右侧 action —— 搜索按钮已下移到工具栏）
//! - Toolbar：Input（关键词） + "书源" label + Select（书源下拉） + Button（搜索）
//! - SourceStatusBar：每个源的 status badge（搜索运行时显示）
//! - ResultList：`gpui-component::List` + `SearchDelegate`（虚拟滚动）
//!   每行 6 列：序号 / 书名+最新章节 / 作者 / 书源名称 / 选章 / 全本
//! - 分页页脚：30 条/页（用通用 `Pagination` 组件，始终渲染）
//!
//! 行为：
//! - 搜索按钮 `.disabled(keyword_empty || running)` —— 关键词空时直接灰。
//! - 搜索按钮 `.loading(running)` —— 跑搜索时显示 spinner。
//! - 选章按钮暂不实现（用户"选章节的功能先不实现"），点击弹 info toast
//!   "功能即将推出" —— 后续真要实现时把 toast 换成 DownloadRangeDialog。
//! - 全本按钮调 `model.spawn_download` 派任务，弹 success toast 通知用户。
//! - 详情功能推迟（"之后再使用其他方式实现"），行内不再有"详情"按钮。
//! - 选书源下拉：用 `SelectState<SearchableVec<SharedString>>`，
//!   第一项 value = `""`（"全部" = None），其余 `format!("rule:{id}")` 编码 source id。
//! - i18n：所有新增 UI 字符串走 `Search.*` 命名空间（locales/app.yml × 3 locale）。

use gpui::{
    div, prelude::FluentBuilder as _, px, App, AppContext, Context, Entity, IntoElement,
    ParentElement, Render, SharedString, Styled, Window,
};
use gpui_component::{
    button::Button,
    h_flex,
    input::{Input, InputEvent, InputState},
    list::{List, ListDelegate, ListItem, ListState},
    notification::{Notification, NotificationType},
    select::{SearchableVec, Select, SelectDelegate, SelectEvent, SelectItem, SelectState},
    spinner::Spinner,
    tag::Tag,
    v_flex, ActiveTheme as _, Disableable, Icon, IconName, IndexPath, Sizable, WindowExt,
};

use crate::app::{AppModel, SourceStatus};
use crate::gpui_app::components::{truncate, EmptyState, PageHeader, Pagination};
use crate::gpui_app::i18n::ts;
use crate::models::SearchResult;

/// 分页大小。跟 library.rs / sources.rs 保持一致。
const PAGE_SIZE: usize = 30;

/// 选书源下拉的自定义 item —— `value`（内部 id）跟 `title`（给用户看）分开。
///
/// 为什么需要：gpui-component 0.5.1 的内置 `SelectItem` impls（`String` / `SharedString` /
/// `&'static str`）都强制 `value() == title() == self`，无法让 value 是 `"rule:1"`、
/// title 是 `"起点 (ZH_CN)"`。手写小 struct 是最简方案。
///
/// - `value`: 内部 id —— `"all"` 表示"聚合搜索"（= `None`），`"rule:{id}"` 表示单源。
/// - `title`: 给用户看的文本 —— 聚合搜索时是 `ts("Search.source.aggregate")`；
///   单源时是 `format!("{name} ({LANG})")`。
/// - `Value` 关联类型 = `SharedString`：`Confirm(Some(value))` 拿到的还是
///   `SharedString`，解析逻辑跟旧版一致（`v == "all" → None;
///   v.strip_prefix("rule:").and_then(parse) → Some(id)`）。
#[derive(Clone, Debug)]
struct SourceSelectItem {
    value: SharedString,
    title: SharedString,
}

impl SelectItem for SourceSelectItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
    // 默认 `matches` 按 title 匹配 —— 下拉里搜"起点"或"ZH"能筛到 rule。
    // 默认 `render` 显示 title —— 弹层列表项正确。
    // 默认 `display_title` 返回 `None`，折叠态显示 `title()` —— Select 关闭时
    // 显示书源名称（不带语言后缀），行为对。
}

/// Search 页面 entity。
pub struct SearchPage {
    model: Entity<AppModel>,

    /// 关键词 Input。placeholder **必须**在 State 上（gpui-component 0.5.1 的 `Input`
    /// element 没有 `.placeholder()` 方法）—— 实时 i18n 走 `last_seen_placeholder` sentinel。
    keyword: Entity<InputState>,

    /// 实时 i18n sentinel：上次同步到 `InputState.placeholder` 的翻译。
    /// 切语言后 `ts()` 返回新值 → render 顶部检测不一致 → `set_placeholder` 刷新。
    last_seen_placeholder: SharedString,

    /// 选书源下拉 SelectState（可搜索）。
    ///
    /// items 是 `SearchableVec<SourceSelectItem>`（自定义 struct 让 `value` 跟
    /// `title` 分开），第一项 value = `"all"`（"聚合搜索" = None），其余
    /// `format!("rule:{id}")` 编码 source id。rule.name 是数据（不译），语种后缀
    /// `LANG` 也是数据，所以 SelectState 缓存 items 不会破坏 i18n。首项 title
    /// "聚合搜索" 来自 `ts("Search.source.aggregate")`，**不在** State 字段里
    /// 缓存 —— 切语言后下次展开下拉时 SelectState 重建 item 列表自动更新。
    source_state: Entity<SelectState<SearchableVec<SourceSelectItem>>>,

    /// gpui-component 虚拟列表 + 自定义 Delegate。必须在 `new()` 里建一次并缓存。
    list_state: Entity<ListState<SearchDelegate>>,

    /// 当前 0-based 页码。UI-only，每次关键词或过滤变化时重置为 0。
    current_page: usize,
}

impl SearchPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // (a) 关键词 InputState + 订阅 InputEvent::Change
        let initial_placeholder = ts("Search.filter.placeholder");
        let keyword =
            cx.new(|cx| InputState::new(window, cx).placeholder(initial_placeholder.to_string()));
        cx.subscribe_in(&keyword, window, |this, _state, ev, _w, cx| {
            match ev {
                InputEvent::Change => {
                    let v = this.keyword.read(cx).value().to_string();
                    this.model.update(cx, |m, _cx| m.search.keyword = v);
                }
                // Enter 直接触发搜索（关键词空 / 已在跑时 run_search 自身兜底）。
                InputEvent::PressEnter { .. } => this.run_search(_w, cx),
                _ => {}
            }
        })
        .detach();

        // (b) 选书源 SelectState
        //
        // 第一个 item 是"聚合搜索"（value="all" = None = 跨书源搜索），
        // 后面每个 rule 一项，title 是 "name (LANG)"。
        let aggregate_title = ts("Search.source.aggregate");
        let mut items: Vec<SourceSelectItem> = vec![SourceSelectItem {
            value: SharedString::from("all"),
            title: aggregate_title,
        }];
        items.extend(model.read(cx).rules.iter().map(|r| {
            // 名字兜底：空时显 "(no name)"，否则 truncate 到 30 字符避免长名字
            // 撑爆下拉。不带语言后缀 —— 用户只要看清是哪个书源即可。
            let name_disp = if r.name.is_empty() {
                SharedString::from("(no name)")
            } else {
                SharedString::from(truncate(&r.name, 30))
            };
            SourceSelectItem {
                value: SharedString::from(format!("rule:{}", r.id)),
                title: name_disp,
            }
        }));
        let items: SearchableVec<SourceSelectItem> = items.into();

        // 初始选中：model.search.source_id → 找对应 row；None 落到 "all"。
        let cur_value = model
            .read(cx)
            .search
            .source_id
            .map(|id| format!("rule:{id}"))
            .unwrap_or_else(|| "all".to_string());
        let cur_value = SharedString::from(cur_value);
        let selected_pos =
            <SearchableVec<SourceSelectItem> as SelectDelegate>::position(&items, &cur_value);
        let source_state =
            cx.new(|cx| SelectState::new(items, selected_pos, window, cx).searchable(true));
        cx.subscribe_in(&source_state, window, |this, _state, ev, _w, cx| {
            if let SelectEvent::Confirm(Some(value)) = ev {
                // `value` 是 `SourceSelectItem::Value = SharedString` —— 内部 id。
                // 旧版用 `v.is_empty()` 区分 "全部" vs "rule:N"；新版第一项的
                // value 改为显式 `"all"`，所以判断改成字符串等值。
                let v = value.to_string();
                let new_source_id = if v == "all" {
                    None
                } else {
                    v.strip_prefix("rule:").and_then(|s| s.parse().ok())
                };
                this.model.update(cx, |m, _cx| {
                    m.search.source_id = new_source_id;
                });
                cx.notify();
            }
        })
        .detach();

        // (c) ListState + Delegate
        let page_handle = cx.entity().clone();
        let delegate = SearchDelegate::new(page_handle);
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx));

        Self {
            model,
            keyword,
            last_seen_placeholder: initial_placeholder,
            source_state,
            list_state,
            current_page: 0,
        }
    }

    /// 点"搜索"按钮 → 把当前 keyword 同步到 model，调 `spawn_search`。
    /// 关键词空 / 已在跑 时按钮已 disabled，理论上不会进；保留 `last_error` 防御。
    fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kw = self.keyword.read(cx).value().to_string();
        self.model.update(cx, |m, _cx| m.search.keyword = kw);
        let started = self.model.update(cx, |m, _cx| m.spawn_search());
        if !started {
            window.push_notification(
                Notification::new()
                    .title(ts("Search.action.search"))
                    .message(ts("Search.empty.subtitle"))
                    .with_type(NotificationType::Warning)
                    .autohide(true),
                cx,
            );
        }
        cx.notify();
    }
}

impl Render for SearchPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // ---- 0. 实时 i18n sentinel: 刷新 keyword placeholder ----
        //
        // `set_placeholder` 内部 `cx.notify()` 只通知 InputState 重新 render。
        // 但 `Input` 元素是 SearchPage render 时构造的，SearchPage 不重 render，
        // Input 元素就不重画。这里额外 `cx.notify()` 强制 SearchPage 重 render，
        // 触发 Input 重构造 → 读取 InputState 的最新 placeholder 渲染。
        let new_placeholder = ts("Search.filter.placeholder");
        if self.last_seen_placeholder != new_placeholder {
            self.last_seen_placeholder = new_placeholder.clone();
            self.keyword.update(cx, |state, cx| {
                state.set_placeholder(new_placeholder, window, cx);
            });
            cx.notify();
        }

        let model = self.model.read(cx);
        let results = model.search.results.clone();
        let running = model.search.running;
        let expected = model.search.expected;
        let received = model.search.received;
        let source_status = model.search.source_status.clone();
        // 提前 drop model 的不可变借用，避免下面 self.list_state.update + render
        // 函数体里 borrow checker 打架。
        let _ = model;

        // ---- 1. 分页切片 + 兜底（清空 results 后 current_page 越界 → 回卷）----
        let total = results.len();
        let page_count = total.div_ceil(PAGE_SIZE);
        if page_count > 0 && self.current_page >= page_count {
            self.current_page = page_count - 1;
        }
        let start = self.current_page * PAGE_SIZE;
        let end = (start + PAGE_SIZE).min(total);
        let page_items: Vec<(usize, SearchResult)> = if start < end {
            results[start..end]
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, r)| (start + i, r))
                .collect()
        } else {
            Vec::new()
        };
        // 推给 delegate，List 渲染时读到。
        self.list_state.update(cx, |state, _cx| {
            state.delegate_mut().page_items = page_items;
        });

        // ---- 2. 搜索按钮是否禁用 ----
        let keyword_empty = self.keyword.read(cx).value().is_empty();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            // ---- 3. PageHeader (无 action) ----
            .child(PageHeader::new(ts("Search.page_title")).subtitle(ts("Search.page_subtitle")))
            // ---- 4. 工具栏: 关键词 Input + 书源 Select + 搜索 Button ----
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        Input::new(&self.keyword).w(px(320.0)).prefix(
                            Icon::new(IconName::Search)
                                .small()
                                .text_color(cx.theme().muted_foreground),
                        ),
                    )
                    .child(
                        // 书源下拉："书源" label + Select。
                        // Select 显示当前选中项的 title（聚合搜索 / 书源名称）。
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(ts("Search.source.label")),
                            )
                            .child(Select::new(&self.source_state).w(px(200.0))),
                    )
                    .child(
                        Button::new("search-go")
                            .icon(Icon::new(IconName::Search))
                            .label(ts("Search.action.search"))
                            .loading(running)
                            // 关键词空 OR 正在跑时禁用 —— 跟加载状态绑定
                            .disabled(keyword_empty || running)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run_search(window, cx);
                            })),
                    ),
            )
            // ---- 5. 源状态行（保留：搜索运行时显示每个源的 status）----
            .when(!source_status.is_empty(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .flex_wrap()
                        .children(source_status.iter().map(|(_, name, status)| {
                            // 源状态：name + 状态文案全部塞进一个 Tag（语义色），
                            // 跟 sources.rs 统计行同款。
                            // Neutral→secondary、Success→success、Error→danger。
                            match status {
                                SourceStatus::Pending => Tag::secondary().outline().child(format!(
                                    "{name} {}",
                                    ts("Search.source_status.pending")
                                )),
                                SourceStatus::Ok(n) => Tag::success().outline().child(format!(
                                    "{name} {} {}",
                                    n,
                                    ts("Search.source_status.format")
                                )),
                                SourceStatus::Err(_) => {
                                    Tag::danger().outline().child(format!("{name}"))
                                }
                            }
                        }))
                        .when(running, |this| {
                            this.child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(Spinner::new().small())
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(" {received}/{expected}")),
                                    ),
                            )
                        }),
                )
            })
            // ---- 6. 结果列表 / 空态 ----
            .child(if total == 0 {
                // 空态：跟 library / sources 一致的 EmptyState（图标 + title + subtitle）。
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        EmptyState::new(IconName::Search, ts("Search.empty.title"))
                            .subtitle(ts("Search.empty.subtitle")),
                    )
                    .into_any_element()
            } else {
                // 列表容器：边框 + List 整体 padding(12px)，让选中边框不被滚动条遮挡。
                // 参考 library.rs / sources.rs 的 List 容器样式。
                div()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_md()
                    .child(List::new(&self.list_state).p(px(12.)).size_full())
                    .into_any_element()
            })
            // ---- 7. 分页页脚（始终渲染 —— ≤1 页时 prev/next disabled，单一数字按钮）----
            .child(Pagination::new(
                self.current_page,
                page_count,
                cx.listener(|this, &new_page, _window, _cx| {
                    this.current_page = new_page;
                    _cx.notify();
                }),
            ))
    }
}

// ============================================================================
// Delegate + row renderer
// ============================================================================

/// `gpui-component::List` 的 delegate —— 把当前页的 `SearchResult` 切片渲染成行。
///
/// 完全对齐 `sources.rs::SourcesDelegate` 模式：
/// - `page_items` 由 `SearchPage::render` 在每帧 render 前写入；`render_item` 直接
///   从这个 Vec 里取，不重新计算过滤 / 分页。
/// - 持有 `Entity<SearchPage>` handle 以便 row 的全本按钮 → `SearchPage::run_search`
///   等转发到 page（**当前**全本按钮直接调 `model.spawn_download`，不经过 page，
///   但保留 handle 留作未来扩展）。
/// - 选中态完全交给 `ListItem::selected(...)` + `set_selected_index` 配对管理。
struct SearchDelegate {
    /// 当前页要展示的条目，每条带"全局序号"（在完整 results 列表里的 0-based 位置）。
    /// 跨分页连续：page 0 → 0..29，page 1 → 30..59，等等。显示时 +1 变 1-based。
    page_items: Vec<(usize, SearchResult)>,
    /// 当前选中项。`None` = 未选中。`set_selected_index` 写入，
    /// `render_item` 读出来给 `ListItem::selected(...)` 用。
    selected_index: Option<IndexPath>,
    /// 拿 SearchPage handle 用于按钮 on_click → 转发回 page（预留扩展）。
    #[allow(dead_code)]
    page: Entity<SearchPage>,
}

impl SearchDelegate {
    fn new(page: Entity<SearchPage>) -> Self {
        Self {
            page_items: Vec::new(),
            selected_index: None,
            page,
        }
    }
}

impl ListDelegate for SearchDelegate {
    type Item = ListItem;

    fn items_count(&self, _section: usize, _cx: &App) -> usize {
        self.page_items.len()
    }

    fn render_item(
        &mut self,
        ix: IndexPath,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<Self::Item> {
        let (global_index, result) = self.page_items.get(ix.row)?.clone();
        let page = self.page.clone();
        Some(
            ListItem::new(ix)
                .selected(Some(ix) == self.selected_index)
                .rounded(cx.theme().radius)
                .mb(px(4.))
                .child(render_result_row(global_index, &result, page, &mut *cx)),
        )
    }

    fn set_selected_index(
        &mut self,
        ix: Option<IndexPath>,
        _window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        self.selected_index = ix;
        cx.notify();
    }
}

/// 渲染一条搜索结果行（6 列：序号 / 书名 / 元信息 / 源 id / 选章 / 全本）。
///
/// 跟 library.rs::render_row 同模式：固定宽度列 + flex_1 撑满剩余 + ListItem 内部选中样式。
/// 删除按钮（这里没有 —— 搜索结果行内不放删除）。
///
/// `page: Entity<SearchPage>` 转发用：全本按钮 on_click 调 `page.update` 拿
/// `&mut AppModel`（`spawn_download` 需要 `&mut self`）。
fn render_result_row(
    idx: usize,
    r: &SearchResult,
    page: Entity<SearchPage>,
    cx: &mut App,
) -> impl IntoElement {
    let name = truncate(&r.book_name, 50);
    let author_display = r
        .author
        .clone()
        .unwrap_or_else(|| ts("Search.result.unknown_author").to_string());
    let latest_display = r
        .latest_chapter
        .clone()
        .unwrap_or_else(|| ts("Search.result.no_latest").to_string());
    // 书源名称：直接用结果自带的 source_name（不再显示 #id）。
    let source_name_display = if r.source_name.is_empty() {
        ts("Search.result.unknown_source").to_string()
    } else {
        truncate(&r.source_name, 20).to_string()
    };

    // 全本按钮要 clone result 进闭包（on_click 是 Fn，可多次触发）—— 在闭包
    // 里每次重新 clone。
    let result_for_whole = r.clone();
    let page_for_whole = page.clone();

    h_flex()
        // 不要 .id(...)：外层 ListItem::new(ix) 已经给了 id，自己再加会和 List 的
        // 虚拟滚动 hit-test 冲突。
        // 不要 .hover / .border_b_1：ListItem paint 逻辑已经根据 selected / hover
        // 画 list_hover / list_active / list_active_border 三套样式。
        .px_2()
        .py_2()
        .gap_2()
        .rounded(cx.theme().radius)
        .items_center()
        // ---- 序号列（跨分页连续：global_index 是 0-based 全局位置，+1 给用户看）----
        // 48px 装 "#100" 这种 4 字符号。右对齐 + muted 颜色，跟 library / sources 一致。
        .child(
            div()
                .w(px(48.0))
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("#{}", idx + 1)),
        )
        // ---- 书名列（flex_1）：书名在上，最新章节在下 ----
        .child(
            v_flex()
                .flex_1()
                .min_w(px(160.))
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(cx.theme().foreground)
                        .child(div().whitespace_nowrap().text_ellipsis().child(name)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(
                            div()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(latest_display),
                        ),
                ),
        )
        // ---- 作者列（固定宽度，超出省略）----
        .child(
            div()
                .w(px(140.0))
                .overflow_x_hidden()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(author_display),
                ),
        )
        // ---- 书源名称列（固定宽度，超出省略）----
        .child(
            div()
                .w(px(120.0))
                .overflow_x_hidden()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(div().whitespace_nowrap().text_ellipsis().child(source_name_display)),
        )
        // ---- 选章按钮（no-op toast —— 功能即将推出）----
        .child(
            Button::new(("search-chapters", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::ChevronRight))
                .label(ts("Search.action.select_chapters"))
                .on_click(move |_, window, cx| {
                    // 弹 info toast 告知用户功能未实现。给个反馈比 silent no-op 体验好，
                    // 后续真要实现时把这里换成 DownloadRangeDialog 即可。
                    window.push_notification(
                        Notification::new()
                            .title(ts("Search.action.select_chapters"))
                            .message(ts("Search.action.select_chapters_unavailable"))
                            .with_type(NotificationType::Info)
                            .autohide(true),
                        cx,
                    );
                }),
        )
        // ---- 全本按钮（spawn download + success toast）----
        .child(
            Button::new(("search-whole", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::BookOpen))
                .label(ts("Search.action.download_whole"))
                .on_click(move |_, window, cx| {
                    // on_click 是 Fn（点击可多次触发），所以 on_click 内不能 move
                    // result_for_whole 多次 — 每次重新 clone 一份。
                    let result_for_click = result_for_whole.clone();
                    // spawn_download 走 AppModel — 通过 page.update 转发。
                    // page 是 Entity<SearchPage>，update 后拿到 &mut SearchPage，再
                    // update model 拿 &mut AppModel。
                    let new_id = page_for_whole.update(cx, |p, cx| {
                        p.model
                            .update(cx, |m, _cx| m.spawn_download(result_for_click))
                    });
                    window.push_notification(
                        Notification::new()
                            .title(ts("Search.action.download_started"))
                            .message(format!("#{new_id}"))
                            .with_type(NotificationType::Success)
                            .autohide(true),
                        cx,
                    );
                }),
        )
}
