//! Search 页面：关键词搜索 + 书源过滤 + 结果列表 + 选章下载 Dialog。
//!
//! 子模块（跟 library / sources / tasks 同款骨架）：
//! - `source_select` — 选书源下拉的 `SelectItem`。
//! - `toolbar` — Input + Select + Button + 源状态行。
//! - `result_row` — 结果行（6 列：序号 / 书名 / 作者 / 源 / 详情 / 选章 / 全本）。
//! - `detail_dialog` — 详情 Dialog body + 封面解码 / 渲染。
//! - `range_dialog` — 选章 Dialog body + 起止输入框 clamp helper。
//! - `delegate` — `SearchDelegate` + `ListDelegate` impl（虚拟滚动）。
//!
//! 布局：PageHeader（无 action）→ Toolbar → `SourceStatusBar` → `ResultList` → Pagination。

use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;

/// 详情面板"已解码封面"缓存最大条目数。
///
/// 32 条够当前详情面板浏览；超额 LRU 自动驱逐最久未访问。
const COVER_IMAGES_CAPACITY: NonZeroUsize = match NonZeroUsize::new(32) {
    Some(n) => n,
    None => unreachable!(),
};

use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, RenderImage,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, WindowExt,
    dialog::{Dialog, DialogButtonProps},
    input::{InputEvent, InputState, NumberInputEvent, StepAction},
    list::{List, ListState},
    notification::{Notification, NotificationType},
    select::{SearchableVec, SelectDelegate, SelectEvent, SelectState},
    v_flex,
};

use crate::desktop::model::{AppModel, TocState};
use crate::desktop::components::{
    EmptyState, PageHeader, Pagination, compute_page_window, truncate,
};
use crate::i18n::ts;
use crate::models::SearchResult;

use self::delegate::SearchDelegate;
use range_dialog::clamp_range_value;
use source_select::SourceSelectItem;

mod delegate;
mod detail_dialog;
mod range_dialog;
mod result_row;
mod source_select;
mod toolbar;

/// Search 页面 entity。
pub struct SearchPage {
    model: Entity<AppModel>,

    /// struct 字段持有（InputState / `SelectState` / `ListState`）—— owner 持有避免
    /// click / focus 丢失。placeholder 在 `new()` 一次性设好，language setter 走
    /// "重启进程"路径，新进程重建时自然拿新 locale。
    keyword: Entity<InputState>,
    source_state: Entity<SelectState<SearchableVec<SourceSelectItem>>>,
    list_state: Entity<ListState<SearchDelegate>>,

    /// UI-only，每次关键词或过滤变化时重置为 0。
    current_page: usize,

    /// 书源下拉 items 的上一次快照（值为 "all" / "rule:{id}"），用来 render 差量
    /// 检测。SourcesPage 改禁用 / 删除 / 重命名书源后 `model.rules` 变化但 `SelectState`
    /// 不会自动重读 —— render 检测到快照不一致就重建 items 并 `set_items` / 调整选中。
    /// 与 `SettingsPage::sync_theme_items` 同套路（observer 拿不到 Window，差量
    /// 更新走 render）。
    last_source_items: Vec<SharedString>,

    /// 封面解码缓存：`cover://` URI → 解码后的 `RenderImage`。
    ///
    /// `CoverEntry` 只存原始字节（UI 中立，见 `app/cover.rs`），解码放 UI 层。
    /// 按 `uri`（稳定去重 key）缓存 `Arc<RenderImage>` —— Dialog 每帧重渲时
    /// 避免重复解码 + 重传纹理（`RenderImage::new` 每次新 id，不缓存让 gpui 每帧
    /// 重传）。`None` = 解码失败，缓存负面结果避免反复重试。
    ///
    /// **LRU 上限 `COVER_IMAGES_CAPACITY`（32）**：旧 `HashMap` 无界，长会话
    /// 累积所有查看过的封面（`Arc<RenderImage>` 含完整像素）。32 条足够覆盖
    /// 当前详情面板的常规浏览；超额自动驱逐最久未访问。
    cover_images: LruCache<String, Option<Arc<RenderImage>>>,

    /// 选章下载 Dialog 的状态（起止输入框 + 当前 target + 初始化标志）。
    ///
    /// 流程：点"选章" → `spawn_resolve_toc` 拉章节列表（写 `toc_cache`）→ 弹 confirm
    /// Dialog 反应式读 `toc_cache，TOC` 回来后初始化起止输入框（1 / N）+ 显示预览。
    /// 用户改输入框 / 按 +/- → 本页订阅 `InputEvent::Change` + `NumberInputEvent::Step`，
    /// clamp 到 [1, N] 后 `set_value` 写回。
    range_start_input: Entity<InputState>,
    range_end_input: Entity<InputState>,
    /// Dialog 当前为哪条搜索结果服务（None = 没开）。点击不同结果时更新；
    /// TOC 用 `(source_id, url)` 在 `toc_cache` 里查。
    range_target: Option<SearchResult>,
    /// 是否已为当前 target `初始化过输入框（set_value` 1 / N）。
    /// 防 TOC 每帧重渲时反复 `set_value` 覆盖用户输入。
    range_initialized: bool,
}

impl SearchPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let keyword = cx.new(|cx| {
            InputState::new(window, cx).placeholder(ts("Search.filter.placeholder").to_string())
        });
        cx.subscribe_in(&keyword, window, |this, _state, ev, w, cx| {
            match ev {
                InputEvent::Change => {
                    let v = this.keyword.read(cx).value().to_string();
                    this.model.update(cx, |m, _cx| m.search.keyword = v);
                }
                // Enter 直接触发搜索（关键词空 / 已在跑时 run_search 自身兜底）。
                InputEvent::PressEnter { .. } => this.run_search(w, cx),
                _ => {}
            }
        })
        .detach();

        // 选书源 SelectState。items 首次为空：render 第一次跑时 `sync_source_items`
        // 会从 `model.rules` 重建并 set_items。这条路径处理 SourcesPage 改禁用 / 删除
        // / 重命名后下拉不刷新的问题 —— observer 拿不到 Window，差量更新走 render，
        // 与 `SettingsPage::sync_theme_items` 同套路。
        let items: SearchableVec<SourceSelectItem> = Vec::<SourceSelectItem>::new().into();
        let source_state = cx.new(|cx| SelectState::new(items, None, window, cx).searchable(true));
        cx.subscribe_in(&source_state, window, |this, _state, ev, _w, cx| {
            if let SelectEvent::Confirm(Some(value)) = ev {
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

        let page_handle = cx.entity();
        let delegate = SearchDelegate::new(page_handle);
        let list_state = cx.new(|cx| ListState::new(delegate, window, cx));

        // 选章 Dialog 的起止输入框。两个 InputState 各绑一个 NumberInput。
        // 订阅两类事件：
        // - `InputEvent::Change`：用户键入数字 → clamp 后 set_value 写回 + 刷新预览。
        // - `NumberInputEvent::Step`：用户按 +/- → ±1 后 set_value。
        //   NumberInput 的 +/- 只发 Step 事件、不改值（见 gpui-component number_input.rs
        //   L106-112），必须自己处理。
        // clamp 范围 [1, N]：N 取 toc_cache Loaded 章节数；TOC 没回来按 [1, u32::MAX]。
        let range_start_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("1".to_string()));
        let range_end_input = cx.new(|cx| InputState::new(window, cx).placeholder("1".to_string()));

        // Change 订阅：只在值不同时 set_value —— 无条件写回触发 Change→set_value→
        // Change 死循环，几轮把 Windows 句柄配额耗尽崩溃（0x80070718）。set_value 写回
        // 的值已是规整值，二次 Change want==cur 直接跳过，循环终止。
        // set_value 要 `&mut Window`（0.5.1 三参签名），用回调自带的 window —— update
        // 只借 cx，window 是独立可变借用，不冲突。
        cx.subscribe_in(
            &range_start_input,
            window,
            |this, _state, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Change) {
                    let cur = this.range_start_input.read(cx).value().to_string();
                    let v = clamp_range_value(this, &cur.clone().into(), cx);
                    if v == 0 {
                        // 空字符串 → 用户正在清空输入框，不要重置。
                        return;
                    }
                    let want = v.to_string();
                    if want != cur {
                        this.range_start_input
                            .update(cx, |s, cx| s.set_value(want, window, cx));
                        cx.notify();
                    }
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &range_end_input,
            window,
            |this, _state, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Change) {
                    let cur = this.range_end_input.read(cx).value().to_string();
                    let v = clamp_range_value(this, &cur.clone().into(), cx);
                    if v == 0 {
                        // 空字符串 → 用户正在清空输入框，不要重置。
                        return;
                    }
                    let want = v.to_string();
                    if want != cur {
                        this.range_end_input
                            .update(cx, |s, cx| s.set_value(want, window, cx));
                        cx.notify();
                    }
                }
            },
        )
        .detach();

        // Step 订阅（+/-）。
        cx.subscribe_in(
            &range_end_input,
            window,
            |this, _state, ev: &NumberInputEvent, window, cx| {
                let NumberInputEvent::Step(action) = ev;
                let cur = clamp_range_value(this, &this.range_start_input.read(cx).value(), cx);
                let v = match action {
                    StepAction::Decrement => cur.saturating_sub(1).max(1),
                    StepAction::Increment => cur.saturating_add(1),
                };
                let n = this
                    .current_range_chapters_len(cx)
                    .unwrap_or(u32::MAX)
                    .max(1);
                this.range_start_input
                    .update(cx, |s, cx| s.set_value(v.min(n).to_string(), window, cx));
                cx.notify();
            },
        )
        .detach();
        cx.subscribe_in(
            &range_end_input,
            window,
            |this, _state, ev: &NumberInputEvent, window, cx| {
                let NumberInputEvent::Step(action) = ev;
                let cur = clamp_range_value(this, &this.range_end_input.read(cx).value(), cx);
                let v = match action {
                    StepAction::Decrement => cur.saturating_sub(1).max(1),
                    StepAction::Increment => cur.saturating_add(1),
                };
                let n = this
                    .current_range_chapters_len(cx)
                    .unwrap_or(u32::MAX)
                    .max(1);
                this.range_end_input
                    .update(cx, |s, cx| s.set_value(v.min(n).to_string(), window, cx));
                cx.notify();
            },
        )
        .detach();

        Self {
            model,
            keyword,
            source_state,
            list_state,
            current_page: 0,
            last_source_items: Vec::new(),
            cover_images: LruCache::new(COVER_IMAGES_CAPACITY),
            range_start_input,
            range_end_input,
            range_target: None,
            range_initialized: false,
        }
    }

    /// 点"搜索"按钮 → 把当前 keyword 同步到 model，调 `spawn_search`。
    /// 关键词空 / 已在跑 时按钮已 disabled，理论上不会进；保留 `last_error` 防御。
    fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let kw = self.keyword.read(cx).value().to_string();
        self.model.update(cx, |m, _cx| m.search.keyword = kw);
        self.current_page = 0;
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

    /// 当前选章 Dialog 的 target 在 `toc_cache` 里 Loaded 的章节数。未拉到 / 失败 → `None`。
    fn current_range_chapters_len(&self, cx: &App) -> Option<u32> {
        let t = self.range_target.as_ref()?;
        let key = (t.source_id, t.url.clone());
        match self.model.read(cx).search.toc_cache.get(&key) {
            Some(TocState::Loaded(_, chs)) => Some(chs.len() as u32),
            _ => None,
        }
    }

    /// 书源下拉 items 差量同步。
    ///
    /// 每次 render 拍一次快照（仅保留 `value` 字符串 = "all" / "rule:{id}"），与
    /// `last_source_items` 对比；无变化 → 0 开销返回；变化 → 重建 items、
    /// 按当前 `model.search.source_id` 重新计算选中位置、`set_items` + `set_selected_index`
    /// 推到 `SelectState`。
    ///
    /// 覆盖的触发场景：
    /// - `SourcesPage` 切换某条规则的 `disabled`
    /// - `SourcesPage` 删除 / 导入一条规则
    /// - 规则重命名（item.title 变了）
    ///
    /// 复用 `Rule::is_search_enabled()` 谓词，与 `spawn_search` 派发时的 `target_sources`
    /// 列表保持一致 —— 下拉里看到的 = 实际会发请求的。
    ///
    /// 选中位置在选中的源被禁用 / 删除后会回到 `None`（`position()` 找不到），让
    /// `SelectState` 落到默认项；`spawn_search` 那边 `source_id` 仍是 stale 值，但会
    /// 因为 id 不匹配任一规则而派发空列表——用户改下拉时 Confirm 处理器会写回 None。
    fn sync_source_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let aggregate_title = ts("Search.source.aggregate");
        let mut items: Vec<SourceSelectItem> = vec![SourceSelectItem {
            value: SharedString::from("all"),
            title: aggregate_title,
        }];
        for r in self
            .model
            .read(cx)
            .rules
            .iter()
            .filter(|r| r.is_search_enabled())
        {
            // 名字兜底：空时显 "(no name)"，否则 truncate 到 30 字符避免长名字撑爆下拉。
            let name_disp = if r.name.is_empty() {
                SharedString::from("(no name)")
            } else {
                SharedString::from(truncate(&r.name, 30))
            };
            items.push(SourceSelectItem {
                value: SharedString::from(format!("rule:{}", r.id)),
                title: name_disp,
            });
        }
        // 用规则 id 升序排，确保顺序稳定。
        items.sort_by(|a, b| a.value.cmp(&b.value));
        let snapshot: Vec<SharedString> = items.iter().map(|it| it.value.clone()).collect();

        if snapshot == self.last_source_items {
            return;
        }
        self.last_source_items = snapshot;

        let items_sv: SearchableVec<SourceSelectItem> = items.into();
        let cur_value = self
            .model
            .read(cx)
            .search
            .source_id
            .map_or_else(|| "all".to_string(), |id| format!("rule:{id}"));
        let cur_value = SharedString::from(cur_value);
        let sel =
            <SearchableVec<SourceSelectItem> as SelectDelegate>::position(&items_sv, &cur_value);
        self.source_state.update(cx, |s, cx| {
            s.set_items(items_sv, window, cx);
            s.set_selected_index(sel, window, cx);
        });
    }

    /// 点"选章"按钮 → 拉 TOC + 弹 confirm Dialog。
    ///
    /// - `spawn_resolve_toc` `幂等（toc_cache` 命中直接返回，见 `app/ops/download.rs`）。
    /// - 记 `range_target`，重置 `range_initialized=false`（让 Dialog 渲染时等 TOC 回来
    ///   再 `set_value` 1 / N 初始化，避免覆盖用户输入）。
    /// - 弹反应式 confirm Dialog：builder 每帧读 `toc_cache，TOC` 回来后自动出现输入框 +
    ///   章节名预览；`on_ok` 校验范围后 `spawn_download_range` 派下载。
    fn open_range_dialog(
        &mut self,
        target: SearchResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 拉 TOC（幂等）。TOC 回来后写 toc_cache + drain loop notify → Dialog 刷新。
        self.model.update(cx, |m, _cx| m.spawn_resolve_toc(&target));
        self.range_target = Some(target);
        self.range_initialized = false;

        let page = cx.entity();
        window.open_dialog(cx, move |dialog: Dialog, window, cx| {
            // builder 是 Fn（每帧重调）→ 每帧 clone page 进当帧闭包。
            let page = page.clone();
            let body = range_dialog::content(&page, window, cx);
            dialog
                .title(ts("Search.range.title"))
                .w(px(520.))
                .child(body)
                // confirm 模式：OK + Cancel 两按钮。OK 文案"下载"。
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(ts("Search.range.confirm"))
                        .cancel_text(ts("Search.range.cancel")),
                )
                .confirm()
                // on_ok 在 Dialog 上（0.5.1 的 DialogButtonProps 无 on_ok 方法）。
                // 签名 `Fn(&ClickEvent, &mut Window, &mut App) -> bool` —— window 在这层，
                // page.update 内部拿不到 Window（只有 Context），所以下载派发放 update 里、
                // 通知用 window 在这层发，用 RangeOutcome 枚举传结果出来。
                .on_ok(move |_, window, cx| {
                    let outcome = page.update(cx, Self::confirm_range_download);
                    match outcome {
                        RangeOutcome::Done { book_name, count } => {
                            // 提示带书名 + 章节数（不用任务 id）。truncate 防超长书名撑爆 toast。
                            window.push_notification(
                                Notification::new()
                                    .title(ts("Search.action.download_started"))
                                    .message(format!(
                                        "{} · {} {}",
                                        truncate(&book_name, 50),
                                        count,
                                        ts("Search.source_status.format")
                                    ))
                                    .with_type(NotificationType::Success)
                                    .autohide(true),
                                cx,
                            );
                            true
                        }
                        RangeOutcome::Invalid => {
                            window.push_notification(
                                Notification::new()
                                    .title(ts("Search.range.title"))
                                    .message(ts("Search.range.invalid"))
                                    .with_type(NotificationType::Warning)
                                    .autohide(true),
                                cx,
                            );
                            false
                        }
                        RangeOutcome::Pending => false,
                    }
                })
        });
    }

    /// confirm Dialog 的 OK 回调：校验起止范围 → 切片章节 → `spawn_download_range`。
    /// 返回 `RangeOutcome`，`由调用方（on_ok` 闭包，持有 Window）据此发通知 + 决定是否关 Dialog。
    /// 不接 `&mut Window`：`page.update` 内部只有 `Context<SearchPage>` 拿不到 Window，
    /// 通知统一在 update 外、用 `on_ok` 自带的 window 发。
    fn confirm_range_download(&mut self, cx: &mut Context<Self>) -> RangeOutcome {
        let Some(target) = self.range_target.clone() else {
            return RangeOutcome::Pending;
        };
        let key = (target.source_id, target.url.clone());
        let Some(TocState::Loaded(book, chapters)) =
            self.model.read(cx).search.toc_cache.get(&key).cloned()
        else {
            // TOC 还没回来 —— 留着 Dialog，等 drain loop 刷新。
            return RangeOutcome::Pending;
        };

        let n = chapters.len();
        // 空值/无效值 → start 默认 1，end 默认 n。这样用户删空 start=从头开始，
        // 删空 end=下载到末尾，无需先选中数字再覆盖。
        let start = self
            .range_start_input
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|&v| v >= 1 && v <= n)
            .unwrap_or(1);
        let end = self
            .range_end_input
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|&v| v >= 1 && v <= n)
            .unwrap_or(n);
        // 即使有默认值兜底，start > end 仍然不可能（end 至少等于 n ≥ start），
        // 但保留防御性检查。
        if start > end {
            return RangeOutcome::Invalid;
        }

        // 切片：章节序号从 1 开始，转 0-based 下标。
        let selected: Vec<_> = chapters[(start - 1)..end].to_vec();
        let count = selected.len();
        // 书名：优先详情 Book（完整），否则用搜索结果的 book_name。
        let book_name = if book.book_name.trim().is_empty() {
            target.book_name.clone()
        } else {
            book.book_name.clone()
        };
        let _ = self
            .model
            .update(cx, |m, _cx| m.spawn_download_range(target, *book, selected));

        // 清掉 target，避免下次开 Dialog 误用旧状态。
        self.range_target = None;
        self.range_initialized = false;
        RangeOutcome::Done { book_name, count }
    }
}

/// `confirm_range_download` `的返回：on_ok` 闭包据此发通知 + 决定是否关 Dialog。
enum RangeOutcome {
    /// 下载已派发（书名 + 章节数）。关 Dialog。
    Done { book_name: String, count: usize },
    /// 范围无效（非数字 / 超界 / start>end）。弹 warning，留着 Dialog。
    Invalid,
    /// TOC 还没回来。留着 Dialog 等 drain loop 刷新。
    Pending,
}

impl Render for SearchPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // 差量同步书源下拉：先于其他读 model 的代码，因为这里要 &mut Window。
        self.sync_source_items(window, cx);

        // 走 list_cache：search.results 经常被 drain 更新，filter_sort 也
        // 会原地替换 results。data_version 字段保证 search.drain 末尾版本号
        // +1，下次 render 立即 miss → 重算 + 写回。
        // filter signature 此处只有 `last_keyword` 一项（搜索页无文本/扩
        // 展名过滤控件）；切 keyword 时旧 key 失效。
        let (results, running, expected, received, source_status) =
            self.model.update(cx, |model, _cx| {
                let filter_sig = crate::desktop::model::filter_signature(&[model
                    .search
                    .last_keyword
                    .as_deref()
                    .unwrap_or("")]);
                let key = crate::desktop::model::ListCacheKey {
                    page: crate::desktop::model::PageKind::Search,
                    data_version: model.search.results_version,
                    filter_sig,
                    page_index: 0, // 缓存"全表 results"；分页在 Render 末尾 slice
                    elem_type: std::any::TypeId::of::<SearchResult>(),
                };
                let results = if let Some(arc) = model.list_cache.get::<SearchResult>(key) {
                    arc
                } else {
                    model.list_cache.insert(key, model.search.results.clone())
                };
                let running = model.search.running;
                let expected = model.search.expected;
                let received = model.search.received;
                let source_status = model.search.source_status.clone();
                (results, running, expected, received, source_status)
            });

        let total = results.len();
        let w = compute_page_window(total, &mut self.current_page);
        let page_items: Vec<(usize, SearchResult)> = if w.is_empty() {
            Vec::new()
        } else {
            results[w.start..w.end]
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, r)| (w.start + i, r))
                .collect()
        };
        self.list_state.update(cx, |state, _cx| {
            state.delegate_mut().page_items = page_items;
        });

        let keyword_empty = self.keyword.read(cx).value().is_empty();

        v_flex()
            .size_full()
            .p_6()
            .gap_4()
            .child(PageHeader::new(ts("Search.page_title")).subtitle(ts("Search.page_subtitle")))
            .child(toolbar::toolbar_row(
                &self.keyword,
                &self.source_state,
                running,
                keyword_empty,
                cx,
            ))
            .when(!source_status.is_empty(), |this| {
                this.child(toolbar::source_status_row(
                    &self.model,
                    &source_status,
                    running,
                    received,
                    expected,
                    cx,
                ))
            })
            .child(if total == 0 {
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
                // List 容器边框 + 12px padding：让选中边框不被滚动条遮挡
                // （跟 library / sources 同款，详见 list_story.rs:594-602）。
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
            .when(total > 0, |this| {
                // 空态不挂分页（避免"第 1 页 / 共 0 条"无意义提示）。
                this.child(Pagination::new(
                    self.current_page,
                    w.page_count,
                    cx.listener(|this, &new_page, _window, cx| {
                        this.current_page = new_page;
                        cx.notify();
                    }),
                ))
            })
    }
}
