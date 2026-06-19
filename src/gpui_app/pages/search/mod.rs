//! Search 页面：关键词搜索 + 书源过滤 + 结果列表。
//!
//! 拆分架构（跟 `library.rs` / `sources.rs` 同模式，settings 拆完后的统一骨架）：
//! - `mod.rs`（本文件）：owner struct + impl 主体（new / run_search / open_range_dialog
//!   / confirm_range_dialog / impl Render / impl ListDelegate）。
//! - `ctx.rs`：子模块共享的 `SearchCtx<'a>` 借用视图。
//! - `source_select.rs`：选书源下拉的自定义 `SelectItem`。
//! - `toolbar.rs`：工具栏（关键词 Input + 书源 Select + 搜索 Button + 源状态 Tag）。
//! - `result_row.rs`：结果行（6 列：序号 / 书名 / 作者 / 源 / 详情 / 选章 / 全本）。
//! - `detail_dialog.rs`：详情 Dialog body + 封面解码 / 渲染。
//! - `range_dialog.rs`：选章 Dialog body + 起止输入框 clamp helper。
//!
//! 布局：
//! - PageHeader：title + subtitle（**无** 右侧 action —— 搜索按钮已下移到工具栏）
//! - Toolbar：Input（关键词） + "书源" label + Select（书源下拉） + Button（搜索）
//! - SourceStatusBar：每个源的 status badge（搜索运行时显示）
//! - ResultList：`gpui-component::List` + `SearchDelegate`（虚拟滚动）
//! - 分页页脚：30 条/页（用通用 `Pagination` 组件，始终渲染）

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, RenderImage,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, IndexPath, WindowExt,
    dialog::{Dialog, DialogButtonProps},
    input::{InputEvent, InputState, NumberInputEvent, StepAction},
    list::{List, ListDelegate, ListItem, ListState},
    notification::{Notification, NotificationType},
    select::{SearchableVec, SelectEvent, SelectState},
    v_flex,
};

use crate::app::{AppModel, TocState};
use crate::gpui_app::components::{
    EmptyState, PageHeader, Pagination, compute_page_window, truncate,
};
use crate::i18n::ts;
use crate::models::SearchResult;

use range_dialog::clamp_range_value;
use source_select::SourceSelectItem;

mod ctx;
mod detail_dialog;
mod range_dialog;
mod result_row;
mod source_select;
mod toolbar;

/// Search 页面 entity。
pub struct SearchPage {
    model: Entity<AppModel>,

    /// 关键词 Input。placeholder 在 `new()` 建 InputState 时一次性设好
    /// （`Search.filter.placeholder`）。语言切换走重启生效，新进程重建时拿新 locale。
    keyword: Entity<InputState>,

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

    /// 封面解码缓存：`cover://` URI → 解码后的 `RenderImage`。
    ///
    /// `CoverEntry` 刻意保持 UI 中立（只存原始字节，见 `app/cover.rs`），解码放 UI 层。
    /// 这里按 CoverEntry 的 `uri`（稳定去重 key）缓存 `Arc<RenderImage>`，避免 Dialog
    /// 每帧重渲染时重复解码 + 重复上传纹理（`RenderImage::new` 每次生成新 id，不缓存会
    /// 让 gpui 每帧重传纹理）。`None` = 解码失败的封面，缓存负面结果避免反复重试。
    cover_images: HashMap<String, Option<Arc<RenderImage>>>,

    // ---- 选章下载 Dialog ----
    //
    // 流程：点"选章" → `spawn_resolve_toc` 拉章节列表（回写 toc_cache）→ 弹 confirm Dialog。
    // Dialog 反应式读 toc_cache：TOC 回来后初始化起止输入框（1 / N）+ 显示章节名预览。
    // 用户改输入框 / 按 +/- → 本页订阅 `InputEvent::Change` + `NumberInputEvent::Step`，
    // clamp 到 [1, N] 后 `set_value` 写回，并刷新预览。
    /// 起始章节输入（NumberInput 绑定的 InputState）。
    range_start_input: Entity<InputState>,
    /// 结束章节输入（NumberInput 绑定的 InputState）。
    range_end_input: Entity<InputState>,
    /// 当前选章 Dialog 正在为哪条搜索结果服务。`None` = 没开 Dialog。
    /// 点击不同结果的"选章"按钮时更新；TOC 用 `(source_id, url)` 在 toc_cache 里查。
    range_target: Option<SearchResult>,
    /// 选章 Dialog 是否已为当前 target 初始化过输入框（set_value 1 / N）。
    /// 防止 TOC 每帧重渲染时反复 set_value 覆盖用户输入。
    range_initialized: bool,
}

impl SearchPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // (a) 关键词 InputState + 订阅 InputEvent::Change
        let keyword = cx.new(|cx| {
            InputState::new(window, cx).placeholder(ts("Search.filter.placeholder").to_string())
        });
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
            <SearchableVec<SourceSelectItem> as gpui_component::select::SelectDelegate>::position(
                &items, &cur_value,
            );
        let source_state =
            cx.new(|cx| SelectState::new(items, selected_pos, window, cx).searchable(true));
        cx.subscribe_in(&source_state, window, |this, _state, ev, _w, cx| {
            if let SelectEvent::Confirm(Some(value)) = ev {
                // `value` 是 `SourceSelectItem::Value = SharedString` —— 内部 id。
                // 首项 value 显式 `"all"`（不用空字符串）→ 区分 "全部" vs "rule:N"。
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

        // (d) 选章 Dialog 的起止输入框。
        //
        // 两个 InputState 各绑一个 NumberInput。订阅两类事件：
        // - `InputEvent::Change`：用户直接键入数字 → clamp 后 set_value 写回 + 刷新预览。
        // - `NumberInputEvent::Step(Decrement/Increment)`：用户按 +/- → 取当前值 ±1，clamp
        //   后 set_value。NumberInput 的 +/- 只发 Step 事件、不改值（见 gpui-component
        //   number_input.rs L106-112），所以必须自己处理。
        //
        // clamp 范围 [1, N]：N 取当前 toc_cache 里 Loaded 的章节数；TOC 没回来时按
        // `[1, u32::MAX]`（任意正整数都行，预览会等 TOC 回来再显示）。
        let range_start_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("1".to_string()));
        let range_end_input = cx.new(|cx| InputState::new(window, cx).placeholder("1".to_string()));

        // 起始输入：Change + Step 都 clamp 写回。
        // set_value 要 `&mut Window`（0.5.1 三参签名），用回调自带的 window 传入 ——
        // update 只借 cx（Context<SearchPage>），window 是独立的可变借用，不冲突。
        // clamp 用自由函数 `clamp_range_value`（不是闭包），避免订阅闭包要 'static 时
        // 反复借用 / clone 闭包。
        cx.subscribe_in(
            &range_start_input,
            window,
            |this, _state, ev, window, cx| {
                match ev {
                    InputEvent::Change => {
                        let cur = this.range_start_input.read(cx).value().to_string();
                        let v = clamp_range_value(this, cur.clone().into(), cx);
                        let want = v.to_string();
                        // **只在值不同时才 set_value**：set_value 内部会 emit Change，若每次都
                        // 无条件写回会触发 Change→set_value→Change 死循环，几轮就把 Windows
                        // 句柄配额耗尽崩溃（0x80070718）。set_value 写回的值已是规整后的，二次
                        // Change 进来时 want==cur 直接跳过，循环立即终止。
                        if want != cur {
                            this.range_start_input
                                .update(cx, |s, cx| s.set_value(want, window, cx));
                            cx.notify();
                        }
                    }
                    InputEvent::PressEnter { .. } => {}
                    _ => {}
                }
            },
        )
        .detach();
        cx.subscribe_in(
            &range_start_input,
            window,
            |this, _state, ev: &NumberInputEvent, window, cx| {
                let NumberInputEvent::Step(action) = ev;
                let cur = clamp_range_value(this, this.range_start_input.read(cx).value(), cx);
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

        // 结束输入：同上（同 start，必须只在值不同时 set_value 防 Change 死循环）。
        cx.subscribe_in(
            &range_end_input,
            window,
            |this, _state, ev, window, cx| match ev {
                InputEvent::Change => {
                    let cur = this.range_end_input.read(cx).value().to_string();
                    let v = clamp_range_value(this, cur.clone().into(), cx);
                    let want = v.to_string();
                    if want != cur {
                        this.range_end_input
                            .update(cx, |s, cx| s.set_value(want, window, cx));
                        cx.notify();
                    }
                }
                InputEvent::PressEnter { .. } => {}
                _ => {}
            },
        )
        .detach();
        cx.subscribe_in(
            &range_end_input,
            window,
            |this, _state, ev: &NumberInputEvent, window, cx| {
                let NumberInputEvent::Step(action) = ev;
                let cur = clamp_range_value(this, this.range_end_input.read(cx).value(), cx);
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
            cover_images: HashMap::new(),
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

    /// 点"选章"按钮 → 拉 TOC + 弹 confirm Dialog。
    ///
    /// - `spawn_resolve_toc` 幂等（toc_cache 命中直接返回，见 `app/ops/download.rs`）。
    /// - 记 `range_target`，重置 `range_initialized=false`（让 Dialog 渲染时等 TOC 回来
    ///   再 set_value 1 / N 初始化，避免覆盖用户输入）。
    /// - 弹反应式 confirm Dialog：builder 每帧读 toc_cache，TOC 回来后自动出现输入框 +
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

        let page = cx.entity().clone();
        window.open_dialog(cx, move |dialog: Dialog, window, cx| {
            // builder 是 Fn（每帧重调）→ 每帧 clone page 进当帧闭包。
            let page = page.clone();
            let body = range_dialog::content(page.clone(), window, cx);
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
                    let outcome = page.update(cx, |p, cx| p.confirm_range_download(cx));
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
    /// 返回 `RangeOutcome`，由调用方（on_ok 闭包，持有 Window）据此发通知 + 决定是否关 Dialog。
    /// 不接 `&mut Window`：`page.update` 内部只有 `Context<SearchPage>` 拿不到 Window，
    /// 通知统一在 update 外、用 on_ok 自带的 window 发。
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
        let start = self
            .range_start_input
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|&v| v >= 1 && v <= n);
        let end = self
            .range_end_input
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|&v| v >= 1 && v <= n);
        let (Some(start), Some(end)) = (start, end) else {
            return RangeOutcome::Invalid;
        };
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

/// `confirm_range_download` 的返回：on_ok 闭包据此发通知 + 决定是否关 Dialog。
enum RangeOutcome {
    /// 下载已派发（书名 + 章节数）。关 Dialog。
    Done { book_name: String, count: usize },
    /// 范围无效（非数字 / 超界 / start>end）。弹 warning，留着 Dialog。
    Invalid,
    /// TOC 还没回来。留着 Dialog 等 drain loop 刷新。
    Pending,
}

impl Render for SearchPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // keyword placeholder 在 `new()` 建 InputState 时一次性设好。语言切换走重启生效，
        // 无需 render 里差量刷新。
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
        let w = compute_page_window(total, &mut self.current_page);
        let page_items: Vec<(usize, SearchResult)> = if !w.is_empty() {
            results[w.start..w.end]
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, r)| (w.start + i, r))
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
            .child(toolbar::toolbar_row(
                &self.keyword,
                &self.source_state,
                running,
                keyword_empty,
                cx,
            ))
            // ---- 5. 源状态行（保留：搜索运行时显示每个源的 status）----
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
            // ---- 7. 分页页脚（仅在列表非空时渲染 —— 空态不显示，避免无意义的"第 1 页 / 共 0 条"）----
            .when(total > 0, |this| {
                this.child(Pagination::new(
                    self.current_page,
                    w.page_count,
                    cx.listener(|this, &new_page, _window, _cx| {
                        this.current_page = new_page;
                        _cx.notify();
                    }),
                ))
            })
    }
}

/// `ListDelegate` 持有当前页 items + 选中索引 + owner handle。
pub(super) struct SearchDelegate {
    /// 当前页要展示的条目，每条带"全局序号"（在完整 results 列表里的 0-based 位置）。
    /// 跨分页连续：page 0 → 0..29，page 1 → 30..59，等等。显示时 +1 变 1-based。
    page_items: Vec<(usize, SearchResult)>,
    /// 当前选中项。`None` = 未选中。`set_selected_index` 写入，`render_item` 读出来
    /// 给 `ListItem::selected(...)` 用。
    selected_index: Option<IndexPath>,
    /// 拿 SearchPage handle 用于按钮 on_click → 转发回 page。
    page: Entity<SearchPage>,
}

impl SearchDelegate {
    pub(super) fn new(page: Entity<SearchPage>) -> Self {
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
                .child(result_row::render(global_index, &result, page, &mut *cx)),
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
