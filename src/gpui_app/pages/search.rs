//! Search 页面：关键词搜索 + 书源过滤 + 结果列表。
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
//! - 详情按钮弹只读 Dialog（`gpui_component::Dialog`）展示 SearchResult 全字段。
//! - 选书源下拉：用 `SelectState<SearchableVec<SharedString>>`，
//!   第一项 value = `""`（"全部" = None），其余 `format!("rule:{id}")` 编码 source id。
//! - i18n：所有新增 UI 字符串走 `Search.*` 命名空间（locales/app.yml × 3 locale）。

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use gpui::{
    App, AppContext, Context, Entity, ImageSource, IntoElement, ObjectFit, ParentElement, Render,
    RenderImage, SharedString, Styled, Window, div, img, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable, Icon, IconName, IndexPath, Sizable, WindowExt,
    button::Button,
    dialog::{Dialog, DialogButtonProps},
    h_flex,
    input::{Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction},
    link::Link,
    list::{List, ListDelegate, ListItem, ListState},
    notification::{Notification, NotificationType},
    select::{SearchableVec, Select, SelectDelegate, SelectEvent, SelectItem, SelectState},
    spinner::Spinner,
    tag::Tag,
    v_flex,
};

use gpui::StyledImage as _;

use crate::app::{AppModel, CoverEntry, DetailState, SourceStatus, TocState};
use crate::gpui_app::components::{
    EmptyState, PageHeader, Pagination, compute_page_window, truncate,
};
use crate::gpui_app::i18n::{ts, ts_fmt};
use crate::models::SearchResult;

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
///   `SharedString`，解析逻辑 `v == "all" → None;
///   v.strip_prefix("rule:").and_then(parse) → Some(id)`。
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
            <SearchableVec<SourceSelectItem> as SelectDelegate>::position(&items, &cur_value);
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
            let body = render_range_dialog_content(page.clone(), window, cx);
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

/// 把输入框原始值规整到 `[1, N]`：N 取当前选章 Dialog 的 toc_cache Loaded 章节数。
/// 非数字 / 越界 → 1。N 取不到（TOC 没回来）时按 `[1, u32::MAX]`（任意正整数）。
///
/// 自由函数（不是闭包）—— 订阅闭包要 `'static`，自由函数无捕获，4 个订阅共用它无需 clone。
fn clamp_range_value(this: &SearchPage, raw: SharedString, cx: &App) -> u32 {
    let n = this
        .current_range_chapters_len(cx)
        .unwrap_or(u32::MAX)
        .max(1);
    let v = raw.trim().parse::<u32>().unwrap_or(1);
    v.clamp(1, n)
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
                                    Tag::danger().outline().child(name.clone())
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
    /// 拿 SearchPage handle 用于按钮 on_click → 转发回 page。
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
    // 详情按钮：page clone 进 on_click → open_dialog builder。封面是反应式的
    // （每帧读 live cover_cache），所以 result 的文本字段也要进 builder 闭包 ——
    // 用 Box 改 FnOnce 语义，让 builder 只能调一次（builder 本来也只每帧调一次）。
    let result_for_detail = r.clone();
    let page_for_detail = page.clone();
    let source_id_for_detail = r.source_id;
    let url_for_detail = r.url.clone();
    // 选章按钮：page clone 进 on_click → open_range_dialog。
    let result_for_range = r.clone();
    let page_for_range = page.clone();

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
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(source_name_display),
                ),
        )
        // ---- 详情按钮（弹只读 Dialog 展示 SearchResult 全字段）----
        .child(
            Button::new(("search-detail", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::Info))
                .label(ts("Search.detail.action"))
                .on_click(move |_, window, cx| {
                    // 1) 拉详情（幂等：detail_cache 命中直接返回）。详情后端返回 cover_url
                    //    后，drain loop 自动派发封面下载（见 app/events.rs L43-50）。
                    page_for_detail.update(cx, |p, cx| {
                        p.model.update(cx, |m, _cx| m.select_search_result(idx));
                    });

                    // 2) 弹反应式 Dialog：builder 每帧被 RootView render 重调，每帧重新
                    //    clone result 文本 + 重新读 live cover_cache，所以封面到达后会自动
                    //    刷新（drain loop 100ms notify → RootView 重 render → builder 重调）。
                    //
                    //    builder 是 `Fn`（每帧重调），但我们要 move result 进去给它读文本字段 ——
                    //    result 是 Clone 的，每次 builder 被调前重新 clone 一份 move 进当帧
                    //    闭包即可，所以 on_click 外层把 result 留在闭包外只 clone 不 move。
                    let page = page_for_detail.clone();
                    let r = result_for_detail.clone();
                    let source_id = source_id_for_detail;
                    let url = url_for_detail.clone();
                    window.open_dialog(cx, move |dialog: Dialog, _window, _cx| {
                        // 每帧重新 clone 文本字段（builder 是 Fn，可多次调）。
                        let r = r.clone();
                        let page = page.clone();
                        let url = url.clone();
                        dialog
                            .title(ts("Search.detail.title"))
                            .w(px(640.))
                            .child(render_detail_content(r, page, source_id, url, _cx))
                    });
                }),
        )
        // ---- 选章按钮（拉 TOC + 弹 confirm Dialog 选起止章节下载）----
        .child(
            Button::new(("search-chapters", idx as u64))
                .small()
                .outline()
                .icon(Icon::new(IconName::ChevronRight))
                .label(ts("Search.action.select_chapters"))
                .on_click(move |_, window, cx| {
                    // on_click 是 Fn → 每次重新 clone 一份 result 喂给 open_range_dialog。
                    let r = result_for_range.clone();
                    page_for_range.update(cx, |p, cx| p.open_range_dialog(r, window, cx));
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
                    let _ = page_for_whole.update(cx, |p, cx| {
                        p.model
                            .update(cx, |m, _cx| m.spawn_download(result_for_click))
                    });
                    // 提示带书名（不用任务 id —— id 对用户无意义）。truncate 防超长书名撑爆 toast。
                    window.push_notification(
                        Notification::new()
                            .title(ts("Search.action.download_started"))
                            .message(truncate(&result_for_whole.book_name, 50).to_string())
                            .with_type(NotificationType::Success)
                            .autohide(true),
                        cx,
                    );
                }),
        )
}

// ============================================================================
// 详情 Dialog 内容渲染
// ============================================================================

/// 渲染详情 Dialog 的 body：左侧封面 + 右侧字段列表。
///
/// 布局 `h_flex`：左封面固定 `COVER_W × COVER_H`，右字段 `flex_1`。Dialog 自身 body 是
/// `overflow_y_scrollbar`（见 gpui-component Dialog::render），字段多 / 简介长可滚动查看。
///
/// 封面是反应式的：`render_detail_cover` 每帧重读 live `cover_cache`，封面到达后自动刷新
/// （drain loop 100ms notify → RootView 重 render → Dialog builder 重调本函数）。
fn render_detail_content(
    r: SearchResult,
    page: Entity<SearchPage>,
    source_id: i32,
    url: String,
    cx: &mut App,
) -> impl IntoElement {
    // 详情是再次请求拿的完整字段（intro / category / status / latest / last_update / author），
    // 搜索结果里这些是空的。优先用 detail_cache 的 Book；detail 还没回来时用 SearchResult
    // 兜底（intro 会显 unknown），drain loop 把 Book 拉回来后自动切到完整数据。
    let book = page
        .read(cx)
        .model
        .read(cx)
        .search
        .detail_cache
        .get(&(source_id, url.clone()))
        .and_then(|s| s.book().cloned());
    let b = book.as_ref();

    // source_name / word_count 只有 SearchResult 有（Book 不带），永远用 r。
    let source_val = if r.source_name.is_empty() {
        ts("Search.detail.unknown").to_string()
    } else {
        r.source_name.clone()
    };

    // 合并：detail-only 字段优先 Book，Book 为空时回退 SearchResult。
    // book_name / author（Book 里是 String）非空才取，否则用 r。
    let book_name = b
        .map(|x| x.book_name.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| r.book_name.clone());
    let author = match b {
        Some(x) if !x.author.trim().is_empty() => SharedString::from(x.author.clone()),
        _ => detail_opt(r.author.as_deref()),
    };
    let category = b
        .and_then(|x| x.category.as_deref())
        .or(r.category.as_deref());
    let status = b.and_then(|x| x.status.as_deref()).or(r.status.as_deref());
    let latest = b
        .and_then(|x| x.latest_chapter.as_deref())
        .or(r.latest_chapter.as_deref());
    let last_update = b
        .and_then(|x| x.last_update_time.as_deref())
        .or(r.last_update_time.as_deref());
    let intro = b.and_then(|x| x.intro.as_deref()).or(r.intro.as_deref());

    // 链接行：label + 可点击 Link（自带 link 色 / 下划线 / hover，点击 cx.open_url 打开）。
    let url_display = if r.url.trim().is_empty() {
        ts("Search.detail.unknown")
    } else {
        SharedString::from(r.url.clone())
    };
    let url_link = h_flex()
        .gap_3()
        .items_start()
        .child(
            div()
                .w(px(84.0))
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(ts("Search.detail.field.url")),
        )
        .child(
            // Link 自带 cursor_pointer + 点击 open_url；长 URL 装进会换行的容器里，
            // overflow_x_hidden + min_w_0 防止撑爆行宽。
            div()
                .flex_1()
                .min_w_0()
                .overflow_x_hidden()
                .text_sm()
                .child(
                    // gpui 0.2.2 无 break_all —— URL 无空格不会自动换行，用 overflow_x_hidden
                    // 截断超长部分（用户点开链接即可看完整 URL）。
                    Link::new("detail-url")
                        .href(r.url.clone())
                        .child(url_display),
                ),
        );

    // 右侧字段列表：所有「label + value」行竖排。
    let fields = v_flex()
        .gap_2()
        .child(detail_row(
            ts("Search.detail.field.book_name"),
            SharedString::from(book_name),
            cx,
        ))
        .child(detail_row(ts("Search.detail.field.author"), author, cx))
        .child(detail_row(
            ts("Search.detail.field.source"),
            SharedString::from(source_val),
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.category"),
            detail_opt(category),
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.status"),
            detail_opt(status),
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.latest_chapter"),
            detail_opt(latest),
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.last_update"),
            detail_opt(last_update),
            cx,
        ))
        .child(detail_row(
            ts("Search.detail.field.intro"),
            detail_opt(intro),
            cx,
        ))
        .child(url_link);

    // 左封面 + 右字段。封面 flex_shrink_0 固定，字段 flex_1 撑满。顶部对齐。
    h_flex()
        .gap_4()
        .items_start()
        .child(render_detail_cover(page, source_id, &url, cx))
        .child(fields.flex_1().min_w_0())
}

/// `Option<&str>` → 显示值；`None` / 纯空白 → `Search.detail.unknown` fallback。
///
/// 接 `Option<&str>`（而非 `&Option<String>`）方便合并：detail(Book) + SearchResult
/// 两边 `as_deref()` 后 `.or()` 合出 `Option<&str>` 直接喂进来。
fn detail_opt(v: Option<&str>) -> SharedString {
    match v {
        Some(s) if !s.trim().is_empty() => SharedString::from(s.to_string()),
        _ => ts("Search.detail.unknown"),
    }
}

/// 详情 Dialog 的「label + value」行：label 固定 84px、muted、xs；value flex_1、可换行。
fn detail_row(label: SharedString, value: SharedString, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_3()
        .items_start()
        .child(
            div()
                .w(px(84.0))
                .flex_shrink_0()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                // min_w_0 让 flex 子项能收缩到内容以下，长 value 不会把行宽撑爆。
                .min_w_0()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(value),
        )
}

// ============================================================================
// 封面解码 + 渲染
// ============================================================================

/// 详情 Dialog 里封面区固定尺寸（宽 × 高）。封面比例不一，统一容器 + `ObjectFit::Contain`
/// 居中显示，留白用 muted 背景，跟空态 / 失败占位共用同一个框。
const COVER_W: f32 = 120.0;
const COVER_H: f32 = 170.0;

/// 解码封面原始字节 → `Arc<RenderImage>`。
///
/// `CoverEntry` 是 UI 中立的（只存原图字节，见 `app/cover.rs`），解码必须放 UI 层。
/// 流程跟 gpui 自己的 `AssetLoader::<ImageDecoder>` 内部一致（`img.rs` L669-692）：
/// `image::ImageReader` → `into_rgba8()` → RGBA↔BGRA swap（GPUI 纹理是 BGRA）→ `Frame` → `RenderImage`。
///
/// 失败返回 `None`（不是 panic）—— 调用方缓存负面结果，避免每帧重试解码。
fn decode_cover_image(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    // with_guessed_format 让 image crate 按 magic bytes 推断格式（PNG/JPEG/WebP/…）。
    let reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    // 二次保险：解码前再校验是有效图片 —— CoverEntry::Ready 已在下载时 probe 过一次，
    // 但缓存可能跨进程/异常，这里 probe 一次更稳，且只是几 µs 的开销。
    let dynamic = reader.decode().ok()?;
    let mut rgba = dynamic.into_rgba8();

    // RGBA → BGRA：GPUI 纹理期望 BGRA 字节序（见 gpui img.rs L671-674 swap(0,2)）。
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }

    // `Frame` 是 `image::Frame`（跟 gpui 的 `RenderImage::new` 内部一致，见 gpui img.rs
    // L669-692 用 `image::Frame::new`）。**别**写成 `gpui::Frame` —— 那是 window 模块的
    // dispatch tree Frame（pub(crate)，外部不可构造，类型也对不上）。
    let frame = image::Frame::new(rgba);
    Some(Arc::new(RenderImage::new(vec![frame])))
}

/// 渲染详情 Dialog 的封面区。
///
/// 封面要两级查找（封面不在 `SearchResult` 里，得先拉详情拿 `cover_url` 再下载字节）：
/// 1. `detail_cache[(source_id, url)]` → `DetailState::Loaded(book)` → `book.cover_url`
/// 2. `cover_cache[(source_id, cover_url)]` → `CoverEntry::Ready { bytes, uri }` → 解码
///
/// 状态分支：
/// - `DetailState::Pending` / detail 未拉 → 显示「封面加载中…」（drain loop 100ms 后刷新）
/// - `Loaded` 但无 `cover_url` → 「无封面」
/// - 有 `cover_url` 但 `cover_cache` 还没到 / `Failed` → 「封面加载中…」/「封面获取失败」
/// - `CoverEntry::Ready` → 命中本页解码缓存就渲染，未命中就解码 + 写缓存再渲染
///
/// `page: Entity<SearchPage>`：本页 `cover_images` 缓存是 `&mut self` 字段，必须通过
/// `page.update` 拿可变借用写缓存。读 model 也走 `page.model`，避免在已借 `model` 时再借。
fn render_detail_cover(
    page: Entity<SearchPage>,
    source_id: i32,
    url: &str,
    cx: &mut App,
) -> impl IntoElement {
    // 封面/详情状态机：返回 (要渲染的内容, 固定容器的额外样式)。
    // 默认 loading 占位；下面分支按 detail→cover 两级覆盖。
    enum CoverView {
        Loading,
        Failed,
        None,
        Image(Arc<RenderImage>),
    }

    let view = page.update(cx, |p, cx| {
        // ---- 第一级：detail_cache 拿 cover_url ----
        let detail = p
            .model
            .read(cx)
            .search
            .detail_cache
            .get(&(source_id, url.to_string()));
        match detail {
            None | Some(DetailState::Pending) => CoverView::Loading,
            Some(DetailState::Failed(_)) => CoverView::Failed,
            Some(DetailState::Loaded(book)) => {
                let Some(cover_url) = book
                    .cover_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    return CoverView::None;
                };

                // ---- 第二级：cover_cache 拿字节 ----
                let cover = p
                    .model
                    .read(cx)
                    .search
                    .cover_cache
                    .get(&(source_id, cover_url.to_string()));
                match cover {
                    Some(CoverEntry::Ready { bytes, uri }) => {
                        // 命中本页解码缓存就复用；否则解码 + 写缓存。
                        if let Some(cached) = p.cover_images.get(uri).cloned() {
                            match cached {
                                Some(img) => CoverView::Image(img),
                                None => CoverView::Failed,
                            }
                        } else {
                            match decode_cover_image(bytes) {
                                Some(img) => {
                                    p.cover_images.insert(uri.clone(), Some(img.clone()));
                                    CoverView::Image(img)
                                }
                                None => {
                                    p.cover_images.insert(uri.clone(), None);
                                    CoverView::Failed
                                }
                            }
                        }
                    }
                    Some(CoverEntry::Failed(_)) => CoverView::Failed,
                    None => CoverView::Loading,
                }
            }
        }
    });

    // 固定容器：muted 底 + 圆角 + 居中内容。封面 / 占位文案都进同一个框，保证布局稳定。
    let container = div()
        .w(px(COVER_W))
        .h(px(COVER_H))
        .flex_shrink_0()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden();

    match view {
        CoverView::Image(rendered) => container.child(
            // `img` 是 gpui 的自由函数（`gpui::img(source)`），不是上面的变量 ——
            // 变量改名 `rendered` 避免遮蔽。
            img(ImageSource::Render(rendered))
                .object_fit(ObjectFit::Contain)
                .size_full(),
        ),
        CoverView::Loading => container.child(
            v_flex()
                .gap_1()
                .items_center()
                .child(Spinner::new().small())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(ts("Search.detail.cover.loading")),
                ),
        ),
        CoverView::Failed => container.child(
            div()
                .p_2()
                .text_center()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(ts("Search.detail.cover.failed")),
        ),
        CoverView::None => container.child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(ts("Search.detail.cover.none")),
        ),
    }
}

// ============================================================================
// 选章下载 Dialog 内容渲染
// ============================================================================

/// 渲染选章 Dialog 的 body。
///
/// 反应式读 `toc_cache[(source_id, url)]`：
/// - `Pending` / 未拉到 → loading 占位（drain loop 100ms 后刷新）
/// - `Loaded(book, chapters)` → 首次 set_value 初始化 1 / N，显示「共 N 章」+ 起止
///   NumberInput + 选中范围首尾章节名预览
/// - `Failed` → 错误占位
///
/// 就绪与否由 on_ok 通过 `confirm_range_download` 返回的 `RangeOutcome::Pending` 判定，
/// 这里只负责渲染。
fn render_range_dialog_content(
    page: Entity<SearchPage>,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    // 先取 toc 状态（读 model，只读借用）。
    let target = page.read(cx).range_target.clone();
    let toc = target.as_ref().and_then(|t| {
        page.read(cx)
            .model
            .read(cx)
            .search
            .toc_cache
            .get(&(t.source_id, t.url.clone()))
            .cloned()
    });

    match toc {
        None | Some(TocState::Pending) => h_flex()
            .gap_2()
            .items_center()
            .py_4()
            .child(Spinner::new().small())
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(ts("Search.range.loading")),
            )
            .into_any_element(),
        Some(TocState::Failed(msg)) => div()
            .py_4()
            .text_sm()
            .text_color(cx.theme().danger_foreground)
            .child(format!("{}: {msg}", ts("Search.range.failed")))
            .into_any_element(),
        Some(TocState::Loaded(_book, chapters)) => {
            let n = chapters.len();
            // 首次进来初始化起止输入框：1 / N。range_initialized 防止每帧覆盖用户输入。
            // set_value 要 `&mut Window`，update 只借 cx，window 独立借用不冲突。
            page.update(cx, |p, cx| {
                if !p.range_initialized {
                    p.range_start_input
                        .update(cx, |s, cx| s.set_value("1".to_string(), window, cx));
                    p.range_end_input
                        .update(cx, |s, cx| s.set_value(n.to_string(), window, cx));
                    p.range_initialized = true;
                }
            });

            // 读当前输入框值，算预览的首尾章节名。
            let start_v = page
                .read(cx)
                .range_start_input
                .read(cx)
                .value()
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|&v| v >= 1 && v <= n)
                .unwrap_or(1);
            let end_v = page
                .read(cx)
                .range_end_input
                .read(cx)
                .value()
                .trim()
                .parse::<usize>()
                .ok()
                .filter(|&v| v >= 1 && v <= n)
                .unwrap_or(n);
            let (lo, hi) = if start_v <= end_v {
                (start_v, end_v)
            } else {
                (end_v, start_v)
            };
            let start_title = chapter_title_display(&chapters, lo);
            let end_title = chapter_title_display(&chapters, hi);
            let count = hi.saturating_sub(lo) + 1;

            // 布局：共 N 章 → 起止 NumberInput（label + 输入框）→ 选中预览（首尾章名各一行）。
            v_flex()
                .gap_3()
                // 共 N 章
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(ts_fmt("Search.range.total", &[("n", &n.to_string())])),
                )
                // 起止输入行
                .child(
                    h_flex()
                        .gap_4()
                        .items_center()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(ts("Search.range.start")),
                                )
                                // 加宽到 160px（minus/plus 按钮各占 ~28px，留 ~100px 给数字）。
                                // 注：gpui-component 0.5.1 的 Input/NumberInput 不支持文本水平
                                // 居中——数字由自定义 element 固定左对齐绘制，无对齐 API，
                                // 外层 styled 的 text_align 也不会被内部 Input 继承。接受左对齐。
                                .child(
                                    NumberInput::new(&page.read(cx).range_start_input).w(px(160.0)),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(ts("Search.range.end")),
                                )
                                .child(
                                    NumberInput::new(&page.read(cx).range_end_input).w(px(160.0)),
                                ),
                        ),
                )
                // 选中预览：首尾章名各一行（避免长章名挤一行被换行成两段）。
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} ({} {})",
                                    ts("Search.range.preview"),
                                    count,
                                    ts("Search.source_status.format")
                                )),
                        )
                        // 起始章名：truncate 防超长。
                        .child(
                            div().text_sm().text_color(cx.theme().foreground).child(
                                div()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .overflow_x_hidden()
                                    .child(format!("{}", start_title)),
                            ),
                        )
                        // 结束章名：truncate 防超长。
                        .child(
                            div().text_sm().text_color(cx.theme().foreground).child(
                                div()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .overflow_x_hidden()
                                    .child(format!("{}", end_title)),
                            ),
                        ),
                )
                .into_any_element()
        }
    }
}

/// 取第 `n` 章（1-based）的显示标题；越界 / 空标题走 fallback。
fn chapter_title_display(chapters: &[crate::models::Chapter], n: usize) -> SharedString {
    match chapters.get(n.saturating_sub(1)) {
        Some(c) if !c.title.trim().is_empty() => {
            SharedString::from(format!("{}. {}", n, truncate(&c.title, 40)))
        }
        _ => SharedString::from(format!("{}. {}", n, ts("Search.range.no_title"))),
    }
}
