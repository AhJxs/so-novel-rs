//! Search 页面：关键词搜索 + 书源过滤 + 结果列表 + 选章下载 Dialog。
//!
//! 两个大坑：书源下拉靠 render 差量同步；Dialog 的 OK 回调里不能直接开新 Dialog。

use std::num::NonZeroUsize;
use std::sync::Arc;

use lru::LruCache;

/// 详情面板"已解码封面"缓存最大条目数；超额 LRU 驱逐最久未访问。
const COVER_IMAGES_CAPACITY: NonZeroUsize = match NonZeroUsize::new(32) {
    Some(n) => n,
    None => unreachable!(),
};

use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants as _},
    dialog::AlertDialog,
    h_flex,
    input::{Input, InputEvent, InputState, NumberInputEvent, StepAction},
    list::{List, ListState},
    notification::{Notification, NotificationType},
    select::{SearchableVec, SelectDelegate, SelectEvent, SelectState},
    v_flex,
};
use gpui_kit::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, RenderImage,
    SharedString, Styled, Window, div, prelude::FluentBuilder as _, px,
};

use crate::desktop::components::{
    EmptyState, PageHeader, Pagination, compute_page_window, truncate,
};
use crate::desktop::model::{AppModel, TocState};
use crate::i18n::{ts, ts_fmt};
use crate::models::SearchResult;
use crate::models::Source;

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

    /// `InputState` / `SelectState` / `ListState` 由 owner 持有，否则 click / focus 丢失。
    keyword: Entity<InputState>,
    source_state: Entity<SelectState<SearchableVec<SourceSelectItem>>>,
    list_state: Entity<ListState<SearchDelegate>>,

    /// UI-only，每次关键词或过滤变化时重置为 0。
    current_page: usize,

    /// 书源下拉 items 的上一次快照（值为 "all" / "rule:{id}"）。`SelectState` 不会自动
    /// 重读 `model.rules`，所以由 render 比对快照重建 items；observer 拿不到 Window。
    last_source_items: Vec<SharedString>,

    /// 封面解码缓存：`cover://` URI → `RenderImage`；缓存才能避免每帧重解码 + 重传纹理
    /// （`RenderImage::new` 每次新 id）。`None` = 解码失败也存；必须限容（含完整像素）。
    cover_images: LruCache<String, Option<Arc<RenderImage>>>,

    /// 选章 Dialog 的起止输入框。TOC 回来后 Dialog 才初始化 1 / N；用户改值或按 +/-
    /// 由本页订阅事件 clamp 到 [1, N] 后写回。
    range_start_input: Entity<InputState>,
    range_end_input: Entity<InputState>,
    /// Dialog 当前服务的搜索结果（None = 没开）；TOC 用 `(source_id, url)` 查 `toc_cache`。
    range_target: Option<SearchResult>,
    /// 是否已为当前 target 初始化过输入框。防 TOC 重渲时反复 `set_value` 覆盖用户输入。
    range_initialized: bool,

    /// URL 输入 Dialog（PageHeader「下载链接」）的输入框，同 `keyword` 由 owner 持有。
    url_input: Entity<InputState>,
    /// URL 输入 Dialog 点「解析」成功后写入，由 `render()` 下一帧 drain 掉。
    /// **不能在 `on_ok` 里直接开 range Dialog**：返回 true 后组件库会 `close_dialog`
    /// （`active_dialogs.pop()`），刚 push 的 Dialog 会被自己弹掉。
    pending_range_dialog: Option<SearchResult>,
}

impl SearchPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let keyword = cx.new(|cx| {
            InputState::new(window, cx).placeholder(ts("Search.filter.placeholder").to_string())
        });
        cx.subscribe_in(&keyword, window, |this, _state, ev, w, cx| match ev {
            InputEvent::Change => {
                let v = this.keyword.read(cx).value().to_string();
                this.model.update(cx, |m, _cx| m.search.keyword = v);
            }
            InputEvent::PressEnter { .. } => this.run_search(w, cx),
            _ => {}
        })
        .detach();

        // 选书源 SelectState。items 首次为空，由 render 里的 `sync_source_items` 重建。
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

        // 起止输入框：Change 事件 clamp 后写回，Step 事件（+/-）由自己算值 —— NumberInput
        // 的 +/- 只发事件不改值。clamp 范围 [1, N]，N 取不到时按 [1, u32::MAX]。
        let range_start_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("1".to_string()));
        let range_end_input = cx.new(|cx| InputState::new(window, cx).placeholder("1".to_string()));

        // URL 输入 Dialog 的 InputState —— PageHeader「下载链接」按钮唤起。
        let url_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(ts("Search.url_download.placeholder").to_string())
        });

        // 只在值不同时 set_value：无条件写回会 Change→set_value→Change 死循环，几轮耗尽
        // Windows 句柄配额崩溃（0x80070718）。写回值已是规整值，二次 Change 直接跳过。
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

        // Step 订阅（+/-）：NumberInput 只发事件不改值，这里自己算并写回。
        cx.subscribe_in(
            &range_start_input,
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
            url_input,
            pending_range_dialog: None,
        }
    }

    /// 点"搜索"按钮 → 同步 keyword 到 model，调 `spawn_search`。关键词空 / 已在跑时
    /// 按钮已 disabled，`!started` 只是防御。
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

    /// 书源下拉 items 差量同步：快照（只有 `value` = "all" / "rule:{id}"）没变就直接返回，
    /// 变了才重建 items、按 `model.search.source_id` 重算选中并推给 `SelectState`。
    ///
    /// 复用 `Rule::is_search_enabled()`，与 `spawn_search` 的 `target_sources` 一致。注意：
    /// 选中的源被禁用 / 删除后 `position()` 找不到会回落默认项，而 `spawn_search` 侧的
    /// stale `source_id` 会派发空列表。
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
        // 用规则 id 升序排，保证顺序稳定。
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

    /// PageHeader「下载链接」回调：弹 URL 输入 Dialog（自动粘贴剪贴板）→ 匹配书源 →
    /// 构造最小 `SearchResult` 后复用 `open_range_dialog`。
    fn open_url_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        // 自动粘贴：Dialog 打开时填入剪贴板的 URL（只认 http(s)）。
        if let Some(s) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let trimmed = s.trim();
            if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
                self.url_input.update(cx, |state, cx| {
                    state.set_value(trimmed.to_string(), window, cx);
                });
            }
        }

        let page = cx.entity();
        window.open_alert_dialog(cx, move |alert: AlertDialog, _window, cx| {
            let page = page.clone();
            let url_input = page.read(cx).url_input.clone();
            let body = v_flex()
                .gap_2()
                .child(Input::new(&url_input).cleanable(true))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Button::new("url-paste")
                                .small()
                                .ghost()
                                .label(ts("Search.url_download.paste_button"))
                                .on_click(move |_, window, cx| {
                                    // 兜底：Dialog 打开后剪贴板被新内容覆盖时，重新读一次。
                                    if let Some(s) =
                                        cx.read_from_clipboard().and_then(|item| item.text())
                                    {
                                        let trimmed = s.trim();
                                        if trimmed.starts_with("http://")
                                            || trimmed.starts_with("https://")
                                        {
                                            url_input.update(cx, |state, cx| {
                                                state.set_value(trimmed.to_string(), window, cx);
                                            });
                                        }
                                    }
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(ts("Search.url_download.auto_pasted")),
                        ),
                );
            // 复杂 body 走 `.child(body)`；宽用 AlertDialog 的 `.width()`。
            alert
                .title(ts("Search.url_download.dialog_title"))
                .width(px(520.))
                .child(body)
                .ok_text(ts("Search.url_download.confirm"))
                .cancel_text(ts("Search.url_download.cancel"))
                .confirm()
                .on_ok(move |_ev, _window, cx| {
                    let url = page.read(cx).url_input.read(cx).value().to_string();
                    let url = url.trim().to_string();
                    if url.is_empty() {
                        page.update(cx, |p, cx| {
                            p.model.update(cx, |m, _cx| {
                                m.push_warning(ts("Search.url_download.no_match"));
                            });
                            cx.notify();
                        });
                        return true;
                    }
                    // rules + config 先取 owned 副本，避免闭包里反复 read 触发借用冲突。
                    let (rules, config) = {
                        let m = page.read(cx).model.read(cx);
                        (m.rules.clone(), m.config.clone())
                    };
                    let source = rules.iter().find_map(|r| {
                        let sources = [Source::from(r.clone(), &config)];
                        crate::core::sources::match_source_by_url(&sources, &url)
                            .map(|s| s.rule.clone())
                    });
                    let Some(source) = source else {
                        page.update(cx, |p, cx| {
                            p.model.update(cx, |m, _cx| {
                                m.push_warning(ts("Search.url_download.no_match"));
                            });
                            cx.notify();
                        });
                        return true;
                    };
                    // 复用 open_range_dialog：它只用到 source_id / source_name / url。
                    let target = SearchResult {
                        source_id: source.id,
                        source_name: source.name.clone(),
                        url,
                        book_name: String::new(),
                        author: None,
                        intro: None,
                        category: None,
                        latest_chapter: None,
                        last_update_time: None,
                        status: None,
                        word_count: None,
                    };
                    page.update(cx, |p, cx| {
                        p.model.update(cx, |m, _cx| {
                            m.push_success(ts_fmt(
                                "Search.url_download.matched_source",
                                &[("name", &source.name)],
                            ));
                        });
                        // 不能直接开 range Dialog：on_ok 返回 true 后组件库会 pop 栈顶，
                        // 刚 push 的 Dialog 会被弹掉。改为置 flag，render 下一帧接手。
                        p.pending_range_dialog = Some(target);
                        cx.notify();
                    });
                    true // 关 URL Dialog，range Dialog 在下一帧 render() 里接管
                })
        });
    }

    /// 点"选章"按钮 → 拉 TOC + 弹 confirm Dialog。
    ///
    /// `spawn_resolve_toc` 幂等；`range_initialized=false` 让 Dialog 等 TOC 回来才初始化，
    /// 避免覆盖用户已输入的值。
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
        window.open_alert_dialog(cx, move |alert: AlertDialog, window, cx| {
            let page = page.clone();
            let body = range_dialog::content(&page, window, cx);
            alert
                .title(ts("Search.range.title"))
                .width(px(520.))
                .child(body)
                .ok_text(ts("Search.range.confirm"))
                .cancel_text(ts("Search.range.cancel"))
                .confirm()
                // on_ok 挂 Dialog 上（带 `&mut Window`）；page.update 内部只有 Context，
                // 所以下载在 update 里派发、通知在这层发，结果用 RangeOutcome 传出。
                .on_ok(move |_, window, cx| {
                    let outcome = page.update(cx, Self::confirm_range_download);
                    match outcome {
                        RangeOutcome::Done { book_name, count } => {
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

    /// confirm Dialog 的 OK 回调：校验范围 → 切片章节 → `spawn_download_range`。
    /// 收 `Context` 而非 Window，通知由持有 Window 的调用方发。
    fn confirm_range_download(&mut self, cx: &mut Context<Self>) -> RangeOutcome {
        let Some(target) = self.range_target.clone() else {
            return RangeOutcome::Pending;
        };
        let key = (target.source_id, target.url.clone());
        let Some(TocState::Loaded(book, chapters)) =
            self.model.read(cx).search.toc_cache.get(&key).cloned()
        else {
            // TOC 还没回来 —— 留着 Dialog 等 drain loop 刷新。
            return RangeOutcome::Pending;
        };

        let n = chapters.len();
        // 空值 / 无效值 → start 取 1、end 取 n：删空 start 表示从头，删空 end 表示到末尾。
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
        // 兜底之下 start > end 已不可能，仍保留防御性检查。
        if start > end {
            return RangeOutcome::Invalid;
        }

        // 章节序号 1-based → 0-based 下标。
        let selected: Vec<_> = chapters[(start - 1)..end].to_vec();
        let count = selected.len();
        // 书名优先用详情 Book，缺失时退回搜索结果的 book_name。
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

/// `confirm_range_download` 的返回：调用方据此发通知 + 决定是否关 Dialog。
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
        // 排空「URL Dialog 解析成功 → 弹 range Dialog」的挂起请求。必须在最前面：此时
        // URL Dialog 已被 close_dialog pop 掉、栈空，push 的新 Dialog 才不会被覆盖。
        if let Some(target) = self.pending_range_dialog.take() {
            self.open_range_dialog(target, window, cx);
        }

        // 差量同步书源下拉：这里要 `&mut Window`，所以先做。
        self.sync_source_items(window, cx);

        // 走 list_cache：drain / filter_sort 会原地替换 search.results，靠 results_version
        // 递增让旧 key 立即失效并重算。filter_sig 目前只有 `last_keyword`（本页无其它过滤控件）。
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
                    page_index: 0, // 缓存全表 results，分页在 render 末尾 slice
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
            .child(
                PageHeader::new(ts("Search.page_title"))
                    .subtitle(ts("Search.page_subtitle"))
                    .action(
                        Button::new("search-url-download")
                            .icon(Icon::new(IconName::ExternalLink))
                            .label(ts("Search.url_download.button"))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_url_dialog(window, cx);
                            })),
                    ),
            )
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
                // 列表容器边框 + 12px padding：选中边框不被滚动条遮挡。
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
            // 页脚可见性由 `Pagination` 自己判。
            .child(Pagination::new(
                self.current_page,
                w.page_count,
                cx.listener(|this, &new_page, _window, cx| {
                    this.current_page = new_page;
                    cx.notify();
                }),
            ))
    }
}
